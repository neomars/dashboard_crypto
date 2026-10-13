//! Rapport complet de la page « Fibonacci à levier » : simulation sur la période
//! d'étude et sur la période de test, niveaux placebo, Monte-Carlo, même jeu de
//! trades à x1, verdict. Aucun ordre réel n'est passé.
//!
//! **Protocole.** Les réglages se choisissent sur la période d'étude ; le verdict
//! ne se lit que sur la période de test, postérieure et jamais utilisée pour
//! régler. Les deux périodes sont rejouées séparément (budget remis à zéro), avec
//! quelques jours de bougies avant chacune pour amorcer l'identification des
//! points sans qu'aucun trade n'y soit pris.

use crate::candle_store::{CandleStore, SeriesKey};
use crate::chart::Chart;
use crate::contract::{self, ContractSpec};
use crate::feed::MarketFeed;
use crate::fibo::{aggregate, analyze, current_points, CurrentPoints, FiboParams};
use crate::fibo_chart::trade_zoom_chart;
use crate::fibo_chart::{budget_chart, monte_carlo_chart, placebo_chart, price_chart, r_histogram};
use crate::leveraged::{
    simulate, Account, Costs, SimInput, SimParams, SimResult, SimStats, Trade, TradeRules,
};
use crate::market::{date_ms, fmt_time, Bar, FundingRate, Interval, DAY_MS};
use crate::market::{Instrument, Market};
use crate::validation::{check_excursions, ExcursionCheck};
use crate::validation::{
    per_level, run_placebo, summarize, verdict, LevelDetail, MonteCarlo, PlaceboParams,
    PlaceboResult, RunSummary, VerdictInfo,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::atomic::AtomicBool;

/// Demande de l'interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiboRequest {
    pub base: String,
    pub fibo: FiboParams,
    pub rules: TradeRules,
    pub costs: Costs,
    pub account: Account,
    /// Période d'étude (réglages), bornes incluses.
    pub study_start: NaiveDate,
    pub study_end: NaiveDate,
    /// Période de test (jugement), bornes incluses.
    pub test_start: NaiveDate,
    pub test_end: NaiveDate,
    pub placebo: PlaceboParams,
    /// Tirages Monte-Carlo (10 000 par défaut).
    #[serde(default = "default_mc_draws")]
    pub mc_draws: usize,
    /// Utiliser l'historique du prix de marque pour les liquidations, s'il est en cache.
    #[serde(default)]
    pub use_mark_price: bool,
}

fn default_mc_draws() -> usize {
    10_000
}

impl FiboRequest {
    pub fn validate(&self, spec: &ContractSpec) -> Result<(), String> {
        self.fibo.validate()?;
        self.placebo.validate()?;
        if self.mc_draws == 0 || self.mc_draws > 200_000 {
            return Err(
                "Le nombre de tirages Monte-Carlo doit être compris entre 1 et 200 000.".into(),
            );
        }
        if self.study_start > self.study_end || self.test_start > self.test_end {
            return Err("Une période commence après sa fin.".into());
        }
        if self.test_start <= self.study_end {
            return Err("La période de test doit commencer après la période d'étude : sinon le jugement n'est plus hors échantillon.".into());
        }
        self.sim_params(self.study_start, self.study_end)
            .validate(spec)
    }

    fn sim_params(&self, start: NaiveDate, end: NaiveDate) -> SimParams {
        SimParams {
            fibo: self.fibo.clone(),
            rules: self.rules,
            costs: self.costs,
            account: Account {
                r_only: false,
                ..self.account
            },
            start_ms: date_ms(start),
            end_ms: date_ms(end) + DAY_MS,
        }
    }

    /// Premier instant de bougies nécessaire : début de l'étude moins le préchauffage.
    pub fn data_start_ms(&self) -> i64 {
        date_ms(self.study_start) - warmup_ms(self.fibo.interval)
    }

    pub fn data_end_ms(&self) -> i64 {
        date_ms(self.test_end) + DAY_MS
    }
}

/// Préchauffage de l'identification des points : 300 bougies de détection, au moins 7 jours.
pub fn warmup_ms(interval: Interval) -> i64 {
    (300 * interval.millis()).max(7 * DAY_MS)
}

/// Données nécessaires, déjà chargées (cache disque et API).
#[derive(Debug, Clone)]
pub struct FiboData {
    /// Trajectoire 1 minute (perpétuel, comptant avant sa première bougie).
    pub bars: Vec<Bar>,
    /// Prix de marque 1 minute, s'il existe.
    pub mark: Option<Vec<Bar>>,
    pub funding: Vec<FundingRate>,
    pub spec: ContractSpec,
    pub warnings: Vec<String>,
}

/// Résultats d'une période.
#[derive(Debug, Clone, Serialize)]
pub struct PeriodReport {
    pub label: String,
    pub start: String,
    pub end: String,
    /// Avec le levier choisi.
    pub summary: RunSummary,
    pub stats: SimStats,
    /// Le même moteur à x1, en R seulement : la taille change, pas l'avantage.
    pub x1: RunSummary,
    pub per_level: Vec<LevelDetail>,
    /// Niveaux fixes comparables (0,45 ; 0,55 ; 0,70) : résultat et détail par niveau.
    pub fixed_levels: Vec<f64>,
    pub fixed_summary: RunSummary,
    pub fixed_per_level: Vec<LevelDetail>,
    pub placebo: PlaceboResult,
    pub monte_carlo: Option<MonteCarlo>,
    pub budget_curve: Vec<(i64, f64)>,
    pub ruin_time: Option<String>,
    pub trades: Vec<Trade>,
    pub budget_chart: Chart,
    pub r_chart: Chart,
    pub placebo_chart: Chart,
    pub monte_carlo_chart: Option<Chart>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FiboReport {
    pub symbol: String,
    pub contract: ContractSpec,
    pub warnings: Vec<String>,
    pub study: PeriodReport,
    pub test: PeriodReport,
    /// Verdict, lu sur la période de test.
    pub verdict: VerdictInfo,
    pub price_chart: Chart,
}

fn slice(bars: &[Bar], from: i64, to: i64) -> &[Bar] {
    let a = bars.partition_point(|b| b.time < from);
    let b = bars.partition_point(|b| b.time < to);
    &bars[a..b]
}

/// Avertissements propres aux hypothèses du moteur.
fn assumption_warnings(req: &FiboRequest, data: &FiboData) -> Vec<String> {
    let mut w = data.warnings.clone();
    w.extend(data.spec.warnings.iter().cloned());
    w.push(format!(
        "Frais : maker {} %, taker {} %. Ce sont des valeurs par défaut modifiables, **à vérifier** : la grille des perpétuels dépend du compte et n'est pas publique par API.",
        req.costs.fees.maker_pct, req.costs.fees.taker_pct
    ));
    w.push("Liquidation en marge isolée : formule générique entrée × (1 − 1/L + taux de maintenance), toute la marge restante est perdue. La formule exacte de Poloniex n'a pas pu être vérifiée.".into());
    if !data.spec.tiers_from_risk_limit {
        w.push(
            "Un seul taux de marge de maintenance est utilisé (celui de la fiche du contrat)."
                .into(),
        );
    }
    match (&data.mark, req.use_mark_price) {
        (Some(_), true) => {}
        (None, true) => w.push(format!(
            "Pas d'historique du prix de marque en cache : les liquidations se lisent sur le dernier prix, avec une marge de sécurité de {} %.",
            req.costs.liquidation_safety_pct
        )),
        (_, false) => w.push(format!(
            "Prix de marque non utilisé : les liquidations se lisent sur le dernier prix, avec une marge de sécurité de {} %.",
            req.costs.liquidation_safety_pct
        )),
    }
    match data.funding.first() {
        None => w.push(format!(
            "Pas d'historique du funding : taux supposé de {} % par 8 h sur toute la période.",
            req.costs.funding_pct_8h
        )),
        Some(f) if f.time > date_ms(req.study_start) => w.push(format!(
            "Le funding réel n'est disponible que depuis le {} (180 derniers jours) : avant, taux supposé de {} % par 8 h.",
            fmt_time(f.time, Interval::D1),
            req.costs.funding_pct_8h
        )),
        Some(_) => {}
    }
    w.push("Dans la bougie d'entrée, les extrêmes adverses comptent et l'objectif n'est pas évalué ; si le stop (ou la liquidation) et l'objectif sont touchés dans la même bougie 1 minute, la sortie défavorable est retenue (nombre de cas affiché).".into());
    w
}

struct PeriodInputs<'a> {
    label: &'a str,
    start: NaiveDate,
    end: NaiveDate,
    /// Le placebo et le Monte-Carlo ne sont recalculés qu'à la demande de l'appelant.
    progress_label: &'a str,
}

fn run_period(
    req: &FiboRequest,
    data: &FiboData,
    inputs: PeriodInputs,
    on_progress: &(dyn Fn(&str, usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<PeriodReport, String> {
    let params = req.sim_params(inputs.start, inputs.end);
    let from = params.start_ms - warmup_ms(req.fibo.interval);
    let bars = slice(&data.bars, from, params.end_ms);
    if slice(bars, params.start_ms, params.end_ms).is_empty() {
        return Err(format!(
            "Aucune bougie 1 minute pour la période {} ({} → {}) : lancez le téléchargement.",
            inputs.label, inputs.start, inputs.end
        ));
    }
    let mark = data
        .mark
        .as_deref()
        .filter(|_| req.use_mark_price)
        .map(|m| slice(m, from, params.end_ms));
    let run = |p: &SimParams| -> SimResult { simulate(bars, mark, &data.funding, &data.spec, p) };

    on_progress(&format!("Simulation — {}", inputs.label), 0, 1);
    let fib = run(&params);
    let summary = summarize(&fib);
    // Même moteur à x1 : quantité fractionnaire, ni arrondi aux contrats ni liquidation.
    let mut x1_params = params.clone();
    x1_params.account.r_only = true;
    x1_params.account.leverage = 1.0;
    let x1 = summarize(&run(&x1_params));
    // Niveaux fixes comparables, simulés ensemble (détail par niveau).
    let mut fixed_params = params.clone();
    fixed_params.fibo.ratios = req.placebo.fixed_levels.clone();
    let fixed = if fixed_params.fibo.ratios.is_empty() {
        None
    } else {
        Some(run(&fixed_params))
    };

    let label = format!("Niveaux placebo — {}", inputs.progress_label);
    let progress = |k: usize, n: usize| on_progress(&label, k, n);
    let placebo = run_placebo(
        &SimInput {
            bars,
            mark,
            funding: &data.funding,
            spec: &data.spec,
        },
        &params,
        &req.placebo,
        summary.expectancy_r_net,
        &progress,
        cancel,
    );
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Calcul annulé.".into());
    }
    on_progress(&format!("Monte-Carlo — {}", inputs.label), 0, 1);
    let monte_carlo = crate::validation::monte_carlo(
        &fib.trades,
        req.account.initial_budget,
        req.mc_draws,
        req.placebo.seed ^ 0x4D43,
    );
    let title = |what: &str| format!("{what} — période {}", inputs.label.to_lowercase());
    Ok(PeriodReport {
        label: inputs.label.to_string(),
        start: inputs.start.to_string(),
        end: inputs.end.to_string(),
        stats: fib.stats.clone(),
        x1,
        per_level: per_level(&fib.trades),
        fixed_levels: req.placebo.fixed_levels.clone(),
        fixed_summary: fixed.as_ref().map(summarize).unwrap_or_default(),
        fixed_per_level: fixed
            .as_ref()
            .map(|f| per_level(&f.trades))
            .unwrap_or_default(),
        budget_chart: budget_chart(
            &title("Budget"),
            &fib.budget_curve,
            req.account.initial_budget,
        ),
        r_chart: r_histogram(&title("Résultats en R"), &fib.trades),
        placebo_chart: placebo_chart(&title("Niveaux placebo"), &placebo),
        monte_carlo_chart: monte_carlo.as_ref().map(|m| {
            monte_carlo_chart(
                &title("Monte-Carlo : budget final"),
                &m.histogram,
                req.account.initial_budget,
            )
        }),
        ruin_time: fib.ruin_time.map(|t| fmt_time(t, Interval::M1)),
        budget_curve: fib.budget_curve.clone(),
        trades: fib.trades,
        summary,
        placebo,
        monte_carlo,
    })
}

/// Construit le rapport complet.
pub fn build_report(
    req: &FiboRequest,
    data: &FiboData,
    on_progress: &(dyn Fn(&str, usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<FiboReport, String> {
    req.validate(&data.spec)?;
    let study = run_period(
        req,
        data,
        PeriodInputs {
            label: "Étude",
            start: req.study_start,
            end: req.study_end,
            progress_label: "étude",
        },
        on_progress,
        cancel,
    )?;
    let test = run_period(
        req,
        data,
        PeriodInputs {
            label: "Test",
            start: req.test_start,
            end: req.test_end,
            progress_label: "test",
        },
        on_progress,
        cancel,
    )?;
    let verdict = verdict(&test.summary, test.placebo.percentile);

    // Prix : bougies de détection de la fin de la période de test, avec les pivots.
    let from = date_ms(req.test_start) - warmup_ms(req.fibo.interval);
    let det = aggregate(
        slice(&data.bars, from, req.data_end_ms()),
        Interval::M1,
        req.fibo.interval,
    );
    let analysis = analyze(&det, &req.fibo);
    let shown = match req.fibo.interval {
        Interval::M15 => 672,
        Interval::H1 => 720,
        _ => 540,
    }
    .min(det.len());
    let offset = det.len() - shown;
    let price = price_chart(
        &det[offset..],
        offset,
        req.fibo.interval,
        &analysis,
        &test.trades,
        &format!(
            "{} — {} derniers jours de la période de test : pivots, niveaux et trades",
            data.spec.symbol,
            shown as i64 * req.fibo.interval.millis() / DAY_MS
        ),
    );
    Ok(FiboReport {
        symbol: data.spec.symbol.clone(),
        contract: data.spec.clone(),
        warnings: assumption_warnings(req, data),
        study,
        test,
        verdict,
        price_chart: price,
    })
}

impl FiboReport {
    /// Tous les trades des deux périodes, au format CSV.
    pub fn trades_csv(&self) -> String {
        let mut out = String::from(
            "Periode,Id,Mise_en_place,Niveau,Sens,Entree,Prix_entree,Sortie,Prix_sortie,Motif,Levier,Marge,Contrats,Montant,Stop,Objectif,Liquidation,Brut,Frais_entree,Frais_sortie,Funding,Net,Risque_R,R_brut,R_net,Ambigue\n",
        );
        for period in [&self.study, &self.test] {
            for t in &period.trades {
                let reason = serde_json::to_value(t.reason)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{},{},{},{},{},{},{},{},{},{},{},{:.4},{},{:.2},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{}",
                    period.label,
                    t.id + 1,
                    t.setup_id + 1,
                    t.ratio,
                    t.side.label(),
                    fmt_time(t.entry_time, Interval::M1),
                    t.entry_price,
                    fmt_time(t.exit_time, Interval::M1),
                    t.exit_price,
                    reason,
                    t.leverage,
                    t.margin,
                    t.contracts,
                    t.notional,
                    t.stop,
                    t.target,
                    t.liquidation.map(|l| l.to_string()).unwrap_or_default(),
                    t.gross,
                    t.fee_entry,
                    t.fee_exit,
                    t.funding,
                    t.net,
                    t.risk,
                    t.r_gross,
                    t.r_net,
                    u8::from(t.ambiguous),
                );
            }
        }
        out
    }
}

/// Panneau « Points actuels » et graphique, d'après les dernières bougies de détection.
#[derive(Debug, Clone, Serialize)]
pub struct PointsReport {
    pub symbol: String,
    pub interval: String,
    pub current: Option<CurrentPoints>,
    /// ATR(14) de l'unité de détection et seuil de retournement correspondant.
    pub atr: Option<f64>,
    pub threshold: Option<f64>,
    pub chart: Chart,
}

/// Points actuels à partir de bougies de détection closes `bars` (récentes).
pub fn points_report(
    bars: &[Bar],
    params: &FiboParams,
    symbol: &str,
    tick: f64,
) -> Result<PointsReport, String> {
    params.validate()?;
    if bars.len() < 30 {
        return Err("Pas assez de bougies pour identifier des points.".into());
    }
    let mut detector = crate::fibo::Detector::new(params.clone());
    for b in bars {
        detector.push(*b);
    }
    let analysis = analyze(bars, params);
    let shown = match params.interval {
        Interval::M15 => 480,
        Interval::H1 => 480,
        _ => 360,
    }
    .min(bars.len());
    let offset = bars.len() - shown;
    let threshold = match params.threshold_mode {
        crate::fibo::ThresholdMode::Atr => detector.atr().map(|a| a * params.atr_mult),
        crate::fibo::ThresholdMode::Percent => {
            bars.last().map(|b| b.close * params.threshold_pct / 100.0)
        }
    };
    Ok(PointsReport {
        symbol: symbol.to_string(),
        interval: params.interval.label().to_string(),
        current: current_points(bars, params, tick),
        atr: detector.atr(),
        threshold,
        chart: price_chart(
            &bars[offset..],
            offset,
            params.interval,
            &analysis,
            &[],
            &format!("{symbol} — points actuels ({})", params.interval.label()),
        ),
    })
}

/// Charge ce que le rapport demande : fiche du contrat (API, sinon cache),
/// bougies 1 minute du cache disque (perpétuel puis comptant), funding, prix de
/// marque. Ne télécharge pas les bougies : voir [`crate::fibo_data::ensure_data`].
pub async fn load_data(
    feed: &MarketFeed,
    store: &CandleStore,
    req: &FiboRequest,
) -> Result<FiboData, String> {
    let spec = feed.contract(&req.base, store).await?;
    req.validate(&spec)?;
    let perp = contract::perp_instrument(&req.base)?;
    let spot = Instrument::new(&req.base, Market::Spot)?;
    let (from, to) = (req.data_start_ms(), req.data_end_ms());
    let perp_bars = store.read(
        &SeriesKey::new(&perp.symbol(), Interval::M1, false),
        from,
        to,
    )?;
    let spot_bars = store.read(
        &SeriesKey::new(&spot.symbol(), Interval::M1, false),
        from,
        to,
    )?;
    let trajectory = crate::fibo_data::stitch(perp_bars, spot_bars, from, to)?;
    let mut warnings = trajectory.warnings;
    let funding = match feed.funding(&perp).await {
        Ok(f) => f.as_ref().clone(),
        Err(e) => {
            warnings.push(format!("Historique du funding indisponible ({e})."));
            Vec::new()
        }
    };
    let mark = if req.use_mark_price {
        let m = store.read(
            &SeriesKey::new(&perp.symbol(), Interval::M1, true),
            from,
            to,
        )?;
        (!m.is_empty()).then_some(m)
    } else {
        None
    };
    Ok(FiboData {
        bars: trajectory.bars,
        mark,
        funding,
        spec,
        warnings,
    })
}

/// Zoom sur un trade : bougies 1 minute autour de l'entrée et de la sortie.
pub fn trade_zoom(store: &CandleStore, base: &str, trade: &Trade) -> Result<Chart, String> {
    let perp = contract::perp_instrument(base)?;
    let spot = Instrument::new(base, Market::Spot)?;
    let (from, to) = (
        trade.entry_time - 90 * 60_000,
        (trade.exit_time + 90 * 60_000).min(trade.entry_time + 20 * 3_600_000),
    );
    let mut bars = store.read(
        &SeriesKey::new(&perp.symbol(), Interval::M1, false),
        from,
        to,
    )?;
    if bars.is_empty() {
        bars = store.read(
            &SeriesKey::new(&spot.symbol(), Interval::M1, false),
            from,
            to,
        )?;
    }
    if bars.is_empty() {
        return Err("Bougies 1 minute de ce trade absentes du cache.".into());
    }
    Ok(trade_zoom_chart(trade, &bars))
}

/// Points actuels, d'après les bougies de détection récentes de Poloniex
/// (le perpétuel ; pas besoin du cache 1 minute).
pub async fn load_points(
    feed: &MarketFeed,
    store: &CandleStore,
    base: &str,
    params: &FiboParams,
    prudent_tick: bool,
    today: NaiveDate,
) -> Result<PointsReport, String> {
    params.validate()?;
    let perp = contract::perp_instrument(base)?;
    let from = today
        - chrono::Duration::days(match params.interval {
            Interval::M15 => 10,
            Interval::H1 => 30,
            _ => 120,
        });
    let bars = feed.bars(&perp, params.interval, from).await?;
    // Poloniex sert l'historique depuis le 1er janvier de l'année : on garde les plus récentes.
    let keep = 1500.min(bars.len());
    let recent = &bars[bars.len() - keep..];
    let tick = if prudent_tick {
        feed.contract(base, store)
            .await
            .map_or(0.1, |s| s.tick_size)
    } else {
        0.0
    };
    points_report(recent, params, &perp.symbol(), tick)
}

/// Contrôle des repères du point 7.2 sur les bougies journalières BTC_USDT du
/// 01/05/2026 au 12/10/2026 (165 jours).
#[derive(Debug, Clone, Serialize)]
pub struct ExcursionReport {
    pub days: usize,
    pub first: String,
    pub last: String,
    pub rows: Vec<ExcursionCheck>,
}

pub async fn load_excursions(feed: &MarketFeed) -> Result<ExcursionReport, String> {
    let inst = Instrument::new("BTC", Market::Spot)?;
    let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap_or_default();
    let to = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap_or_default();
    let bars = feed.bars(&inst, Interval::D1, from).await?;
    let days: Vec<Bar> = bars
        .iter()
        .filter(|b| b.date() >= from && b.date() <= to)
        .copied()
        .collect();
    if days.is_empty() {
        return Err("Aucune bougie journalière BTC_USDT dans la période de contrôle.".into());
    }
    Ok(ExcursionReport {
        days: days.len(),
        first: days[0].date().to_string(),
        last: days[days.len() - 1].date().to_string(),
        rows: check_excursions(&days),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::MaintenanceTier;
    use crate::fibo::ThresholdMode;

    const T0: i64 = 20_000 * DAY_MS;

    fn spec() -> ContractSpec {
        ContractSpec {
            symbol: "BTC_USDT_PERP".into(),
            max_leverage: 75.0,
            contract_value: 0.001,
            lot_size: 1.0,
            min_size: 1.0,
            tick_size: 0.1,
            tiers: vec![MaintenanceTier {
                up_to_contracts: None,
                rate: 0.005,
            }],
            tiers_from_risk_limit: false,
            warnings: vec![],
        }
    }

    /// Marche aléatoire 1 min de `days` jours à partir de T0 (jour 20 000), prix de l'ordre de 60 000.
    pub(crate) fn walk(days: usize, seed: u64) -> Vec<Bar> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as f64 / u64::MAX as f64 - 0.5
        };
        let mut price: f64 = 60_000.0;
        (0..days * 1440)
            .map(|i| {
                let open = price;
                let close = open * (1.0 + 0.0003 * (next() + next() + next() + next()) * 1.732);
                price = close;
                Bar {
                    time: T0 + i as i64 * 60_000,
                    open,
                    high: open.max(close) * (1.0 + 0.0001 * (next() + 0.5)),
                    low: open.min(close) * (1.0 - 0.0001 * (next() + 0.5)),
                    close,
                    volume: 0.0,
                }
            })
            .collect()
    }

    fn day(n: i64) -> NaiveDate {
        chrono::DateTime::from_timestamp_millis(T0 + n * DAY_MS)
            .unwrap()
            .date_naive()
    }

    fn request() -> FiboRequest {
        FiboRequest {
            base: "BTC".into(),
            fibo: FiboParams {
                threshold_mode: ThresholdMode::Atr,
                ..FiboParams::default()
            },
            rules: TradeRules::default(),
            costs: Costs::default(),
            account: Account {
                initial_budget: 500.0,
                margin: 10.0,
                leverage: 10.0,
                r_only: false,
            },
            study_start: day(10),
            study_end: day(79),
            test_start: day(80),
            test_end: day(139),
            placebo: PlaceboParams {
                draws: 6,
                ..PlaceboParams::default()
            },
            mc_draws: 500,
            use_mark_price: false,
        }
    }

    /// Les commandes Tauri exigent des futures `Send` : vérifié à la compilation.
    #[allow(dead_code)]
    fn futures_are_send(
        feed: &MarketFeed,
        store: &CandleStore,
        req: &FiboRequest,
        cancel: &AtomicBool,
    ) {
        fn is_send<T: Send>(_: T) {}
        is_send(load_data(feed, store, req));
        is_send(load_points(
            feed,
            store,
            "BTC",
            &req.fibo,
            false,
            req.test_end,
        ));
        is_send(load_excursions(feed));
        is_send(feed.contract("BTC", store));
        let want = crate::fibo_data::DataRange {
            base: "BTC",
            from: 0,
            to: 1,
            use_mark: false,
        };
        is_send(crate::fibo_data::ensure_data(
            feed,
            store,
            want,
            &|_| {},
            cancel,
        ));
    }

    /// La demande envoyée par l'interface (valeurs par défaut de la page) se décode.
    #[test]
    fn the_ui_request_deserializes() {
        let json = r#"{"base":"BTC","fibo":{"interval":"HOUR_1","threshold_mode":"atr","atr_mult":3,"threshold_pct":1,"ratios":[0.382,0.5,0.618,0.786],"direction":"continuation","max_age_bars":48},"rules":{"stop":{"kind":"next_level","margin_pct":0},"target":{"kind":"return_to_b"},"prudent_tick":false,"liquidation_as_stop":false,"max_hold_bars":0},"costs":{"fees":{"maker_pct":0.02,"taker_pct":0.05},"slippage_pct":0.01,"funding_pct_8h":0.01,"liquidation_safety_pct":0.1},"account":{"initial_budget":500,"margin":10,"leverage":20},"study_start":"2023-01-01","study_end":"2024-12-31","test_start":"2025-01-01","test_end":"2026-10-13","placebo":{"draws":200,"seed":20260101,"low":0.3,"high":0.8,"fixed_levels":[0.45,0.55,0.7]},"mc_draws":10000,"use_mark_price":false}"#;
        let r: FiboRequest = serde_json::from_str(json).unwrap();
        assert_eq!(r.fibo.interval, Interval::H1);
        assert_eq!(r.fibo.max_age_bars, 48);
        assert_eq!(r.account.leverage, 20.0);
        assert!(!r.account.r_only, "champ facultatif");
        assert!(r.validate(&spec()).is_ok());
        // Autres règles de l'interface.
        for (stop, target) in [
            (
                r#"{"kind":"origin","margin_pct":1.5}"#,
                r#"{"kind":"extension","ratio":1.272}"#,
            ),
            (
                r#"{"kind":"fixed","pct":2}"#,
                r#"{"kind":"r_multiple","r":2}"#,
            ),
        ] {
            let alt = json
                .replace(r#"{"kind":"next_level","margin_pct":0}"#, stop)
                .replace(r#"{"kind":"return_to_b"}"#, target);
            let r: FiboRequest = serde_json::from_str(&alt).unwrap();
            assert!(r.validate(&spec()).is_ok(), "{stop} / {target}");
        }
        // Intervalles et sens de l'interface.
        let m15 = json
            .replace("HOUR_1", "MINUTE_15")
            .replace("continuation", "contrary")
            .replace(r#""threshold_mode":"atr""#, r#""threshold_mode":"percent""#);
        let r: FiboRequest = serde_json::from_str(&m15).unwrap();
        assert_eq!(r.fibo.interval, Interval::M15);
        assert_eq!(r.fibo.direction, crate::fibo::Direction::Contrary);
    }

    #[test]
    fn periods_must_not_overlap() {
        let mut r = request();
        assert!(r.validate(&spec()).is_ok());
        r.test_start = r.study_end;
        assert!(r
            .validate(&spec())
            .unwrap_err()
            .contains("hors échantillon"));
        let mut r = request();
        r.account.leverage = 100.0;
        assert!(r.validate(&spec()).unwrap_err().contains("maximum"));
        let mut r = request();
        r.study_end = day(5);
        assert!(r.validate(&spec()).is_err());
    }

    #[test]
    fn full_report_on_a_synthetic_market() {
        let data = FiboData {
            bars: walk(140, 4242),
            mark: None,
            funding: vec![],
            spec: spec(),
            warnings: vec!["avertissement de données".into()],
        };
        let req = request();
        let seen = std::sync::Mutex::new(Vec::new());
        let on =
            |label: &str, k: usize, n: usize| seen.lock().unwrap().push((label.to_string(), k, n));
        let report = build_report(&req, &data, &on, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.symbol, "BTC_USDT_PERP");
        assert_eq!(report.study.start, day(10).to_string());
        for p in [&report.study, &report.test] {
            assert!(p.summary.trades > 0, "{} : aucun trade", p.label);
            assert_eq!(p.placebo.draws.len(), 6);
            assert!(p.placebo.percentile.is_some());
            assert!(p.monte_carlo.is_some());
            assert_eq!(p.fixed_levels, vec![0.45, 0.55, 0.70]);
            assert!(
                p.x1.trades >= p.summary.trades,
                "x1 : plus de trades (aucun refus)"
            );
            // Les trades restent dans leur période.
            let (a, b) = (
                date_ms(NaiveDate::parse_from_str(&p.start, "%Y-%m-%d").unwrap()),
                date_ms(NaiveDate::parse_from_str(&p.end, "%Y-%m-%d").unwrap()) + DAY_MS,
            );
            assert!(p
                .trades
                .iter()
                .all(|t| t.entry_time >= a && t.entry_time < b));
            assert!(
                p.budget_chart.data.len() == 2 && p.placebo_chart.data[0]["type"] == "histogram"
            );
        }
        // Le verdict se lit sur la période de test et ne conclut pas à un avantage sur du bruit.
        assert_ne!(
            report.verdict.verdict,
            crate::validation::Verdict::Demonstrated
        );
        assert_eq!(report.verdict.criteria.len(), 3);
        // Avertissements : données, frais « à vérifier », liquidation, prix de marque, funding.
        let joined = report.warnings.join("\n");
        for needle in [
            "avertissement de données",
            "à vérifier",
            "Liquidation en marge isolée",
            "Prix de marque non utilisé",
            "Pas d'historique du funding",
        ] {
            assert!(joined.contains(needle), "manque : {needle}");
        }
        // Progression : simulation, placebo, Monte-Carlo, pour les deux périodes.
        let labels: Vec<String> = seen.lock().unwrap().iter().map(|s| s.0.clone()).collect();
        assert!(
            labels.iter().any(|l| l.contains("placebo — étude"))
                && labels.iter().any(|l| l.contains("Monte-Carlo — Test"))
        );
        // Graphique de prix et export.
        assert_eq!(report.price_chart.data[0]["type"], "candlestick");
        let csv = report.trades_csv();
        assert_eq!(
            csv.lines().count(),
            1 + report.study.trades.len() + report.test.trades.len()
        );
        assert!(csv.lines().next().unwrap().starts_with("Periode,Id,"));
        // Déterminisme : mêmes données, mêmes réglages, même rapport.
        let again = build_report(&req, &data, &|_, _, _| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            serde_json::to_string(&report.test.placebo.draws).unwrap(),
            serde_json::to_string(&again.test.placebo.draws).unwrap()
        );
        assert_eq!(
            report.test.summary.final_budget,
            again.test.summary.final_budget
        );
    }

    #[test]
    fn a_period_without_data_is_reported() {
        let data = FiboData {
            bars: walk(30, 1),
            mark: None,
            funding: vec![],
            spec: spec(),
            warnings: vec![],
        };
        let err =
            build_report(&request(), &data, &|_, _, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(err.contains("Aucune bougie 1 minute"), "{err}");
    }

    #[test]
    fn points_report_describes_the_latest_move() {
        let minutes = walk(40, 9);
        let det = aggregate(&minutes, Interval::M1, Interval::H1);
        let params = FiboParams {
            threshold_mode: ThresholdMode::Percent,
            threshold_pct: 1.0,
            ..FiboParams::default()
        };
        let r = points_report(&det, &params, "BTC_USDT_PERP", 0.1).unwrap();
        let cur = r.current.expect("un mouvement confirmé");
        assert!(cur.confirmed_time > cur.b_time && cur.b_time > cur.a_time);
        assert!(!cur.levels.is_empty() || !cur.ignored.is_empty());
        assert!(r.threshold.unwrap() > 0.0 && r.atr.unwrap() > 0.0);
        assert_eq!(r.chart.data[0]["type"], "candlestick");
        assert!(points_report(&det[..10], &params, "X", 0.1).is_err());
    }
}

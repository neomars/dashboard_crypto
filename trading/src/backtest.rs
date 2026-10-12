//! Backtest barre par barre du module tendance, avec les règles de risque du
//! moteur.
//!
//! Chronologie d'une bougie :
//! 1. **ouverture** : si le cours d'ouverture franchit le stop suiveur, la
//!    position est soldée à ce cours ; puis l'ordre décidé à la clôture
//!    précédente est exécuté au cours d'ouverture (glissement défavorable,
//!    frais taker) ;
//! 2. **pendant la bougie** : funding (perpétuel), stop suiveur touché par le
//!    plus bas (ou le plus haut pour une vente), contrôle de liquidation au
//!    pire cours de la bougie ;
//! 3. **clôture** : valorisation, coupe-circuits, mise à jour du stop, puis
//!    nouveau signal et ordre pour l'ouverture suivante.
//!
//! Aucune décision n'utilise une donnée postérieure à la clôture qui la
//! déclenche : pas de biais d'anticipation.

use crate::feed::MarketFeed;
use crate::market::{
    date_ms, fmt_time, Bar, FundingRate, Instrument, Interval, Market, FUNDING_PERIOD_MS,
};
use crate::metrics::{performance, Performance};
use crate::risk::{
    needs_rebalance, target_exposure, trailing_stop, ATR_LOOKBACK, DAILY_LOSS_LIMIT,
    HARD_MAX_LEVERAGE, MAINTENANCE_MARGIN, MAX_DRAWDOWN_LIMIT, RECOMMENDED_MAX_LEVERAGE,
    VOL_WINDOW_DAYS,
};
use crate::strategy::{trend_signal, TrendVariant};
use crate::ta::{atr, realized_vol};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Paramètres d'un backtest, tels que saisis dans l'interface (pourcentages en %).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestParams {
    /// Actif coté contre USDT (BTC, ETH...).
    pub base: String,
    pub market: Market,
    pub interval: Interval,
    pub start: NaiveDate,
    /// Exclue : le backtest s'arrête à la dernière bougie ouverte avant cette date.
    pub end: NaiveDate,
    /// Capital initial, en USDT.
    pub initial_capital: f64,
    pub variant: TrendVariant,
    /// Volatilité annualisée visée pour la position (%).
    pub target_vol_pct: f64,
    /// Levier maximal (ramené à 1 sur le spot).
    pub max_leverage: f64,
    /// Distance du stop suiveur en ATR (0 = pas de stop).
    pub stop_atr_mult: f64,
    /// Frais par ordre (taker), en % du montant.
    pub fee_pct: f64,
    /// Glissement par ordre, en % du cours.
    pub slippage_pct: f64,
    /// Écart relatif d'exposition en dessous duquel on ne rééquilibre pas (%).
    pub rebalance_pct: f64,
    /// Funding supposé (% par 8 h) quand Poloniex ne fournit pas l'historique (plus de 180 jours).
    pub funding_pct_8h: f64,
    /// Si vrai, un coupe-circuit solde la position et arrête le backtest (reprise manuelle en réel).
    pub halt_on_breach: bool,
}

impl BacktestParams {
    pub fn validate(&self) -> Result<(), String> {
        self.instrument()?;
        let check = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        check(
            self.start < self.end,
            "La date de début doit être antérieure à la date de fin.",
        )?;
        check(
            self.initial_capital.is_finite() && self.initial_capital > 0.0,
            "Le capital initial doit être positif.",
        )?;
        check(
            (1.0..=100.0).contains(&self.target_vol_pct),
            "La volatilité cible doit être comprise entre 1 et 100 %.",
        )?;
        check(
            (0.1..=HARD_MAX_LEVERAGE).contains(&self.max_leverage),
            "Le levier maximal doit être compris entre 0,1 et 5.",
        )?;
        check(
            (0.0..=10.0).contains(&self.stop_atr_mult),
            "Le stop doit être compris entre 0 (désactivé) et 10 ATR.",
        )?;
        check(
            (0.0..=1.0).contains(&self.fee_pct),
            "Les frais doivent être compris entre 0 et 1 %.",
        )?;
        check(
            (0.0..=1.0).contains(&self.slippage_pct),
            "Le glissement doit être compris entre 0 et 1 %.",
        )?;
        check(
            (0.0..=100.0).contains(&self.rebalance_pct),
            "Le seuil de rééquilibrage doit être compris entre 0 et 100 %.",
        )?;
        check(
            (-1.0..=1.0).contains(&self.funding_pct_8h),
            "Le funding supposé doit être compris entre −1 et 1 % par 8 h.",
        )
    }

    pub fn instrument(&self) -> Result<Instrument, String> {
        Instrument::new(&self.base, self.market)
    }

    /// Levier effectivement autorisé : au plus 1 sur le spot (pas d'emprunt).
    pub fn leverage_cap(&self) -> f64 {
        match self.market {
            Market::Spot => self.max_leverage.min(1.0),
            Market::Perp => self.max_leverage,
        }
    }

    /// Bougies nécessaires avant la première décision (signal, volatilité, ATR).
    pub fn warmup_bars(&self) -> usize {
        self.variant
            .warmup_bars()
            .max(self.interval.bars_for_days(VOL_WINDOW_DAYS) + 1)
            .max(ATR_LOOKBACK + 1)
    }

    /// Date à partir de laquelle charger les bougies pour préchauffer les indicateurs.
    pub fn data_start(&self) -> NaiveDate {
        let days = (self.warmup_bars() as f64 / self.interval.bars_per_day()).ceil() as i64;
        self.start - Duration::days(days + 7)
    }
}

/// Origine d'un ordre exécuté.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FillKind {
    #[serde(rename = "Signal")]
    Rebalance,
    #[serde(rename = "Stop suiveur")]
    Stop,
    #[serde(rename = "Coupe-circuit")]
    CircuitBreaker,
    #[serde(rename = "Liquidation")]
    Liquidation,
}

/// Ordre exécuté pendant le backtest.
#[derive(Debug, Clone, Serialize)]
pub struct Fill {
    pub time: i64,
    pub date: String,
    pub kind: FillKind,
    /// « Achat » ou « Vente ».
    pub side: &'static str,
    /// Cours d'exécution, glissement inclus.
    pub price: f64,
    /// Quantité d'actif échangée (positive).
    pub quantity: f64,
    /// Montant échangé, en USDT.
    pub notional: f64,
    pub fee: f64,
    /// Exposition après l'ordre (fraction signée du capital).
    pub exposure_after: f64,
    pub detail: String,
}

/// Événement de risque : stop, coupe-circuit, liquidation.
#[derive(Debug, Clone, Serialize)]
pub struct RiskEvent {
    pub time: i64,
    pub date: String,
    pub kind: String,
    pub detail: String,
}

/// État à la clôture de chaque bougie du backtest.
#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub time: i64,
    pub close: f64,
    pub equity: f64,
    /// Achat au premier cours d'ouverture puis conservation (même actif, sans levier).
    pub buy_hold: f64,
    /// Baisse depuis le plus haut du capital (%, négative).
    pub drawdown_pct: f64,
    /// Exposition à la clôture : valeur de la position / capital (négative si vendeuse).
    pub exposure: f64,
    /// Score de tendance à la clôture (−1 à +1).
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Backtest {
    pub params: BacktestParams,
    pub symbol: String,
    pub points: Vec<EquityPoint>,
    pub fills: Vec<Fill>,
    pub events: Vec<RiskEvent>,
    /// Résultat net de chaque aller-retour (ouverture → retour à plat ou changement de sens), en USDT.
    pub round_trips: Vec<f64>,
    pub fees: f64,
    /// Funding payé (positif) ou reçu (négatif), en USDT.
    pub funding: f64,
    pub funding_real_periods: usize,
    pub funding_estimated_periods: usize,
    pub traded_notional: f64,
    pub liquidated: bool,
    pub halted: bool,
    /// Avertissements à afficher (début décalé, levier...).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub strategy: Performance,
    pub buy_hold: Performance,
    pub fills: usize,
    pub round_trips: usize,
    pub win_rate_pct: Option<f64>,
    /// Part des bougies avec une position ouverte (%).
    pub exposure_pct: f64,
    /// Levier moyen quand une position est ouverte.
    pub avg_leverage: f64,
    pub fees: f64,
    pub funding: f64,
    pub funding_real_periods: usize,
    pub funding_estimated_periods: usize,
    /// Montant échangé par an, rapporté au capital moyen.
    pub turnover: f64,
    pub stops: usize,
    pub circuit_breakers: usize,
    pub liquidated: bool,
    pub halted: bool,
    pub first_date: String,
    pub last_date: String,
}

/// État du portefeuille simulé.
struct Sim {
    interval: Interval,
    market: Market,
    fee: f64,
    slip: f64,
    stop_mult: f64,
    cash: f64,
    /// Quantité d'actif détenue (négative si vendeuse).
    units: f64,
    best: f64,
    stop: Option<f64>,
    blocked_long: bool,
    blocked_short: bool,
    /// Après un stop : clôture à dépasser (meilleur cours avant le stop) pour revenir dans ce sens.
    reentry_long: f64,
    reentry_short: f64,
    halted: bool,
    liquidated: bool,
    fills: Vec<Fill>,
    events: Vec<RiskEvent>,
    fees: f64,
    funding: f64,
    funding_real: usize,
    funding_estimated: usize,
    traded_notional: f64,
    trip_start: Option<f64>,
    round_trips: Vec<f64>,
}

impl Sim {
    fn equity(&self, price: f64) -> f64 {
        self.cash + self.units * price
    }

    fn exposure(&self, price: f64) -> f64 {
        let equity = self.equity(price);
        if equity > 0.0 {
            self.units * price / equity
        } else {
            0.0
        }
    }

    fn event(&mut self, time: i64, kind: &str, detail: String) {
        self.events.push(RiskEvent {
            time,
            date: fmt_time(time, self.interval),
            kind: kind.into(),
            detail,
        });
    }

    /// Échange `delta` unités au cours de référence `price` : glissement
    /// défavorable, frais taker sur le montant exécuté.
    fn execute(&mut self, time: i64, delta: f64, price: f64, kind: FillKind, detail: String) {
        if delta == 0.0 || !delta.is_finite() {
            return;
        }
        let before = self.units;
        let equity_before = self.equity(price);
        let fill = price * (1.0 + self.slip * delta.signum());
        let fee = delta.abs() * fill * self.fee;
        self.cash -= delta * fill + fee;
        self.units += delta;
        if self.units.abs() < 1e-12 * delta.abs().max(1.0) {
            self.units = 0.0;
        }
        self.fees += fee;
        self.traded_notional += delta.abs() * fill;

        let closed = before != 0.0 && (self.units == 0.0 || self.units.signum() != before.signum());
        if closed {
            if let Some(start) = self.trip_start.take() {
                self.round_trips.push(self.equity(price) - start);
            }
        }
        if self.units != 0.0 && (before == 0.0 || closed) {
            self.trip_start = Some(if closed {
                self.equity(price)
            } else {
                equity_before
            });
        }
        self.fills.push(Fill {
            time,
            date: fmt_time(time, self.interval),
            kind,
            side: if delta > 0.0 { "Achat" } else { "Vente" },
            price: fill,
            quantity: delta.abs(),
            notional: delta.abs() * fill,
            fee,
            exposure_after: self.exposure(price),
            detail,
        });
    }

    fn close_all(&mut self, time: i64, price: f64, kind: FillKind, detail: String) {
        let delta = -self.units;
        self.execute(time, delta, price, kind, detail);
        self.stop = None;
    }

    /// Solde la position au stop (ou à l'ouverture si elle l'a franchi) et
    /// bloque ce sens jusqu'à la reprise de la tendance : clôture au-delà du
    /// meilleur cours atteint avant le stop, ou retour du score à 0.
    fn stop_out(&mut self, time: i64, price: f64, how: &str) {
        let long = self.units > 0.0;
        let stop = self.stop.unwrap_or(price);
        let best = self.best;
        self.close_all(
            time,
            price,
            FillKind::Stop,
            format!("Stop suiveur à {stop:.2} ({how})"),
        );
        if long {
            self.blocked_long = true;
            self.reentry_long = best;
        } else {
            self.blocked_short = true;
            self.reentry_short = best;
        }
        self.event(
            time,
            "Stop suiveur",
            format!(
                "Position {} soldée à {price:.2} ({how}). Nouvelle entrée dans ce sens après une clôture {} {best:.2} ou un retour du score à 0.",
                if long { "acheteuse" } else { "vendeuse" },
                if long { "au-dessus de" } else { "en dessous de" }
            ),
        );
    }

    /// Place le stop d'une position qui vient de s'ouvrir.
    fn arm_stop(&mut self, entry: f64, atr: Option<f64>) {
        self.best = entry;
        self.stop = match atr {
            Some(a) if self.stop_mult > 0.0 && self.units != 0.0 => {
                Some(trailing_stop(self.units.signum(), entry, a, self.stop_mult))
            }
            _ => None,
        };
    }

    /// Remonte le stop (jamais dans le sens défavorable) après une clôture.
    fn trail(&mut self, bar: &Bar, atr: Option<f64>) {
        let (Some(a), Some(current)) = (atr, self.stop) else {
            return;
        };
        if self.units > 0.0 {
            self.best = self.best.max(bar.high);
            self.stop = Some(current.max(trailing_stop(1.0, self.best, a, self.stop_mult)));
        } else if self.units < 0.0 {
            self.best = self.best.min(bar.low);
            self.stop = Some(current.min(trailing_stop(-1.0, self.best, a, self.stop_mult)));
        }
    }

    /// Funding des paiements (toutes les 8 h UTC) tombant dans la bougie, sur la
    /// position détenue ; cours de référence : milieu ouverture/clôture.
    fn pay_funding(&mut self, bar: &Bar, real: &HashMap<i64, f64>, default_rate: f64) {
        if self.market != Market::Perp || self.units == 0.0 {
            return;
        }
        let end = bar.time + self.interval.millis();
        let mut period = bar.time.div_euclid(FUNDING_PERIOD_MS);
        if period * FUNDING_PERIOD_MS < bar.time {
            period += 1;
        }
        let mark = (bar.open + bar.close) / 2.0;
        while period * FUNDING_PERIOD_MS < end {
            let rate = match real.get(&period) {
                Some(r) => {
                    self.funding_real += 1;
                    *r
                }
                None => {
                    self.funding_estimated += 1;
                    default_rate
                }
            };
            let payment = self.units * mark * rate;
            self.cash -= payment;
            self.funding += payment;
            period += 1;
        }
    }
}

/// Exécute le backtest sur `bars` (bougies closes, triées, préchauffage
/// compris) avec l'historique de funding `funding` (perpétuel, peut être vide).
pub fn run(
    bars: &[Bar],
    funding: &[FundingRate],
    params: &BacktestParams,
) -> Result<Backtest, String> {
    params.validate()?;
    let inst = params.instrument()?;
    let interval = params.interval;
    let allow_short = inst.market.allows_short();
    let max_lev = params.leverage_cap();
    let fee = params.fee_pct / 100.0;
    let slip = params.slippage_pct / 100.0;
    let threshold = params.rebalance_pct / 100.0;
    let target_vol = params.target_vol_pct / 100.0;
    let default_funding = params.funding_pct_8h / 100.0;

    if bars.is_empty() {
        return Err(format!("Aucune bougie disponible pour {}.", inst.symbol()));
    }
    let signal = trend_signal(bars, params.variant, allow_short);
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let vol = realized_vol(
        &closes,
        interval.bars_for_days(VOL_WINDOW_DAYS),
        interval.bars_per_year(),
    );
    let atr_v = atr(bars, ATR_LOOKBACK);

    let (start_ms, end_ms) = (date_ms(params.start), date_ms(params.end));
    let first_in_range = bars
        .iter()
        .position(|b| b.time >= start_ms && b.time < end_ms)
        .ok_or_else(|| format!("Aucune bougie {} dans la période demandée.", inst.symbol()))?;
    let ready = |i: usize| {
        i >= 1 && signal.score[i - 1].is_some() && vol[i - 1].is_some() && atr_v[i - 1].is_some()
    };
    let last = bars
        .iter()
        .rposition(|b| b.time < end_ms)
        .unwrap_or(first_in_range);
    let first = (first_in_range..=last).find(|&i| ready(i)).ok_or_else(|| {
        format!(
            "Historique insuffisant : il faut {} bougies avant la première décision, et seulement {} sont disponibles jusqu'à la fin de la période.",
            params.warmup_bars(),
            last + 1
        )
    })?;

    let mut warnings = Vec::new();
    if first > first_in_range {
        warnings.push(format!(
            "Début décalé au {} : l'historique Poloniex ne permet pas de calculer les indicateurs plus tôt ({} bougies de préchauffage).",
            fmt_time(bars[first].time, interval),
            params.warmup_bars()
        ));
    }
    if inst.market == Market::Spot && params.max_leverage > 1.0 {
        warnings.push("Spot : levier ramené à 1 (pas d'emprunt sur le comptant).".into());
    }
    if max_lev > RECOMMENDED_MAX_LEVERAGE {
        warnings.push(format!(
            "Levier maximal {max_lev:.1}x : au-delà des 2x retenus comme plafond pour le trading réel."
        ));
    }

    let real_funding: HashMap<i64, f64> = funding
        .iter()
        .map(|f| {
            (
                (f.time as f64 / FUNDING_PERIOD_MS as f64).round() as i64,
                f.rate,
            )
        })
        .collect();

    let mut sim = Sim {
        interval,
        market: inst.market,
        fee,
        slip,
        stop_mult: params.stop_atr_mult,
        cash: params.initial_capital,
        units: 0.0,
        best: 0.0,
        stop: None,
        blocked_long: false,
        blocked_short: false,
        reentry_long: f64::INFINITY,
        reentry_short: 0.0,
        halted: false,
        liquidated: false,
        fills: Vec::new(),
        events: Vec::new(),
        fees: 0.0,
        funding: 0.0,
        funding_real: 0,
        funding_estimated: 0,
        traded_notional: 0.0,
        trip_start: None,
        round_trips: Vec::new(),
    };

    // Décision prise à la clôture de la bougie `i`, pour l'ouverture suivante.
    let decide = |sim: &mut Sim, i: usize| -> f64 {
        let score = signal.score[i];
        // Après un stop : la tendance reprend (nouveau plus haut / plus bas) ou le signal se réinitialise.
        if sim.blocked_long && (score.is_some_and(|s| s <= 0.0) || closes[i] > sim.reentry_long) {
            sim.blocked_long = false;
        }
        if sim.blocked_short && (score.is_some_and(|s| s >= 0.0) || closes[i] < sim.reentry_short) {
            sim.blocked_short = false;
        }
        let target = match (score, vol[i]) {
            (Some(s), Some(v)) => target_exposure(s, v, target_vol, max_lev, allow_short),
            _ => 0.0,
        };
        if (target > 0.0 && sim.blocked_long) || (target < 0.0 && sim.blocked_short) {
            0.0
        } else {
            target
        }
    };

    let entry_price = bars[first].open * (1.0 + slip) * (1.0 + fee);
    let bh_units = params.initial_capital / entry_price;
    let mut points = Vec::with_capacity(last + 1 - first);
    let mut pending = Some(decide(&mut sim, first - 1));
    let mut peak = params.initial_capital;
    let mut prev_equity = params.initial_capital;
    let mut day = bars[first].date();
    let mut day_start = params.initial_capital;
    let mut daily_fired = false;
    let mut dd_breached = false;

    for i in first..=last {
        let bar = bars[i];

        // 1. Ouverture : stop franchi par un écart d'ouverture, puis ordre en attente.
        if !sim.halted {
            if let Some(stop) = sim.stop {
                let gap =
                    (sim.units > 0.0 && bar.open <= stop) || (sim.units < 0.0 && bar.open >= stop);
                if gap {
                    sim.stop_out(bar.time, bar.open, "écart d'ouverture");
                }
            }
            if let Some(mut target) = pending.take() {
                if (target > 0.0 && sim.blocked_long) || (target < 0.0 && sim.blocked_short) {
                    target = 0.0;
                }
                let current = sim.exposure(bar.open);
                if needs_rebalance(current, target, threshold, max_lev) {
                    let before = sim.units;
                    let mut delta = target * sim.equity(bar.open) / bar.open - sim.units;
                    if inst.market == Market::Spot && delta > 0.0 {
                        // Pas d'emprunt : l'achat, frais et glissement compris, tient dans la trésorerie.
                        let max_buy = sim.cash.max(0.0) / (bar.open * (1.0 + slip) * (1.0 + fee));
                        delta = delta.min(max_buy);
                    }
                    sim.execute(
                        bar.time,
                        delta,
                        bar.open,
                        FillKind::Rebalance,
                        format!("Exposition {current:+.2}x → {target:+.2}x"),
                    );
                    if sim.units == 0.0 {
                        sim.stop = None;
                    } else if before == 0.0 || before.signum() != sim.units.signum() {
                        sim.arm_stop(bar.open, atr_v[i - 1]);
                    }
                }
            }
        }

        // 2. Pendant la bougie : funding, stop suiveur, liquidation.
        sim.pay_funding(&bar, &real_funding, default_funding);
        if !sim.halted {
            if let Some(stop) = sim.stop {
                let hit =
                    (sim.units > 0.0 && bar.low <= stop) || (sim.units < 0.0 && bar.high >= stop);
                if hit {
                    sim.stop_out(bar.time, stop, "touché en cours de bougie");
                }
            }
        }
        if sim.units != 0.0 {
            let worst = if sim.units > 0.0 { bar.low } else { bar.high };
            if sim.equity(worst) <= MAINTENANCE_MARGIN * sim.units.abs() * worst {
                sim.close_all(
                    bar.time,
                    worst,
                    FillKind::Liquidation,
                    "Marge de maintenance atteinte".into(),
                );
                sim.cash = sim.cash.max(0.0);
                sim.liquidated = true;
                sim.halted = true;
                sim.event(
                    bar.time,
                    "Liquidation",
                    format!("Position liquidée vers {worst:.2} : le capital restant ne couvrait plus la marge de maintenance."),
                );
            }
        }

        // 3. Clôture : valorisation, coupe-circuits, stop, décision suivante.
        let equity = sim.equity(bar.close);
        if bar.date() != day {
            day = bar.date();
            day_start = prev_equity;
            daily_fired = false;
        }
        peak = peak.max(equity);
        let drawdown = if peak > 0.0 { equity / peak - 1.0 } else { 0.0 };
        let mut breach = None;
        if !daily_fired && day_start > 0.0 && equity / day_start - 1.0 <= -DAILY_LOSS_LIMIT {
            daily_fired = true;
            breach = Some(format!(
                "Perte de {:.1} % sur la journée (seuil : {:.0} %).",
                (1.0 - equity / day_start) * 100.0,
                DAILY_LOSS_LIMIT * 100.0
            ));
        }
        if drawdown <= -MAX_DRAWDOWN_LIMIT {
            if !dd_breached {
                dd_breached = true;
                breach.get_or_insert(format!(
                    "Baisse de {:.1} % depuis le plus haut du capital (seuil : {:.0} %).",
                    -drawdown * 100.0,
                    MAX_DRAWDOWN_LIMIT * 100.0
                ));
            }
        } else {
            dd_breached = false;
        }
        if let Some(detail) = breach {
            if !sim.halted {
                let action = if params.halt_on_breach {
                    " Le moteur solde la position et s'arrête (reprise manuelle)."
                } else {
                    " Le backtest continue (arrêt désactivé dans les paramètres)."
                };
                sim.event(bar.time, "Coupe-circuit", format!("{detail}{action}"));
                if params.halt_on_breach {
                    if sim.units != 0.0 {
                        sim.close_all(bar.time, bar.close, FillKind::CircuitBreaker, detail);
                    }
                    sim.halted = true;
                }
            }
        }
        sim.trail(&bar, atr_v[i]);

        let equity = sim.equity(bar.close);
        prev_equity = equity;
        points.push(EquityPoint {
            time: bar.time,
            close: bar.close,
            equity,
            buy_hold: bh_units * bar.close,
            drawdown_pct: if peak > 0.0 {
                (equity / peak - 1.0) * 100.0
            } else {
                0.0
            },
            exposure: sim.exposure(bar.close),
            score: signal.score[i],
        });
        if i < last && !sim.halted {
            pending = Some(decide(&mut sim, i));
        }
    }

    Ok(Backtest {
        params: params.clone(),
        symbol: inst.symbol(),
        points,
        fills: sim.fills,
        events: sim.events,
        round_trips: sim.round_trips,
        fees: sim.fees,
        funding: sim.funding,
        funding_real_periods: sim.funding_real,
        funding_estimated_periods: sim.funding_estimated,
        traded_notional: sim.traded_notional,
        liquidated: sim.liquidated,
        halted: sim.halted,
        warnings,
    })
}

/// Charge les bougies et le funding Poloniex nécessaires, puis exécute le backtest.
/// Sans historique de funding (réseau, API), le funding supposé s'applique partout.
pub async fn run_with_feed(feed: &MarketFeed, params: &BacktestParams) -> Result<Backtest, String> {
    params.validate()?;
    let inst = params.instrument()?;
    let bars = feed
        .bars(&inst, params.interval, params.data_start())
        .await?;
    let (funding, funding_error) = match feed.funding(&inst).await {
        Ok(f) => (f, None),
        Err(e) => (Default::default(), Some(e)),
    };
    let mut bt = run(&bars, &funding, params)?;
    if let Some(e) = funding_error {
        bt.warnings.push(format!(
            "Historique du funding indisponible ({e}) : funding supposé de {} % par 8 h sur toute la période.",
            params.funding_pct_8h
        ));
    }
    Ok(bt)
}

impl Backtest {
    pub fn summary(&self) -> Summary {
        let bpy = self.params.interval.bars_per_year();
        let times: Vec<i64> = self.points.iter().map(|p| p.time).collect();
        let col = |f: fn(&EquityPoint) -> f64| self.points.iter().map(f).collect::<Vec<f64>>();
        let capital = self.params.initial_capital;
        let invested: Vec<f64> = self
            .points
            .iter()
            .map(|p| p.exposure.abs())
            .filter(|e| *e > 1e-9)
            .collect();
        let n = self.points.len().max(1) as f64;
        let avg_equity = self.points.iter().map(|p| p.equity).sum::<f64>() / n;
        let wins = self.round_trips.iter().filter(|r| **r > 0.0).count();
        let interval = self.params.interval;
        Summary {
            strategy: performance(&times, &col(|p| p.equity), capital, bpy),
            buy_hold: performance(&times, &col(|p| p.buy_hold), capital, bpy),
            fills: self.fills.len(),
            round_trips: self.round_trips.len(),
            win_rate_pct: (!self.round_trips.is_empty())
                .then(|| wins as f64 / self.round_trips.len() as f64 * 100.0),
            exposure_pct: invested.len() as f64 / n * 100.0,
            avg_leverage: if invested.is_empty() {
                0.0
            } else {
                invested.iter().sum::<f64>() / invested.len() as f64
            },
            fees: self.fees,
            funding: self.funding,
            funding_real_periods: self.funding_real_periods,
            funding_estimated_periods: self.funding_estimated_periods,
            turnover: if avg_equity > 0.0 {
                self.traded_notional / avg_equity / (n / bpy)
            } else {
                0.0
            },
            stops: self
                .fills
                .iter()
                .filter(|f| f.kind == FillKind::Stop)
                .count(),
            circuit_breakers: self
                .events
                .iter()
                .filter(|e| e.kind == "Coupe-circuit")
                .count(),
            liquidated: self.liquidated,
            halted: self.halted,
            first_date: self
                .points
                .first()
                .map(|p| fmt_time(p.time, interval))
                .unwrap_or_default(),
            last_date: self
                .points
                .last()
                .map(|p| fmt_time(p.time, interval))
                .unwrap_or_default(),
        }
    }

    /// Courbe de capital au format CSV (une ligne par bougie).
    pub fn to_csv(&self) -> String {
        let mut out = String::from("Date,Close,Equity,Buy_Hold,Drawdown_pct,Exposure,Score\n");
        for p in &self.points {
            let _ = writeln!(
                out,
                "{},{},{:.2},{:.2},{:.4},{:.4},{}",
                fmt_time(p.time, self.params.interval),
                p.close,
                p.equity,
                p.buy_hold,
                p.drawdown_pct,
                p.exposure,
                p.score.map(|s| s.to_string()).unwrap_or_default()
            );
        }
        out
    }

    /// Ordres exécutés au format CSV.
    pub fn fills_csv(&self) -> String {
        let mut out =
            String::from("Date,Origine,Sens,Prix,Quantite,Montant,Frais,Exposition_apres,Detail\n");
        for f in &self.fills {
            let kind = serde_json::to_value(f.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{},{},{},{},{},{:.2},{:.4},{:.4},\"{}\"",
                f.date,
                kind,
                f.side,
                f.price,
                f.quantity,
                f.notional,
                f.fee,
                f.exposure_after,
                f.detail.replace('"', "'")
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::DAY_MS;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Bougies journalières dès le 01/01/2020, sans mèche sauf `wick` (fraction).
    fn bars_from(closes: &[f64], wick: f64) -> Vec<Bar> {
        let t0 = date_ms(d("2020-01-01"));
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let open = if i == 0 { c } else { closes[i - 1] };
                Bar {
                    time: t0 + i as i64 * DAY_MS,
                    open,
                    high: open.max(c) * (1.0 + wick),
                    low: open.min(c) * (1.0 - wick),
                    close: c,
                    volume: 0.0,
                }
            })
            .collect()
    }

    /// Hausse régulière bruitée (volatilité réalisée non nulle).
    fn uptrend(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 * 1.003f64.powi(i as i32) * (1.0 + 0.01 * ((i % 5) as f64 - 2.0)))
            .collect()
    }

    fn params(market: Market) -> BacktestParams {
        BacktestParams {
            base: "BTC".into(),
            market,
            interval: Interval::D1,
            start: d("2020-09-01"),
            end: d("2030-01-01"),
            initial_capital: 10_000.0,
            variant: TrendVariant::Donchian,
            target_vol_pct: 25.0,
            max_leverage: 1.0,
            stop_atr_mult: 3.0,
            fee_pct: 0.075,
            slippage_pct: 0.05,
            rebalance_pct: 20.0,
            funding_pct_8h: 0.01,
            halt_on_breach: false,
        }
    }

    #[test]
    fn uptrend_goes_long_and_beats_cash() {
        let bars = bars_from(&uptrend(600), 0.002);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        let s = bt.summary();
        assert!(s.strategy.final_equity > 10_000.0, "{s:?}");
        assert!(bt
            .points
            .iter()
            .all(|p| p.exposure >= 0.0 && p.exposure <= 1.0 + 1e-9));
        assert!(s.fees > 0.0);
        assert!(bt.points.iter().all(|p| p.equity > 0.0));
        assert!(bt.warnings.is_empty(), "{:?}", bt.warnings);
    }

    #[test]
    fn decisions_are_executed_at_the_next_open() {
        let bars = bars_from(&uptrend(600), 0.002);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        let first = &bt.fills[0];
        // La première décision est prise à la clôture de la veille du début :
        // l'ordre part à l'ouverture de la première bougie du backtest.
        assert_eq!(first.time, bt.points[0].time);
        let open = bars.iter().find(|b| b.time == first.time).unwrap().open;
        assert!(
            (first.price - open * 1.0005).abs() < 1e-9,
            "glissement de 0,05 %"
        );
        assert!((first.fee - first.notional * 0.00075).abs() < 1e-9);
    }

    #[test]
    fn spot_never_borrows() {
        let bars = bars_from(&uptrend(600), 0.002);
        let mut p = params(Market::Spot);
        p.target_vol_pct = 100.0; // pousse l'exposition vers le plafond
        p.max_leverage = 3.0;
        let bt = run(&bars, &[], &p).unwrap();
        assert!(bt.points.iter().all(|pt| pt.exposure <= 1.0 + 1e-9));
        assert!(bt.warnings.iter().any(|w| w.contains("Spot")));
    }

    #[test]
    fn flat_market_stays_in_cash() {
        let closes: Vec<f64> = (0..400)
            .map(|i| 100.0 + if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let bars = bars_from(&closes, 0.0);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        assert!(
            bt.fills.is_empty(),
            "aucune cassure : {:?}",
            bt.fills.first()
        );
        assert_eq!(bt.points.last().unwrap().equity, 10_000.0);
    }

    #[test]
    fn crash_triggers_the_trailing_stop_and_blocks_reentry() {
        let mut closes = uptrend(500);
        let top = *closes.last().unwrap();
        // Chute de 30 % en une bougie, puis rebond sous l'ancien plus haut.
        closes.push(top * 0.7);
        closes.extend((1..=20).map(|i| top * (0.7 + 0.005 * i as f64)));
        let bars = bars_from(&closes, 0.002);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        let stop = bt
            .fills
            .iter()
            .find(|f| f.kind == FillKind::Stop)
            .expect("stop exécuté");
        assert_eq!(stop.side, "Vente");
        assert!(bt.events.iter().any(|e| e.kind == "Stop suiveur"));
        let after: Vec<&Fill> = bt.fills.iter().filter(|f| f.time > stop.time).collect();
        assert!(
            after.iter().all(|f| f.side == "Vente"),
            "pas de rachat tant que le score reste positif"
        );
        assert_eq!(bt.points.last().unwrap().exposure, 0.0);
    }

    #[test]
    fn a_new_high_after_a_stop_reopens_the_position() {
        let mut closes = uptrend(500);
        let top = *closes.last().unwrap();
        // Repli de 12 % en trois bougies (stop touché, tendance de fond intacte),
        // puis reprise jusqu'à de nouveaux plus hauts.
        closes.extend((1..=3).map(|i| top * (1.0 - 0.04 * i as f64)));
        closes.extend((1..=30).map(|i| top * 0.88 * 1.01f64.powi(i)));
        let bars = bars_from(&closes, 0.002);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        let stop = bt
            .fills
            .iter()
            .rfind(|f| f.kind == FillKind::Stop)
            .expect("stop exécuté");
        let rebuy = bt
            .fills
            .iter()
            .find(|f| f.time > stop.time && f.side == "Achat")
            .expect("rachat après un nouveau plus haut");
        // Bougie dont la clôture a décidé le rachat (la veille de l'ordre).
        let new_high = bars.iter().rfind(|b| b.time < rebuy.time).unwrap();
        let best = bars
            .iter()
            .filter(|b| b.time < stop.time)
            .map(|b| b.high)
            .fold(0.0, f64::max);
        assert!(
            new_high.close > best * 0.99,
            "rachat seulement après la reprise : {} / {best}",
            new_high.close
        );
        assert!(bt.points.last().unwrap().exposure > 0.0);
    }

    #[test]
    fn perp_shorts_a_downtrend_and_pays_or_receives_funding() {
        let closes: Vec<f64> = (0..600)
            .map(|i| 100.0 * 0.997f64.powi(i) * (1.0 + 0.01 * ((i % 5) as f64 - 2.0)))
            .collect();
        let bars = bars_from(&closes, 0.002);
        let mut p = params(Market::Perp);
        p.max_leverage = 2.0;
        let bt = run(&bars, &[], &p).unwrap();
        assert!(
            bt.points.iter().any(|pt| pt.exposure < 0.0),
            "position vendeuse"
        );
        assert!(bt.summary().strategy.final_equity > 10_000.0);
        // Funding positif estimé : une position vendeuse le reçoit.
        assert!(bt.funding < 0.0, "funding reçu : {}", bt.funding);
        assert!(bt.funding_estimated_periods > 0 && bt.funding_real_periods == 0);
    }

    #[test]
    fn real_funding_is_used_when_available() {
        let bars = bars_from(&uptrend(600), 0.002);
        let mut p = params(Market::Perp);
        p.funding_pct_8h = 0.0;
        // Taux réel de 0,1 % sur tous les paiements : une position acheteuse paie.
        let funding: Vec<FundingRate> = bars
            .iter()
            .flat_map(|b| {
                (0..3).map(move |k| FundingRate {
                    time: b.time + k * FUNDING_PERIOD_MS,
                    rate: 0.001,
                })
            })
            .collect();
        let bt = run(&bars, &funding, &p).unwrap();
        assert!(bt.funding > 0.0);
        assert_eq!(bt.funding_estimated_periods, 0);
        assert!(bt.funding_real_periods > 0);
        let without = run(&bars, &[], &p).unwrap();
        assert_eq!(without.funding, 0.0, "funding supposé nul");
        assert!(bt.summary().strategy.final_equity < without.summary().strategy.final_equity);
    }

    #[test]
    fn circuit_breaker_halts_when_enabled() {
        // Hausse, chute de 8 % en une bougie, puis reprise.
        let mut closes = uptrend(500);
        let top = *closes.last().unwrap();
        closes.push(top * 0.92);
        closes.extend((1..=20).map(|i| top * 0.92 * 1.003f64.powi(i)));
        let bars = bars_from(&closes, 0.0);
        let mut p = params(Market::Spot);
        p.target_vol_pct = 100.0; // position pleine : une baisse de plus de 3 % sur un jour suffit
        p.stop_atr_mult = 0.0; // sans stop, seul le coupe-circuit protège
        let free = run(&bars, &[], &p).unwrap();
        assert!(free.events.iter().any(|e| e.kind == "Coupe-circuit"));
        assert!(!free.halted);

        p.halt_on_breach = true;
        let halted = run(&bars, &[], &p).unwrap();
        assert!(halted.halted);
        let event = halted
            .events
            .iter()
            .find(|e| e.kind == "Coupe-circuit")
            .unwrap();
        assert!(halted
            .points
            .iter()
            .filter(|pt| pt.time >= event.time)
            .all(|pt| pt.exposure == 0.0));
        assert_eq!(
            halted.fills.last().unwrap().kind,
            FillKind::CircuitBreaker,
            "plus aucun ordre après l'arrêt"
        );
    }

    #[test]
    fn insufficient_history_is_reported_or_shifted() {
        let bars = bars_from(&uptrend(150), 0.002);
        let mut p = params(Market::Spot);
        p.start = d("2020-01-10");
        let err = run(&bars, &[], &p).unwrap_err();
        assert!(err.contains("Historique insuffisant"), "{err}");

        // Début demandé avant le préchauffage : décalé, avec un avertissement.
        let bars = bars_from(&uptrend(600), 0.002);
        let mut p = params(Market::Spot);
        p.start = d("2020-01-15");
        let bt = run(&bars, &[], &p).unwrap();
        assert!(bt.warnings.iter().any(|w| w.contains("Début décalé")));
        assert!(bt.points[0].time >= bars[TrendVariant::Donchian.warmup_bars()].time);
    }

    #[test]
    fn invalid_params_are_rejected() {
        let base = params(Market::Spot);
        let bad = [
            BacktestParams {
                end: base.start,
                ..base.clone()
            },
            BacktestParams {
                initial_capital: f64::NAN,
                ..base.clone()
            },
            BacktestParams {
                max_leverage: 10.0,
                ..base.clone()
            },
            BacktestParams {
                fee_pct: 2.0,
                ..base.clone()
            },
            BacktestParams {
                base: "BTC/USDT".into(),
                ..base.clone()
            },
            BacktestParams {
                target_vol_pct: 0.0,
                ..base.clone()
            },
        ];
        for p in bad {
            assert!(p.validate().is_err(), "{p:?}");
        }
        assert!(base.validate().is_ok());
    }

    #[test]
    fn data_start_covers_the_warmup() {
        let p = params(Market::Spot);
        // 201 bougies journalières + 7 jours de marge.
        assert_eq!(p.data_start(), p.start - Duration::days(208));
        let h4 = BacktestParams {
            interval: Interval::H4,
            ..p
        };
        // 4 h : 30 jours de volatilité (181 bougies) < 201 bougies Donchian = 34 jours.
        assert_eq!(h4.data_start(), h4.start - Duration::days(34 + 7));
    }

    #[test]
    fn csv_exports() {
        let bars = bars_from(&uptrend(600), 0.002);
        let bt = run(&bars, &[], &params(Market::Spot)).unwrap();
        let csv = bt.to_csv();
        assert!(csv.starts_with("Date,Close,Equity,Buy_Hold,"));
        assert_eq!(csv.lines().count(), bt.points.len() + 1);
        let fills = bt.fills_csv();
        assert_eq!(fills.lines().count(), bt.fills.len() + 1);
        assert!(fills.lines().nth(1).unwrap().contains(",Signal,Achat,"));
    }
}

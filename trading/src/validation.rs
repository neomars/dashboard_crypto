//! Tests de validité de la stratégie Fibonacci : niveaux placebo, Monte-Carlo
//! sur la suite des trades, verdict automatique et repères de contrôle.
//!
//! Le module peut conclure « avantage non démontré » : c'est son résultat par
//! défaut. Un avantage n'est retenu que sur la **période de test** (jamais sur
//! celle qui a servi à régler les paramètres), avec une espérance nette positive
//! par trade, un rang supérieur ou égal à 95 % face aux niveaux placebo et au
//! moins 200 trades.

use crate::leveraged::{ExitReason, SimInput, SimParams, SimResult, Trade};
use crate::market::Bar;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Nombre de trades minimal, sur la période de test, pour conclure.
pub const MIN_TRADES: usize = 200;
/// Rang minimal de Fibonacci parmi les tirages placebo.
pub const MIN_PERCENTILE: f64 = 0.95;

/// Générateur pseudo-aléatoire déterministe (SplitMix64) : mêmes graines, mêmes tirages.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniforme sur [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Entier uniforme sur [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_f64() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// Mesures d'une simulation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunSummary {
    pub trades: usize,
    pub wins: usize,
    pub win_rate_pct: Option<f64>,
    /// Espérance nette par trade, en R (perte prévue au stop).
    pub expectancy_r_net: Option<f64>,
    pub expectancy_r_gross: Option<f64>,
    pub expectancy_usdt: Option<f64>,
    pub total_net: f64,
    pub fees: f64,
    pub funding: f64,
    /// Frais, funding et marges : rapportés à la marge totale engagée.
    pub margin_total: f64,
    pub fees_pct_of_margin: Option<f64>,
    pub funding_pct_of_margin: Option<f64>,
    pub final_budget: f64,
    pub initial_budget: f64,
    pub ruined: bool,
    pub liquidations: usize,
    pub max_drawdown_pct: f64,
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (n, s) = values.fold((0usize, 0.0), |(n, s), x| (n + 1, s + x));
    (n > 0).then(|| s / n as f64)
}

pub fn summarize(r: &SimResult) -> RunSummary {
    let t = &r.trades;
    let wins = t.iter().filter(|x| x.net > 0.0).count();
    let margin_total: f64 = t.iter().map(|x| x.margin).sum();
    let fees: f64 = t.iter().map(|x| x.fee_entry + x.fee_exit).sum();
    let funding: f64 = t.iter().map(|x| x.funding).sum();
    // Pire baisse du budget, du plus haut au plus bas.
    let (mut peak, mut max_dd) = (r.initial_budget, 0.0f64);
    for (_, b) in &r.budget_curve {
        peak = peak.max(*b);
        if peak > 0.0 {
            max_dd = max_dd.min(b / peak - 1.0);
        }
    }
    RunSummary {
        trades: t.len(),
        wins,
        win_rate_pct: (!t.is_empty()).then(|| wins as f64 / t.len() as f64 * 100.0),
        expectancy_r_net: mean(t.iter().map(|x| x.r_net)),
        expectancy_r_gross: mean(t.iter().map(|x| x.r_gross)),
        expectancy_usdt: mean(t.iter().map(|x| x.net)),
        total_net: t.iter().map(|x| x.net).sum(),
        fees,
        funding,
        margin_total,
        fees_pct_of_margin: (margin_total > 0.0).then(|| fees / margin_total * 100.0),
        funding_pct_of_margin: (margin_total > 0.0).then(|| funding / margin_total * 100.0),
        final_budget: r.final_budget,
        initial_budget: r.initial_budget,
        ruined: r.ruined,
        liquidations: r.stats.liquidations,
        max_drawdown_pct: max_dd * 100.0,
    }
}

/// Résultats d'un niveau de retracement.
#[derive(Debug, Clone, Serialize)]
pub struct LevelDetail {
    pub ratio: f64,
    pub trades: usize,
    pub win_rate_pct: Option<f64>,
    pub expectancy_r_net: Option<f64>,
    pub expectancy_r_gross: Option<f64>,
    pub total_net: f64,
}

/// Détail par niveau d'une liste de trades.
pub fn per_level(trades: &[Trade]) -> Vec<LevelDetail> {
    let mut ratios: Vec<f64> = trades.iter().map(|t| t.ratio).collect();
    ratios.sort_by(f64::total_cmp);
    ratios.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    ratios
        .into_iter()
        .map(|ratio| {
            let sel: Vec<&Trade> = trades
                .iter()
                .filter(|t| (t.ratio - ratio).abs() < 1e-9)
                .collect();
            let wins = sel.iter().filter(|t| t.net > 0.0).count();
            LevelDetail {
                ratio,
                trades: sel.len(),
                win_rate_pct: (!sel.is_empty()).then(|| wins as f64 / sel.len() as f64 * 100.0),
                expectancy_r_net: mean(sel.iter().map(|t| t.r_net)),
                expectancy_r_gross: mean(sel.iter().map(|t| t.r_gross)),
                total_net: sel.iter().map(|t| t.net).sum(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceboParams {
    /// Nombre de tirages (200 par défaut).
    pub draws: usize,
    /// Graine : mêmes tirages à chaque exécution.
    pub seed: u64,
    /// Bornes de l'intervalle uniforme des niveaux tirés.
    pub low: f64,
    pub high: f64,
    /// Niveaux fixes comparables, simulés ensemble avec le détail par niveau.
    pub fixed_levels: Vec<f64>,
}

impl Default for PlaceboParams {
    fn default() -> Self {
        Self {
            draws: 200,
            seed: 20_260_101,
            low: 0.30,
            high: 0.80,
            fixed_levels: vec![0.45, 0.55, 0.70],
        }
    }
}

impl PlaceboParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.draws == 0 || self.draws > 5000 {
            return Err("Le nombre de tirages placebo doit être compris entre 1 et 5000.".into());
        }
        if !(self.low.is_finite()
            && self.high.is_finite()
            && 0.0 < self.low
            && self.low < self.high
            && self.high < 1.0)
        {
            return Err(
                "Les bornes des niveaux placebo doivent vérifier 0 < min < max < 1.".into(),
            );
        }
        if self.fixed_levels.iter().any(|r| !(*r > 0.0 && *r < 1.0)) {
            return Err("Les niveaux fixes doivent être compris entre 0 et 1 (exclus).".into());
        }
        Ok(())
    }

    /// Niveaux du tirage `k` : `count` valeurs uniformes sur `[low, high]`,
    /// distinctes, ne dépendant que de la graine et de `k`.
    pub fn draw_levels(&self, k: usize, count: usize) -> Vec<f64> {
        let mut rng = Rng::new(self.seed ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut levels: Vec<f64> = Vec::with_capacity(count);
        while levels.len() < count.max(1) {
            // Arrondi à 4 décimales : lisible, et deux niveaux ne se confondent pas.
            let r = ((self.low + (self.high - self.low) * rng.next_f64()) * 1e4).round() / 1e4;
            if levels.iter().all(|x| (x - r).abs() > 1e-4) {
                levels.push(r);
            }
        }
        levels.sort_by(f64::total_cmp);
        levels
    }
}

/// Un tirage placebo.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceboDraw {
    pub levels: Vec<f64>,
    pub trades: usize,
    pub expectancy_r_net: Option<f64>,
    pub final_budget: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaceboResult {
    pub draws: Vec<PlaceboDraw>,
    /// Espérance nette (R par trade) de Fibonacci, et son rang parmi les tirages :
    /// part des tirages (avec au moins un trade) qu'elle bat strictement.
    pub fibo_expectancy_r_net: Option<f64>,
    pub percentile: Option<f64>,
    pub valid_draws: usize,
    pub mean: Option<f64>,
    pub p5: Option<f64>,
    pub median: Option<f64>,
    pub p95: Option<f64>,
}

/// Quantile (interpolation linéaire) d'une liste triée.
pub fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64))
}

/// Rang de `value` : part des `others` strictement inférieurs.
pub fn percentile_rank(value: f64, others: &[f64]) -> Option<f64> {
    (!others.is_empty())
        .then(|| others.iter().filter(|x| **x < value).count() as f64 / others.len() as f64)
}

/// Relance exactement le même moteur avec des niveaux tirés au hasard.
/// `progress(k, n)` est appelé après chaque tirage ; `cancel` l'interrompt.
pub fn run_placebo(
    input: &SimInput,
    params: &SimParams,
    placebo: &PlaceboParams,
    fibo_expectancy: Option<f64>,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> PlaceboResult {
    let count = params.fibo.ratios.len();
    let n = placebo.draws;
    let workers = std::thread::available_parallelism()
        .map_or(1, |p| p.get())
        .clamp(1, 16)
        .min(n.max(1));
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let mut slots: Vec<Option<PlaceboDraw>> = vec![None; n];
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut out: Vec<(usize, PlaceboDraw)> = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        if k >= n || cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let levels = placebo.draw_levels(k, count);
                        let mut p = params.clone();
                        p.fibo.ratios = levels.clone();
                        let r = input.run(&p);
                        let s = summarize(&r);
                        out.push((
                            k,
                            PlaceboDraw {
                                levels,
                                trades: s.trades,
                                expectancy_r_net: s.expectancy_r_net,
                                final_budget: s.final_budget,
                            },
                        ));
                        progress(done.fetch_add(1, Ordering::Relaxed) + 1, n);
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            for (k, d) in h.join().unwrap_or_default() {
                slots[k] = Some(d);
            }
        }
    });
    let draws: Vec<PlaceboDraw> = slots.into_iter().flatten().collect();
    let mut values: Vec<f64> = draws.iter().filter_map(|d| d.expectancy_r_net).collect();
    values.sort_by(f64::total_cmp);
    PlaceboResult {
        fibo_expectancy_r_net: fibo_expectancy,
        percentile: fibo_expectancy.and_then(|f| percentile_rank(f, &values)),
        valid_draws: values.len(),
        mean: mean(values.iter().copied()),
        p5: quantile(&values, 0.05),
        median: quantile(&values, 0.5),
        p95: quantile(&values, 0.95),
        draws,
    }
}

/// Monte-Carlo sur la suite des trades.
#[derive(Debug, Clone, Serialize)]
pub struct MonteCarlo {
    pub draws: usize,
    /// Trades par tirage : autant que dans la période observée.
    pub trades_per_draw: usize,
    pub initial_budget: f64,
    /// Budget final : moyenne et quantiles 5, 25, 50, 75, 95 %.
    pub final_mean: f64,
    pub final_quantiles: [f64; 5],
    /// Probabilité que le budget passe sous 50 % de son montant initial.
    pub prob_loss_50: f64,
    /// Probabilité de perdre tout le budget : la marge du trade suivant n'est plus finançable.
    pub prob_ruin: f64,
    /// Pire série de trades perdants : médiane et 95e centile des tirages, et celle observée.
    pub worst_streak_median: f64,
    pub worst_streak_p95: f64,
    pub worst_streak_observed: usize,
    /// Histogramme du budget final : bornes et effectifs.
    pub histogram: Vec<(f64, usize)>,
}

fn worst_losing_streak(results: impl Iterator<Item = f64>) -> usize {
    let (mut best, mut cur) = (0, 0);
    for net in results {
        if net < 0.0 {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// Rééchantillonne (avec remise) les trades observés : chaque tirage rejoue autant
/// de trades que la période en contient, résultat en R et en USDT ensemble
/// (le même trade garde sa taille de risque). Le budget s'arrête quand la marge du
/// trade suivant n'est plus finançable.
pub fn monte_carlo(
    trades: &[Trade],
    initial_budget: f64,
    draws: usize,
    seed: u64,
) -> Option<MonteCarlo> {
    if trades.is_empty() || draws == 0 {
        return None;
    }
    let n = trades.len();
    let mut rng = Rng::new(seed);
    let (mut finals, mut streaks) = (Vec::with_capacity(draws), Vec::with_capacity(draws));
    let (mut loss50, mut ruin) = (0usize, 0usize);
    for _ in 0..draws {
        let mut budget = initial_budget;
        let mut lowest = budget;
        let (mut cur, mut worst) = (0usize, 0usize);
        let mut ruined = false;
        for _ in 0..n {
            let t = &trades[rng.below(n)];
            if budget < t.margin + t.fee_entry {
                ruined = true;
                break;
            }
            budget += t.net;
            lowest = lowest.min(budget);
            if t.net < 0.0 {
                cur += 1;
                worst = worst.max(cur);
            } else {
                cur = 0;
            }
        }
        loss50 += usize::from(lowest <= 0.5 * initial_budget);
        ruin += usize::from(ruined || budget <= 0.0);
        finals.push(budget.max(0.0));
        streaks.push(worst as f64);
    }
    finals.sort_by(f64::total_cmp);
    streaks.sort_by(f64::total_cmp);
    let q = |v: &[f64], p: f64| quantile(v, p).unwrap_or(0.0);
    let (lo, hi) = (finals[0], finals[finals.len() - 1]);
    let bins = 20usize;
    let width = ((hi - lo) / bins as f64).max(1e-9);
    let mut counts = vec![0usize; bins];
    for f in &finals {
        counts[(((f - lo) / width) as usize).min(bins - 1)] += 1;
    }
    Some(MonteCarlo {
        draws,
        trades_per_draw: n,
        initial_budget,
        final_mean: finals.iter().sum::<f64>() / draws as f64,
        final_quantiles: [
            q(&finals, 0.05),
            q(&finals, 0.25),
            q(&finals, 0.5),
            q(&finals, 0.75),
            q(&finals, 0.95),
        ],
        prob_loss_50: loss50 as f64 / draws as f64,
        prob_ruin: ruin as f64 / draws as f64,
        worst_streak_median: q(&streaks, 0.5),
        worst_streak_p95: q(&streaks, 0.95),
        worst_streak_observed: worst_losing_streak(trades.iter().map(|t| t.net)),
        histogram: counts
            .into_iter()
            .enumerate()
            .map(|(k, c)| (lo + width * k as f64, c))
            .collect(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verdict {
    #[serde(rename = "Pas assez de trades")]
    NotEnoughTrades,
    #[serde(rename = "Avantage non démontré")]
    NotDemonstrated,
    #[serde(rename = "Avantage démontré sur la période de test")]
    Demonstrated,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerdictInfo {
    pub verdict: Verdict,
    pub label: String,
    /// Critères : chacun avec son résultat, pour que le verdict soit lisible.
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Criterion {
    pub name: String,
    pub value: String,
    pub ok: bool,
}

/// Verdict automatique, **sur la période de test**. « Avantage non démontré »
/// sauf si l'espérance nette par trade est positive, Fibonacci bat au moins 95 %
/// des tirages placebo et il y a au moins 200 trades ; « pas assez de trades »
/// en dessous de 200.
pub fn verdict(test: &RunSummary, percentile: Option<f64>) -> VerdictInfo {
    let exp = test.expectancy_r_net;
    let enough = test.trades >= MIN_TRADES;
    let positive = exp.is_some_and(|e| e > 0.0);
    let beats = percentile.is_some_and(|p| p >= MIN_PERCENTILE);
    let verdict = if !enough {
        Verdict::NotEnoughTrades
    } else if positive && beats {
        Verdict::Demonstrated
    } else {
        Verdict::NotDemonstrated
    };
    let fmt_opt = |v: Option<f64>, f: &dyn Fn(f64) -> String| v.map_or("–".to_string(), f);
    VerdictInfo {
        verdict,
        label: match verdict {
            Verdict::NotEnoughTrades => "Pas assez de trades",
            Verdict::NotDemonstrated => "Avantage non démontré",
            Verdict::Demonstrated => "Avantage démontré sur la période de test",
        }
        .to_string(),
        criteria: vec![
            Criterion {
                name: format!("Au moins {MIN_TRADES} trades"),
                value: test.trades.to_string(),
                ok: enough,
            },
            Criterion {
                name: "Espérance nette par trade positive".into(),
                value: fmt_opt(exp, &|e| format!("{e:+.3} R")),
                ok: positive,
            },
            Criterion {
                name: format!(
                    "Mieux que {:.0} % des niveaux placebo",
                    MIN_PERCENTILE * 100.0
                ),
                value: fmt_opt(percentile, &|p| format!("rang {:.1} %", p * 100.0)),
                ok: beats,
            },
        ],
    }
}

/// Seuil de rentabilité d'un trade à levier, en unités de marge (repère 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BreakEven {
    /// Gain net d'un trade gagnant, en marges.
    pub win_net: f64,
    /// Perte nette d'un trade perdant (liquidé : toute la marge, plus les frais d'entrée), en marges.
    pub loss_net: f64,
    /// Part de trades gagnants nécessaire pour ne rien perdre, avec les frais.
    pub breakeven_win_rate: f64,
    /// Part de trades gagnants attendue **sans aucun avantage** (marche aléatoire) :
    /// `distance de liquidation / (distance de liquidation + objectif)`, en prix.
    /// Quand le seuil de rentabilité est au-dessus, il faut un avantage réel.
    pub frictionless_win_rate: f64,
}

/// Espérance par trade, en marges, pour une part de gagnants `p_win`.
pub fn expectancy(p_win: f64, win_net: f64, loss_net: f64) -> f64 {
    p_win * win_net + (1.0 - p_win) * loss_net
}

/// Calcul d'équilibre : un trade à levier `leverage` qui gagne `target_pct` % du
/// prix, ou est liquidé à `liquidation_pct` % du prix (toute la marge perdue),
/// avec `fee_pct` % de frais par ordre (deux ordres pour un gagnant, un seul
/// pour un liquidé). À x100, +1,5 %, −0,5 % et 0,05 % : environ 43 % de
/// gagnants, contre 25 % sans frais.
pub fn breakeven(leverage: f64, target_pct: f64, liquidation_pct: f64, fee_pct: f64) -> BreakEven {
    let fee = fee_pct / 100.0 * leverage; // frais d'un ordre, en marges
    let win_net = leverage * target_pct / 100.0 - 2.0 * fee;
    let loss_net = -1.0 - fee;
    BreakEven {
        win_net,
        loss_net,
        breakeven_win_rate: -loss_net / (win_net - loss_net),
        frictionless_win_rate: liquidation_pct / (liquidation_pct + target_pct),
    }
}

/// Part des jours où le prix s'éloigne de l'ouverture d'au moins `distance` (repère 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Excursion {
    pub distance_pct: f64,
    pub up_pct: f64,
    pub down_pct: f64,
    pub both_pct: f64,
}

/// Mesure, sur des bougies journalières, la part des jours où le plus haut
/// dépasse l'ouverture de `distance` %, où le plus bas la passe sous, et où
/// les deux arrivent le même jour.
pub fn excursion_stats(days: &[Bar], distances_pct: &[f64]) -> Vec<Excursion> {
    let n = days.len().max(1) as f64;
    distances_pct
        .iter()
        .map(|&d| {
            let (mut up, mut down, mut both) = (0usize, 0usize, 0usize);
            for b in days {
                let hit_up = b.high >= b.open * (1.0 + d / 100.0);
                let hit_down = b.low <= b.open * (1.0 - d / 100.0);
                up += usize::from(hit_up);
                down += usize::from(hit_down);
                both += usize::from(hit_up && hit_down);
            }
            Excursion {
                distance_pct: d,
                up_pct: up as f64 / n * 100.0,
                down_pct: down as f64 / n * 100.0,
                both_pct: both as f64 / n * 100.0,
            }
        })
        .collect()
}

/// Repères relevés par l'utilisateur sur les bougies journalières BTC_USDT du
/// 01/05/2026 au 12/10/2026 (165 jours) : distance (%), hausse, baisse, les deux (%).
/// Ces distances correspondent environ à la liquidation à x100, x75, x50 et x20.
pub const EXCURSION_REFERENCE: [(f64, f64, f64, f64); 4] = [
    (0.5, 76.0, 69.0, 48.0),
    (0.83, 57.0, 58.0, 25.0),
    (1.5, 31.0, 38.0, 4.0),
    (4.5, 5.0, 2.0, 0.0),
];

/// Mesure et repère côte à côte.
#[derive(Debug, Clone, Serialize)]
pub struct ExcursionCheck {
    pub measured: Excursion,
    pub reference_up: f64,
    pub reference_down: f64,
    pub reference_both: f64,
    /// Écart maximal, en points de pourcentage, avec le repère.
    pub max_gap: f64,
}

pub fn check_excursions(days: &[Bar]) -> Vec<ExcursionCheck> {
    let distances: Vec<f64> = EXCURSION_REFERENCE.iter().map(|r| r.0).collect();
    excursion_stats(days, &distances)
        .into_iter()
        .zip(EXCURSION_REFERENCE)
        .map(|(m, (_, up, down, both))| ExcursionCheck {
            max_gap: (m.up_pct - up)
                .abs()
                .max((m.down_pct - down).abs())
                .max((m.both_pct - both).abs()),
            measured: m,
            reference_up: up,
            reference_down: down,
            reference_both: both,
        })
        .collect()
}

/// Les trades non expirés d'un résultat (exclut les soldes de fin de période).
pub fn closed_trades(r: &SimResult) -> Vec<&Trade> {
    r.trades
        .iter()
        .filter(|t| t.reason != ExitReason::Expiration)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ContractSpec, MaintenanceTier, Side};
    use crate::fibo::{FiboParams, ThresholdMode};
    use crate::leveraged::{Account, Costs, SimParams, TradeRules};
    use crate::market::{Interval, DAY_MS};

    fn trade(net: f64, r_net: f64, margin: f64) -> Trade {
        Trade {
            id: 0,
            setup_id: 0,
            ratio: 0.5,
            side: Side::Long,
            entry_time: 0,
            entry_price: 100.0,
            exit_time: 1,
            exit_price: 100.0,
            reason: ExitReason::Target,
            leverage: 10.0,
            margin,
            contracts: 1.0,
            notional: 100.0,
            stop: 90.0,
            target: 120.0,
            liquidation: None,
            a_price: 0.0,
            b_price: 0.0,
            gross: net,
            fee_entry: 0.0,
            fee_exit: 0.0,
            funding: 0.0,
            net,
            risk: 10.0,
            r_gross: r_net,
            r_net,
            ambiguous: false,
        }
    }

    #[test]
    fn rng_is_deterministic_and_uniform() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        let xs: Vec<f64> = (0..5).map(|_| a.next_f64()).collect();
        assert_eq!(xs, (0..5).map(|_| b.next_f64()).collect::<Vec<_>>());
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        let mut r = Rng::new(1);
        let mean = (0..20_000).map(|_| r.next_f64()).sum::<f64>() / 20_000.0;
        assert!((mean - 0.5).abs() < 0.01);
        assert!((0..1000).all(|_| r.below(7) < 7));
    }

    #[test]
    fn placebo_levels_are_deterministic_uniform_and_within_bounds() {
        let p = PlaceboParams::default();
        assert!(p.validate().is_ok());
        // Même graine : mêmes niveaux, à chaque exécution.
        assert_eq!(p.draw_levels(3, 4), p.draw_levels(3, 4));
        assert_ne!(p.draw_levels(3, 4), p.draw_levels(4, 4));
        let other_seed = PlaceboParams {
            seed: 1,
            ..PlaceboParams::default()
        };
        assert_ne!(p.draw_levels(3, 4), other_seed.draw_levels(3, 4));
        // Bornes [0,30 ; 0,80], niveaux distincts et triés.
        let mut all = Vec::new();
        for k in 0..200 {
            let l = p.draw_levels(k, 4);
            assert_eq!(l.len(), 4);
            assert!(l.windows(2).all(|w| w[0] < w[1]));
            assert!(l.iter().all(|r| (0.30..=0.80).contains(r)));
            all.extend(l);
        }
        let mean = all.iter().sum::<f64>() / all.len() as f64;
        assert!((mean - 0.55).abs() < 0.02, "moyenne {mean}");
        let low = all.iter().filter(|r| **r < 0.55).count() as f64 / all.len() as f64;
        assert!((low - 0.5).abs() < 0.05);
        let bad = [
            PlaceboParams {
                draws: 0,
                ..PlaceboParams::default()
            },
            PlaceboParams {
                low: 0.8,
                high: 0.3,
                ..PlaceboParams::default()
            },
            PlaceboParams {
                fixed_levels: vec![1.5],
                ..PlaceboParams::default()
            },
        ];
        assert!(bad.iter().all(|b| b.validate().is_err()));
    }

    #[test]
    fn quantiles_and_ranks() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile(&v, 0.0), Some(1.0));
        assert_eq!(quantile(&v, 0.5), Some(3.0));
        assert_eq!(quantile(&v, 1.0), Some(5.0));
        assert!((quantile(&v, 0.125).unwrap() - 1.5).abs() < 1e-12);
        assert_eq!(quantile(&[], 0.5), None);
        assert_eq!(percentile_rank(3.5, &v), Some(0.6));
        assert_eq!(
            percentile_rank(1.0, &v),
            Some(0.0),
            "égalité : pas meilleur"
        );
        assert_eq!(percentile_rank(9.0, &v), Some(1.0));
        assert_eq!(percentile_rank(1.0, &[]), None);
    }

    #[test]
    fn monte_carlo_is_deterministic_and_reads_the_sequence() {
        // 6 trades : +20 ×3, −10 ×3 (série de 3 pertes), marge 10.
        let trades: Vec<Trade> = [20.0, -10.0, -10.0, -10.0, 20.0, 20.0]
            .iter()
            .map(|n| trade(*n, n / 10.0, 10.0))
            .collect();
        let a = monte_carlo(&trades, 100.0, 10_000, 5).unwrap();
        let b = monte_carlo(&trades, 100.0, 10_000, 5).unwrap();
        assert_eq!(a.final_mean, b.final_mean);
        assert_eq!(a.final_quantiles, b.final_quantiles);
        assert_eq!(a.worst_streak_observed, 3);
        assert_eq!(a.trades_per_draw, 6);
        // Espérance par trade : +5 → budget moyen final proche de 130.
        assert!((a.final_mean - 130.0).abs() < 2.0, "{}", a.final_mean);
        assert!(a.final_quantiles.windows(2).all(|w| w[0] <= w[1]));
        assert!(a.prob_ruin < 0.01);
        assert!(a.prob_loss_50 < a.prob_ruin + 0.05);
        assert!(a.worst_streak_p95 >= a.worst_streak_median);
        assert_eq!(a.histogram.iter().map(|h| h.1).sum::<usize>(), 10_000);
        // Une série qui perd toujours : ruine certaine, budget final nul.
        let losing: Vec<Trade> = (0..20).map(|_| trade(-30.0, -3.0, 10.0)).collect();
        let m = monte_carlo(&losing, 100.0, 1000, 1).unwrap();
        assert_eq!(m.prob_ruin, 1.0);
        assert_eq!(m.prob_loss_50, 1.0);
        assert_eq!(m.final_quantiles[2], 0.0);
        assert!(monte_carlo(&[], 100.0, 10, 1).is_none());
    }

    #[test]
    fn verdict_requires_all_three_criteria_on_the_test_period() {
        let base = RunSummary {
            trades: 250,
            expectancy_r_net: Some(0.1),
            ..RunSummary::default()
        };
        let v = verdict(&base, Some(0.97));
        assert_eq!(v.verdict, Verdict::Demonstrated);
        assert!(v.criteria.iter().all(|c| c.ok));
        // Rang insuffisant.
        assert_eq!(verdict(&base, Some(0.90)).verdict, Verdict::NotDemonstrated);
        assert_eq!(
            verdict(&base, Some(0.95)).verdict,
            Verdict::Demonstrated,
            "95 % inclus"
        );
        assert_eq!(verdict(&base, None).verdict, Verdict::NotDemonstrated);
        // Espérance nulle ou négative.
        let neg = RunSummary {
            expectancy_r_net: Some(-0.01),
            ..base.clone()
        };
        assert_eq!(verdict(&neg, Some(0.99)).verdict, Verdict::NotDemonstrated);
        let zero = RunSummary {
            expectancy_r_net: Some(0.0),
            ..base.clone()
        };
        assert_eq!(verdict(&zero, Some(0.99)).verdict, Verdict::NotDemonstrated);
        // Pas assez de trades, même avec un excellent résultat.
        let few = RunSummary {
            trades: 199,
            ..base.clone()
        };
        let v = verdict(&few, Some(0.99));
        assert_eq!(v.verdict, Verdict::NotEnoughTrades);
        assert_eq!(v.label, "Pas assez de trades");
        assert!(!v.criteria[0].ok);
        assert_eq!(
            verdict(
                &RunSummary {
                    trades: 200,
                    ..base
                },
                Some(0.99)
            )
            .verdict,
            Verdict::Demonstrated
        );
        // Libellés.
        assert_eq!(
            serde_json::to_value(Verdict::NotDemonstrated).unwrap(),
            "Avantage non démontré"
        );
    }

    #[test]
    fn breakeven_at_x100_matches_the_reference_calculation() {
        // Repère 7.3 : x100, objectif +1,5 %, liquidation −0,5 %, 0,05 % de frais par ordre.
        let b = breakeven(100.0, 1.5, 0.5, 0.05);
        assert!((b.win_net - 1.4).abs() < 1e-12, "{}", b.win_net);
        assert!((b.loss_net + 1.05).abs() < 1e-12);
        assert!((b.breakeven_win_rate - 1.05 / 2.45).abs() < 1e-12);
        assert!(
            (b.breakeven_win_rate * 100.0 - 43.0).abs() < 0.5,
            "environ 43 %"
        );
        assert!(
            (b.frictionless_win_rate - 0.25).abs() < 1e-12,
            "25 % sans avantage"
        );
        // À l'équilibre, l'espérance est nulle ; au-dessus, positive.
        assert!(expectancy(b.breakeven_win_rate, b.win_net, b.loss_net).abs() < 1e-12);
        assert!(expectancy(0.50, b.win_net, b.loss_net) > 0.0);
        assert!(expectancy(0.25, b.win_net, b.loss_net) < 0.0);
        // Sans frais : le seuil reste à 40 % (1 marge perdue contre 1,5 gagnée), car la
        // liquidation coûte toute la marge, soit plus que les 0,5 % de variation de prix ;
        // la chance d'atteindre +1,5 % avant −0,5 % sans avantage est, elle, de 25 %.
        let free = breakeven(100.0, 1.5, 0.5, 0.0);
        assert!((free.breakeven_win_rate - 0.4).abs() < 1e-12);
        assert!((free.frictionless_win_rate - 0.25).abs() < 1e-12);
    }

    fn day(open: f64, high: f64, low: f64) -> Bar {
        Bar {
            time: 0,
            open,
            high,
            low,
            close: open,
            volume: 0.0,
        }
    }

    #[test]
    fn excursions_count_days_by_distance() {
        // 4 jours à partir d'une ouverture de 100.
        let days = [
            day(100.0, 100.4, 99.6),  // ±0,4 %
            day(100.0, 100.6, 100.0), // +0,6 % seulement
            day(100.0, 100.0, 99.4),  // −0,6 % seulement
            day(100.0, 101.0, 98.9),  // des deux côtés
        ];
        let s = excursion_stats(&days, &[0.5, 1.0]);
        assert_eq!(
            (s[0].up_pct, s[0].down_pct, s[0].both_pct),
            (50.0, 50.0, 25.0)
        );
        // 1 % : seul le dernier jour monte à +1 % ; il baisse de 1,1 %.
        assert_eq!(
            (s[1].up_pct, s[1].down_pct, s[1].both_pct),
            (25.0, 25.0, 25.0)
        );
        let checks = check_excursions(&days);
        assert_eq!(checks.len(), 4);
        assert_eq!(checks[0].measured.distance_pct, 0.5);
        assert_eq!(checks[0].reference_up, 76.0);
        // Les repères eux-mêmes (hausse, baisse, les deux) sont cohérents.
        for r in EXCURSION_REFERENCE {
            assert!(r.3 <= r.1.min(r.2), "les deux côtés ≤ chaque côté");
        }
    }

    fn spec() -> ContractSpec {
        ContractSpec {
            symbol: "T".into(),
            max_leverage: 75.0,
            contract_value: 1.0,
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

    #[test]
    fn placebo_runs_are_deterministic_whatever_the_thread_count() {
        // Marche aléatoire courte : le moteur tourne vraiment à chaque tirage.
        let mut state = 99u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as f64 / u64::MAX as f64 - 0.5
        };
        let t0 = 20_000 * DAY_MS;
        let mut price: f64 = 100.0;
        let bars: Vec<Bar> = (0..120_000)
            .map(|i| {
                let open = price;
                let close = open * (1.0 + 0.0004 * (next() + next() + next()));
                price = close;
                Bar {
                    time: t0 + i as i64 * 60_000,
                    open,
                    high: open.max(close),
                    low: open.min(close),
                    close,
                    volume: 0.0,
                }
            })
            .collect();
        let params = SimParams {
            fibo: FiboParams {
                interval: Interval::M15,
                threshold_mode: ThresholdMode::Percent,
                threshold_pct: 1.0,
                ..FiboParams::default()
            },
            rules: TradeRules::default(),
            costs: Costs::default(),
            account: Account {
                initial_budget: 1e6,
                margin: 100.0,
                leverage: 2.0,
                r_only: false,
            },
            start_ms: t0,
            end_ms: t0 + 1000 * DAY_MS,
        };
        let spec = spec();
        let input = SimInput {
            bars: &bars,
            mark: None,
            funding: &[],
            spec: &spec,
        };
        let placebo = PlaceboParams {
            draws: 12,
            ..PlaceboParams::default()
        };
        let cancel = AtomicBool::new(false);
        let noop = |_: usize, _: usize| {};
        let fib = summarize(&input.run(&params));
        let a = run_placebo(
            &input,
            &params,
            &placebo,
            fib.expectancy_r_net,
            &noop,
            &cancel,
        );
        let b = run_placebo(
            &input,
            &params,
            &placebo,
            fib.expectancy_r_net,
            &noop,
            &cancel,
        );
        assert_eq!(a.draws.len(), 12);
        let key = |r: &PlaceboResult| -> Vec<(Vec<f64>, usize, Option<f64>)> {
            r.draws
                .iter()
                .map(|d| (d.levels.clone(), d.trades, d.expectancy_r_net))
                .collect()
        };
        assert_eq!(
            key(&a),
            key(&b),
            "graine fixe : mêmes tirages et mêmes résultats"
        );
        assert!(a.draws.iter().any(|d| d.trades > 0), "le moteur a joué");
        assert!(a.percentile.is_some() && a.valid_draws > 0);
        // Chaque tirage est reproductible à part, avec ses niveaux.
        let mut p = params.clone();
        p.fibo.ratios = a.draws[5].levels.clone();
        let again = summarize(&input.run(&p));
        assert_eq!(again.expectancy_r_net, a.draws[5].expectancy_r_net);
        // Annulation : aucun tirage ne démarre.
        cancel.store(true, Ordering::Relaxed);
        let stopped = run_placebo(&input, &params, &placebo, None, &noop, &cancel);
        assert!(stopped.draws.is_empty());
    }

    #[test]
    fn per_level_groups_the_trades() {
        let mut t = vec![
            trade(10.0, 1.0, 10.0),
            trade(-10.0, -1.0, 10.0),
            trade(10.0, 2.0, 10.0),
        ];
        t[0].ratio = 0.382;
        t[1].ratio = 0.618;
        t[2].ratio = 0.618;
        let d = per_level(&t);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].ratio, d[0].trades), (0.382, 1));
        assert_eq!(d[1].trades, 2);
        assert_eq!(d[1].win_rate_pct, Some(50.0));
        assert_eq!(d[1].expectancy_r_net, Some(0.5));
    }

    #[test]
    fn summary_reports_costs_over_margin() {
        let mut t = trade(-1.0, -0.1, 10.0);
        t.fee_entry = 0.2;
        t.fee_exit = 0.3;
        t.funding = 0.1;
        let r = SimResult {
            trades: vec![t, trade(5.0, 0.5, 10.0)],
            stats: Default::default(),
            ruined: false,
            ruin_time: None,
            initial_budget: 100.0,
            final_budget: 104.0,
            budget_curve: vec![(0, 100.0), (1, 99.0), (2, 104.0)],
            bars_in_period: 10,
        };
        let s = summarize(&r);
        assert_eq!((s.trades, s.wins), (2, 1));
        assert!((s.fees - 0.5).abs() < 1e-12);
        assert!(
            (s.fees_pct_of_margin.unwrap() - 2.5).abs() < 1e-12,
            "0,5 / 20 marges"
        );
        assert!((s.funding_pct_of_margin.unwrap() - 0.5).abs() < 1e-12);
        assert!((s.max_drawdown_pct + 1.0).abs() < 1e-9);
        assert_eq!(s.win_rate_pct, Some(50.0));
        assert!((s.expectancy_usdt.unwrap() - 2.0).abs() < 1e-12);
    }
}

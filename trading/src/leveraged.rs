//! Simulation, bougie 1 minute par bougie 1 minute, des trades à levier pris sur
//! les niveaux de Fibonacci de [`crate::fibo`].
//!
//! **Chronologie.** Les signaux sont calculés sur l'unité de détection et ne
//! sont connus qu'à la clôture de leur bougie : une mise en place créée à la
//! clôture de la bougie de détection `d` n'agit qu'à partir de la première
//! bougie 1 min qui suit cette clôture.
//!
//! **Ordres.** Une mise en place pose des ordres sur ses niveaux ; le premier
//! niveau touché ouvre le trade (un trade par mouvement, une seule position à la
//! fois). En continuation, l'entrée est un ordre limite (frais maker, exécuté
//! au niveau) ; en sens contraire, c'est un ordre stop sur le niveau (frais
//! taker, glissement). Stop et sortie « au marché » : frais taker et glissement ;
//! objectif : ordre limite (frais maker). Un ordre limite n'est jamais exécuté
//! à un meilleur prix que le sien.
//!
//! **Bougie ambiguë.** À l'intérieur d'une bougie 1 min, l'ordre des prix est
//! inconnu. Si le stop (ou la liquidation) et l'objectif sont touchés dans la
//! même bougie, on compte la sortie défavorable ; l'objectif n'est jamais évalué
//! dans la bougie d'entrée ; le nombre de ces cas est publié.
//!
//! **Risque.** `R` est la perte prévue au stop : quantité × distance entrée-stop,
//! hors frais. Les résultats en R ne dépendent donc pas du levier : celui-ci
//! fixe seulement la taille de la position (en contrats entiers) et le prix de
//! liquidation.

use crate::contract::{liquidation_price, ContractSpec, Fees, Side};
use crate::fibo::{Aggregator, Detector, Direction, FiboParams, Setup, LADDER};
use crate::market::{Bar, FundingRate, FUNDING_PERIOD_MS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopRule {
    /// Au-delà du niveau suivant (entrée à 61,8 % : stop à 78,6 %), plus une marge en %.
    NextLevel { margin_pct: f64 },
    /// Au-delà de l'origine A, plus une marge en %.
    Origin { margin_pct: f64 },
    /// Distance fixe en % du prix d'entrée.
    Fixed { pct: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetRule {
    /// Retour au point B (0 %).
    ReturnToB,
    /// Extension du mouvement A→B (1,272 ou 1,618).
    Extension { ratio: f64 },
    /// Multiple fixe du risque.
    RMultiple { r: f64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TradeRules {
    pub stop: StopRule,
    pub target: TargetRule,
    /// Il faut traverser le niveau (et l'objectif) d'un tick pour être exécuté.
    pub prudent_tick: bool,
    /// Si la liquidation tombe avant le stop : vrai, la liquidation devient le
    /// stop ; faux, le trade est refusé et compté.
    pub liquidation_as_stop: bool,
    /// Durée maximale d'un trade, en bougies de détection (0 : aucune).
    pub max_hold_bars: usize,
}

impl Default for TradeRules {
    fn default() -> Self {
        Self {
            stop: StopRule::NextLevel { margin_pct: 0.0 },
            target: TargetRule::ReturnToB,
            prudent_tick: false,
            liquidation_as_stop: false,
            max_hold_bars: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Costs {
    pub fees: Fees,
    /// Glissement des sorties au marché et des entrées stop, en % du prix.
    pub slippage_pct: f64,
    /// Funding supposé, en % par 8 h, quand l'historique réel manque.
    pub funding_pct_8h: f64,
    /// Marge de sécurité sur la liquidation, en % du prix, quand il n'y a pas
    /// d'historique du prix de marque : la liquidation est déclenchée plus tôt.
    pub liquidation_safety_pct: f64,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            fees: crate::contract::DEFAULT_FEES,
            slippage_pct: 0.01,
            funding_pct_8h: 0.01,
            liquidation_safety_pct: 0.1,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Account {
    /// Budget de trading initial, en USDT (séparé du stock de BTC).
    pub initial_budget: f64,
    /// Marge isolée par trade, en USDT.
    pub margin: f64,
    pub leverage: f64,
    /// Mode « R seulement » : levier 1, quantité fractionnaire, ni arrondi aux
    /// contrats, ni liquidation, ni ruine. Sert à montrer que le levier ne
    /// change pas l'avantage mesuré en R.
    #[serde(default)]
    pub r_only: bool,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            initial_budget: 500.0,
            margin: 10.0,
            leverage: 20.0,
            r_only: false,
        }
    }
}

/// Paramètres complets d'une simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimParams {
    pub fibo: FiboParams,
    pub rules: TradeRules,
    pub costs: Costs,
    pub account: Account,
    /// Début de la période jugée (ms, UTC) : avant, les bougies ne servent qu'à
    /// préchauffer l'identification des points.
    pub start_ms: i64,
    /// Fin exclue (ms, UTC).
    pub end_ms: i64,
}

impl SimParams {
    pub fn validate(&self, spec: &ContractSpec) -> Result<(), String> {
        self.fibo.validate()?;
        let check = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        check(self.start_ms < self.end_ms, "La période est vide.")?;
        let a = &self.account;
        check(
            a.initial_budget.is_finite() && a.initial_budget > 0.0,
            "Le budget initial doit être positif.",
        )?;
        check(
            a.margin.is_finite() && a.margin > 0.0,
            "La marge par trade doit être positive.",
        )?;
        if !a.r_only {
            spec.check_leverage(a.leverage)?;
        }
        let c = &self.costs;
        check(
            (0.0..=1.0).contains(&c.fees.maker_pct) && (0.0..=1.0).contains(&c.fees.taker_pct),
            "Les frais doivent être compris entre 0 et 1 %.",
        )?;
        check(
            (0.0..=2.0).contains(&c.slippage_pct),
            "Le glissement doit être compris entre 0 et 2 %.",
        )?;
        check(
            (-1.0..=1.0).contains(&c.funding_pct_8h),
            "Le funding supposé doit être compris entre −1 et 1 % par 8 h.",
        )?;
        check(
            (0.0..=5.0).contains(&c.liquidation_safety_pct),
            "La marge de sécurité de liquidation doit être comprise entre 0 et 5 %.",
        )?;
        match self.rules.stop {
            StopRule::NextLevel { margin_pct } | StopRule::Origin { margin_pct } => check(
                (0.0..=50.0).contains(&margin_pct),
                "La marge du stop doit être comprise entre 0 et 50 %.",
            )?,
            StopRule::Fixed { pct } => check(
                pct > 0.0 && pct <= 50.0,
                "La distance du stop doit être comprise entre 0 et 50 %.",
            )?,
        }
        match self.rules.target {
            TargetRule::Extension { ratio } => check(
                ratio > 1.0 && ratio <= 5.0,
                "L'extension doit être comprise entre 1 et 5.",
            )?,
            TargetRule::RMultiple { r } => check(
                r > 0.0 && r <= 50.0,
                "Le multiple de risque doit être compris entre 0 et 50.",
            )?,
            TargetRule::ReturnToB => {}
        }
        check(
            self.rules.max_hold_bars <= 100_000,
            "La durée maximale d'un trade est trop grande.",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExitReason {
    #[serde(rename = "Objectif")]
    Target,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "Liquidation")]
    Liquidation,
    /// Durée maximale atteinte, ou fin de la période.
    #[serde(rename = "Expiration")]
    Expiration,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub id: usize,
    /// Mise en place qui a posé l'ordre.
    pub setup_id: usize,
    /// Niveau de retracement utilisé.
    pub ratio: f64,
    pub side: Side,
    pub entry_time: i64,
    pub entry_price: f64,
    pub exit_time: i64,
    pub exit_price: f64,
    pub reason: ExitReason,
    pub leverage: f64,
    /// Marge isolée engagée (contrats entiers : un peu moins que la marge demandée).
    pub margin: f64,
    pub contracts: f64,
    pub notional: f64,
    pub stop: f64,
    pub target: f64,
    pub liquidation: Option<f64>,
    /// Origine A et point B du mouvement de référence.
    pub a_price: f64,
    pub b_price: f64,
    /// Résultat brut, en USDT.
    pub gross: f64,
    pub fee_entry: f64,
    pub fee_exit: f64,
    pub funding: f64,
    /// Résultat net (brut − frais − funding), en USDT.
    pub net: f64,
    /// R : perte prévue au stop, en USDT (quantité × distance entrée-stop, hors frais).
    pub risk: f64,
    pub r_gross: f64,
    pub r_net: f64,
    /// Stop (ou liquidation) et objectif touchés dans la même bougie 1 min.
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SimStats {
    /// Mises en place créées (un mouvement de référence chacune).
    pub setups: usize,
    /// Mises en place où des ordres ont été posés.
    pub placed: usize,
    /// Niveaux ignorés car déjà dépassés à la confirmation.
    pub levels_ignored: usize,
    /// Mises en place non posées : position déjà ouverte.
    pub skipped_busy: usize,
    /// Mises en place sans ordre déclenché (annulées, expirées).
    pub unfilled: usize,
    /// Trades refusés : moins d'un contrat.
    pub refused_min_size: usize,
    /// Trades refusés : la liquidation tombe avant le stop.
    pub refused_liquidation: usize,
    /// Trades refusés pour une autre raison (stop ou objectif incohérent, levier trop élevé).
    pub refused_other: usize,
    pub liquidations: usize,
    /// Bougies 1 min où stop (ou liquidation) et objectif sont touchés ensemble.
    pub ambiguous: usize,
    /// Bougies sans prix de marque (repli sur le dernier prix).
    pub mark_missing: usize,
}

impl SimStats {
    pub fn refused(&self) -> usize {
        self.refused_min_size + self.refused_liquidation + self.refused_other
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SimResult {
    pub trades: Vec<Trade>,
    pub stats: SimStats,
    /// La marge n'est plus finançable : le budget est ruiné.
    pub ruined: bool,
    pub ruin_time: Option<i64>,
    pub initial_budget: f64,
    pub final_budget: f64,
    /// Budget après chaque trade : `(instant de sortie, budget)`.
    pub budget_curve: Vec<(i64, f64)>,
    /// Bougies 1 min parcourues dans la période jugée.
    pub bars_in_period: usize,
}

struct Armed {
    setup: Setup,
    expires_time: i64,
}

struct Position {
    id: usize,
    setup_id: usize,
    ratio: f64,
    side: Side,
    entry_time: i64,
    entry_price: f64,
    contracts: f64,
    qty: f64,
    margin: f64,
    stop: f64,
    /// Le stop est la liquidation elle-même (option « la liquidation devient le stop »).
    stop_is_liquidation: bool,
    target: f64,
    liq_price: Option<f64>,
    /// Niveau où la liquidation est déclenchée (avec la marge de sécurité).
    liq_trigger: Option<f64>,
    a_price: f64,
    b_price: f64,
    fee_entry: f64,
    funding: f64,
    risk: f64,
    ambiguous: bool,
}

/// Prix du stop selon la règle choisie, ou `None` s'il ne protège pas l'entrée.
fn stop_price(
    rule: StopRule,
    setup: &Setup,
    ratio: f64,
    entry: f64,
    side: Side,
    contrary: bool,
) -> Option<f64> {
    let (level, margin) = match rule {
        StopRule::Fixed { pct } => (entry, pct / 100.0),
        StopRule::NextLevel { margin_pct } => {
            // En continuation : le niveau suivant, plus profond. En sens contraire
            // (structure en miroir) : le niveau précédent, plus proche de B.
            let r = if contrary {
                LADDER
                    .iter()
                    .copied()
                    .rev()
                    .find(|r| *r < ratio - 1e-9)
                    .unwrap_or(0.0)
            } else {
                Setup::next_ratio(ratio)
            };
            (setup.price_at(r), margin_pct / 100.0)
        }
        StopRule::Origin { margin_pct } => {
            // En sens contraire, le miroir de l'origine A est le point B.
            let p = if contrary {
                setup.b.price
            } else {
                setup.a.price
            };
            (p, margin_pct / 100.0)
        }
    };
    let stop = level * (1.0 - side.sign() * margin);
    // Le stop doit être du côté de la perte.
    (side.sign() * (entry - stop) > 0.0).then_some(stop)
}

fn target_price(
    rule: TargetRule,
    setup: &Setup,
    entry: f64,
    stop: f64,
    side: Side,
    contrary: bool,
) -> Option<f64> {
    let target = match rule {
        TargetRule::ReturnToB => setup.price_at(if contrary { 1.0 } else { 0.0 }),
        TargetRule::Extension { ratio } => {
            setup.price_at(if contrary { ratio } else { 1.0 - ratio })
        }
        TargetRule::RMultiple { r } => entry + side.sign() * r * (entry - stop).abs(),
    };
    (side.sign() * (target - entry) > 0.0).then_some(target)
}

/// Constantes d'une simulation.
struct Ctx<'a> {
    spec: &'a ContractSpec,
    rules: TradeRules,
    acc: Account,
    contrary: bool,
    maker: f64,
    taker: f64,
    slip: f64,
    safety: f64,
    tick: f64,
}

enum Refusal {
    /// Moins d'un contrat.
    MinSize,
    /// La liquidation tombe avant le stop.
    LiquidationFirst,
    /// Stop, objectif ou levier incohérent.
    Invalid,
    /// La marge n'est plus finançable.
    Ruin,
}

impl Ctx<'_> {
    /// Ouvre la position déclenchée par `level` dans la bougie `bar`.
    fn open(
        &self,
        setup: &Setup,
        level: crate::fibo::Level,
        bar: &Bar,
        cash: f64,
        id: usize,
    ) -> Result<Position, Refusal> {
        let side = setup.side;
        let level_price = self.spec.round_price(level.price);
        // Continuation : ordre limite au niveau. Sens contraire : ordre stop sur
        // le niveau, exécuté au marché (écart d'ouverture et glissement défavorables).
        let (entry, fee_rate) = if self.contrary {
            let base = if setup.up {
                bar.open.min(level_price)
            } else {
                bar.open.max(level_price)
            };
            (base * (1.0 + side.sign() * self.slip), self.taker)
        } else {
            (level_price, self.maker)
        };
        let (contracts, qty, margin) = if self.acc.r_only {
            let q = self.acc.margin / entry;
            (q / self.spec.contract_value, q, self.acc.margin)
        } else {
            let n = self
                .spec
                .contracts_for(self.acc.margin, self.acc.leverage, entry)
                .ok_or(Refusal::MinSize)?;
            let q = self.spec.quantity(n);
            (n, q, q * entry / self.acc.leverage)
        };
        let mut stop = stop_price(
            self.rules.stop,
            setup,
            level.ratio,
            entry,
            side,
            self.contrary,
        )
        .ok_or(Refusal::Invalid)?;
        let liq = if self.acc.r_only {
            None
        } else {
            Some(
                liquidation_price(
                    side,
                    entry,
                    self.acc.leverage,
                    self.spec.maintenance_rate(contracts),
                )
                .ok_or(Refusal::Invalid)?,
            )
        };
        let liq_trigger = liq.map(|l| l * (1.0 + side.sign() * self.safety));
        let mut stop_is_liquidation = false;
        if let Some(lt) = liq_trigger {
            // La liquidation tombe avant le stop (ou en même temps).
            if side.sign() * (stop - lt) <= 0.0 {
                if !self.rules.liquidation_as_stop {
                    return Err(Refusal::LiquidationFirst);
                }
                stop = lt;
                stop_is_liquidation = true;
            }
        }
        let target = target_price(self.rules.target, setup, entry, stop, side, self.contrary)
            .ok_or(Refusal::Invalid)?;
        let fee_entry = qty * entry * fee_rate;
        if !self.acc.r_only && cash < margin + fee_entry {
            return Err(Refusal::Ruin);
        }
        Ok(Position {
            id,
            setup_id: setup.id,
            ratio: level.ratio,
            side,
            entry_time: bar.time,
            entry_price: entry,
            contracts,
            qty,
            margin,
            stop,
            stop_is_liquidation,
            target,
            liq_price: liq,
            liq_trigger,
            a_price: setup.a.price,
            b_price: setup.b.price,
            fee_entry,
            funding: 0.0,
            risk: qty * (entry - stop).abs(),
            ambiguous: false,
        })
    }

    /// Sortie de la position dans la bougie `bar`, si elle a lieu : liquidation,
    /// stop, objectif ou durée maximale. Renvoie le motif, le prix et si
    /// l'objectif a aussi été touché dans la même bougie.
    ///
    /// `entered_now` : la position vient de s'ouvrir dans cette bougie ; son
    /// objectif n'est pas évalué (ordre des prix inconnu), mais les extrêmes
    /// adverses comptent (hypothèse prudente).
    fn exit(
        &self,
        pos: &Position,
        bar: &Bar,
        liq_range: (f64, f64),
        entered_now: bool,
        held_too_long: bool,
    ) -> Option<(ExitReason, f64, bool)> {
        let sign = pos.side.sign();
        // `price` est-il au niveau de `level` ou au-delà, du côté de la perte ?
        let beyond = |price: f64, level: f64| sign * (level - price) >= 0.0;
        let (adverse, favorable) = if sign > 0.0 {
            (bar.low, bar.high)
        } else {
            (bar.high, bar.low)
        };
        let adverse_liq = if sign > 0.0 { liq_range.0 } else { liq_range.1 };
        let target_hit = !entered_now
            && if sign > 0.0 {
                favorable >= pos.target + self.tick
            } else {
                favorable <= pos.target - self.tick
            };
        let liq_price = pos.liq_price.unwrap_or(pos.stop);
        let liquidated = pos.liq_trigger.is_some_and(|lt| beyond(adverse_liq, lt));
        let gap_liquidated = pos.liq_trigger.is_some_and(|lt| beyond(bar.open, lt));
        let stopped = !pos.stop_is_liquidation && beyond(adverse, pos.stop);
        let outcome = if gap_liquidated || (pos.stop_is_liquidation && liquidated) {
            Some((ExitReason::Liquidation, liq_price))
        } else if stopped {
            // Écart d'ouverture : exécution à l'ouverture si elle franchit déjà le stop.
            let raw = if beyond(bar.open, pos.stop) {
                bar.open
            } else {
                pos.stop
            };
            Some((ExitReason::Stop, raw * (1.0 - sign * self.slip)))
        } else if liquidated {
            Some((ExitReason::Liquidation, liq_price))
        } else {
            None
        };
        match outcome {
            Some((reason, price)) => Some((reason, price, target_hit)),
            None if target_hit => Some((ExitReason::Target, pos.target, false)),
            None if held_too_long => Some((
                ExitReason::Expiration,
                bar.open * (1.0 - sign * self.slip),
                false,
            )),
            None => None,
        }
    }
}

/// Données d'une simulation : bougies 1 min, prix de marque, funding, contrat.
#[derive(Clone, Copy)]
pub struct SimInput<'a> {
    pub bars: &'a [Bar],
    pub mark: Option<&'a [Bar]>,
    pub funding: &'a [FundingRate],
    pub spec: &'a ContractSpec,
}

impl SimInput<'_> {
    pub fn run(&self, p: &SimParams) -> SimResult {
        simulate(self.bars, self.mark, self.funding, self.spec, p)
    }
}

/// Joue la simulation sur les bougies 1 min `bars` (triées, préchauffage
/// compris, bougie en cours exclue).
///
/// * `mark` : bougies 1 min du prix de marque, si l'historique existe ; sinon
///   les liquidations se lisent sur le dernier prix avec la marge de sécurité.
/// * `funding` : historique réel du funding ; au-delà, le taux supposé.
pub fn simulate(
    bars: &[Bar],
    mark: Option<&[Bar]>,
    funding: &[FundingRate],
    spec: &ContractSpec,
    p: &SimParams,
) -> SimResult {
    let ctx = Ctx {
        spec,
        rules: p.rules,
        acc: p.account,
        contrary: p.fibo.direction == Direction::Contrary,
        maker: p.costs.fees.maker_pct / 100.0,
        taker: p.costs.fees.taker_pct / 100.0,
        slip: p.costs.slippage_pct / 100.0,
        safety: p.costs.liquidation_safety_pct / 100.0,
        tick: if p.rules.prudent_tick {
            spec.tick_size
        } else {
            0.0
        },
    };
    let interval_ms = p.fibo.interval.millis();
    let assumed = p.costs.funding_pct_8h / 100.0;
    let real: HashMap<i64, f64> = funding
        .iter()
        .map(|f| {
            (
                (f.time as f64 / FUNDING_PERIOD_MS as f64).round() as i64,
                f.rate,
            )
        })
        .collect();

    let mut detector = Detector::new(p.fibo.clone());
    let mut agg = Aggregator::new(p.fibo.interval);
    let mut stats = SimStats::default();
    let mut trades: Vec<Trade> = Vec::new();
    let mut cash = p.account.initial_budget;
    let mut curve = vec![(p.start_ms, cash)];
    let mut armed: Option<Armed> = None;
    let mut pos: Option<Position> = None;
    let (mut ruined, mut ruin_time) = (false, None);
    let mut mark_ix = 0usize;
    let mut bars_in_period = 0usize;
    let mut next_id = 0usize;
    let mut last_in_period: Option<&Bar> = None;

    // Clôture d'une position : crée le trade et met le budget à jour.
    let close = |pos: Position,
                 time: i64,
                 price: f64,
                 reason: ExitReason,
                 fee_rate: f64,
                 cash: &mut f64,
                 trades: &mut Vec<Trade>,
                 curve: &mut Vec<(i64, f64)>| {
        let liquidated = reason == ExitReason::Liquidation;
        let gross = if liquidated {
            // Toute la marge isolée est perdue.
            -pos.margin
        } else {
            pos.side.sign() * pos.qty * (price - pos.entry_price)
        };
        let fee_exit = if liquidated {
            0.0
        } else {
            pos.qty * price * fee_rate
        };
        let net = gross - pos.fee_entry - fee_exit - pos.funding;
        *cash += gross - fee_exit;
        curve.push((time, *cash));
        trades.push(Trade {
            id: pos.id,
            setup_id: pos.setup_id,
            ratio: pos.ratio,
            side: pos.side,
            entry_time: pos.entry_time,
            entry_price: pos.entry_price,
            exit_time: time,
            exit_price: price,
            reason,
            leverage: if ctx.acc.r_only {
                1.0
            } else {
                ctx.acc.leverage
            },
            margin: pos.margin,
            contracts: pos.contracts,
            notional: pos.qty * pos.entry_price,
            stop: pos.stop,
            target: pos.target,
            liquidation: pos.liq_price,
            a_price: pos.a_price,
            b_price: pos.b_price,
            gross,
            fee_entry: pos.fee_entry,
            fee_exit,
            funding: pos.funding,
            net,
            risk: pos.risk,
            r_gross: if pos.risk > 0.0 {
                gross / pos.risk
            } else {
                0.0
            },
            r_net: if pos.risk > 0.0 { net / pos.risk } else { 0.0 },
            ambiguous: pos.ambiguous,
        });
    };

    for bar in bars {
        if bar.time >= p.end_ms {
            break;
        }
        let in_period = bar.time >= p.start_ms;

        // 0. Bougie de détection close : l'identification n'agit qu'à partir de cette bougie 1 min.
        if let Some(det) = agg.push(bar) {
            if let Some(conf) = detector.push(det) {
                // Un nouveau pivot annule la mise en place précédente.
                if armed.take().is_some() {
                    stats.unfilled += 1;
                }
                if let Some(setup) = conf.setup.filter(|_| in_period) {
                    stats.setups += 1;
                    stats.levels_ignored += setup.ignored.len();
                    if !setup.levels.is_empty() {
                        if pos.is_some() {
                            stats.skipped_busy += 1;
                        } else if !ruined {
                            stats.placed += 1;
                            let expires_time =
                                setup.created_time() + p.fibo.max_age_bars as i64 * interval_ms;
                            armed = Some(Armed {
                                setup,
                                expires_time,
                            });
                        }
                    }
                }
            }
        }
        if in_period {
            bars_in_period += 1;
            last_in_period = Some(bar);
        }
        if ruined {
            continue;
        }

        // 1. Funding : toutes les 8 h, sur le montant engagé, si la position existait avant l'instant.
        if let Some(pos) = pos.as_mut() {
            if bar.time.rem_euclid(FUNDING_PERIOD_MS) == 0 && pos.entry_time < bar.time {
                let rate = real
                    .get(&(bar.time / FUNDING_PERIOD_MS))
                    .copied()
                    .unwrap_or(assumed);
                let payment = pos.side.sign() * pos.qty * bar.open * rate;
                pos.funding += payment;
                cash -= payment;
            }
        }

        // 2. Ordres en attente : expiration, exécution, annulation.
        let mut entered_now = false;
        if pos.is_none() {
            if let Some(a) = armed.take() {
                if bar.time >= a.expires_time {
                    stats.unfilled += 1;
                } else {
                    // Premier niveau rencontré par le prix : le plus proche de B.
                    let hit = a
                        .setup
                        .levels
                        .iter()
                        .find(|l| a.setup.reached(l.price, bar.low, bar.high, ctx.tick))
                        .copied();
                    match hit {
                        None if a.setup.beyond_origin(bar.low, bar.high) => stats.unfilled += 1,
                        None => armed = Some(a),
                        Some(level) => match ctx.open(&a.setup, level, bar, cash, next_id) {
                            Ok(position) => {
                                cash -= position.fee_entry;
                                pos = Some(position);
                                next_id += 1;
                                entered_now = true;
                            }
                            Err(Refusal::MinSize) => stats.refused_min_size += 1,
                            Err(Refusal::LiquidationFirst) => stats.refused_liquidation += 1,
                            Err(Refusal::Invalid) => stats.refused_other += 1,
                            Err(Refusal::Ruin) => {
                                ruined = true;
                                ruin_time = Some(bar.time);
                            }
                        },
                    }
                }
            }
        }
        if ruined {
            continue;
        }

        // 3. Position ouverte : liquidation, stop, objectif, durée maximale.
        let Some(mut cur) = pos.take() else { continue };
        // Plus bas / plus haut pour la liquidation : prix de marque, sinon dernier prix.
        let liq_range = match mark {
            Some(m) => {
                while mark_ix < m.len() && m[mark_ix].time < bar.time {
                    mark_ix += 1;
                }
                match m.get(mark_ix).filter(|x| x.time == bar.time) {
                    Some(x) => (x.low, x.high),
                    None => {
                        stats.mark_missing += 1;
                        (bar.low, bar.high)
                    }
                }
            }
            None => (bar.low, bar.high),
        };
        let held_too_long = p.rules.max_hold_bars > 0
            && bar.time >= cur.entry_time + p.rules.max_hold_bars as i64 * interval_ms;
        match ctx.exit(&cur, bar, liq_range, entered_now, held_too_long) {
            Some((reason, price, ambiguous)) => {
                if ambiguous {
                    stats.ambiguous += 1;
                    cur.ambiguous = true;
                }
                if reason == ExitReason::Liquidation {
                    stats.liquidations += 1;
                }
                let fee_rate = if reason == ExitReason::Target {
                    ctx.maker
                } else {
                    ctx.taker
                };
                close(
                    cur,
                    bar.time,
                    price,
                    reason,
                    fee_rate,
                    &mut cash,
                    &mut trades,
                    &mut curve,
                );
            }
            None => pos = Some(cur),
        }
    }

    // Fin de la période : position encore ouverte, soldée au dernier cours.
    if let (Some(cur), Some(last)) = (pos.take(), last_in_period) {
        let price = last.close * (1.0 - cur.side.sign() * ctx.slip);
        close(
            cur,
            last.time + 60_000,
            price,
            ExitReason::Expiration,
            ctx.taker,
            &mut cash,
            &mut trades,
            &mut curve,
        );
    }

    SimResult {
        trades,
        stats,
        ruined,
        ruin_time,
        initial_budget: p.account.initial_budget,
        final_budget: cash,
        budget_curve: curve,
        bars_in_period,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::MaintenanceTier;
    use crate::fibo::ThresholdMode;
    use crate::market::{Interval, DAY_MS};

    const T0: i64 = 20_000 * DAY_MS; // minuit UTC : les paiements de funding tombent à 08:00 et 16:00
    const MIN: i64 = 60_000;

    /// Contrat de test : un contrat vaut 1 unité (prix de l'ordre de 100-200).
    fn spec() -> ContractSpec {
        ContractSpec {
            symbol: "TEST_USDT_PERP".into(),
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

    fn params() -> SimParams {
        SimParams {
            fibo: FiboParams {
                interval: Interval::M15,
                threshold_mode: ThresholdMode::Percent,
                threshold_pct: 5.0,
                max_age_bars: 48,
                ..FiboParams::default()
            },
            rules: TradeRules::default(),
            costs: Costs {
                fees: Fees {
                    maker_pct: 0.02,
                    taker_pct: 0.05,
                },
                slippage_pct: 0.01,
                funding_pct_8h: 0.01,
                liquidation_safety_pct: 0.1,
            },
            account: Account {
                initial_budget: 500.0,
                margin: 100.0,
                leverage: 10.0,
                r_only: false,
            },
            start_ms: T0,
            end_ms: T0 + 100 * DAY_MS,
        }
    }

    /// Bougies 1 min sans mèche : `(durée en minutes, prix final)` par segment,
    /// interpolation linéaire à partir de `start`.
    fn path(start: f64, segments: &[(usize, f64)]) -> Vec<Bar> {
        let mut out = Vec::new();
        let mut price = start;
        for &(minutes, end) in segments {
            for k in 1..=minutes {
                let close = price + (end - price) * k as f64 / minutes as f64;
                let open = out.last().map_or(start, |b: &Bar| b.close);
                out.push(Bar {
                    time: T0 + out.len() as i64 * MIN,
                    open,
                    high: open.max(close),
                    low: open.min(close),
                    close,
                    volume: 0.0,
                });
            }
            price = end;
        }
        out
    }

    /// Mouvement haussier 100 → 200 (A = 100, B = 200), repli confirmé à la
    /// minute 180 (clôture de la bougie de détection 11) : niveaux 161,8 ; 150 ;
    /// 138,2 ; 121,4. Le prix atteint 161,8 à la minute 199, puis suit `tail`.
    fn scenario(tail: &[(usize, f64)]) -> Vec<Bar> {
        let mut segments = vec![(60, 100.0), (100, 200.0), (40, 160.0)];
        segments.extend_from_slice(tail);
        path(100.0, &segments)
    }

    fn run(bars: &[Bar], p: &SimParams) -> SimResult {
        simulate(bars, None, &[], &spec(), p)
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_target_trade_with_maker_fees() {
        // Le rebond à 210 confirme aussi un creux : une seconde mise en place suit.
        let r = run(&scenario(&[(60, 210.0)]), &params());
        assert!(r.stats.setups >= 1 && r.stats.placed >= 1, "{:?}", r.stats);
        let t = &r.trades[0];
        assert_eq!(t.setup_id, 0);
        assert_eq!(t.reason, ExitReason::Target);
        assert_eq!(t.side, Side::Long);
        assert_eq!(t.ratio, 0.382);
        assert!(near(t.entry_price, 161.8));
        assert!(near(t.stop, 150.0), "niveau suivant : 0,5");
        assert!(near(t.target, 200.0), "retour à B");
        assert_eq!(
            t.entry_time,
            T0 + 198 * MIN,
            "premier contact : 161 à la 199e minute"
        );
        // 1 000 USDT de montant demandé / 161,8 = 6,18 : 6 contrats.
        assert_eq!(t.contracts, 6.0);
        assert!(near(t.notional, 6.0 * 161.8));
        assert!(near(t.margin, 6.0 * 161.8 / 10.0));
        assert!(near(t.gross, 6.0 * 38.2));
        // Frais maker à l'entrée et à la sortie (ordres limites).
        assert!(near(t.fee_entry, 6.0 * 161.8 * 0.0002));
        assert!(near(t.fee_exit, 6.0 * 200.0 * 0.0002));
        assert!(near(t.net, t.gross - t.fee_entry - t.fee_exit - t.funding));
        assert!(near(t.risk, 6.0 * 11.8));
        assert!(near(t.r_gross, 38.2 / 11.8));
        assert!(near(r.final_budget, 500.0 + t.net));
        assert!(!t.ambiguous && r.stats.ambiguous == 0);
        // Liquidation à x10 : 161,8 × (1 − 0,1 + 0,005) = 146,4.
        assert!(near(t.liquidation.unwrap(), 161.8 * 0.905));
    }

    #[test]
    fn a_stop_trade_pays_taker_fees_and_slippage() {
        let r = run(&scenario(&[(30, 140.0)]), &params());
        let t = &r.trades[0];
        assert_eq!(t.reason, ExitReason::Stop);
        // Exécution au marché : stop 150 − 0,01 % de glissement.
        assert!(near(t.exit_price, 150.0 * (1.0 - 0.0001)));
        assert!(near(t.fee_exit, 6.0 * t.exit_price * 0.0005));
        assert!(near(t.gross, 6.0 * (t.exit_price - 161.8)));
        assert!(t.r_gross < -1.0 && t.r_gross > -1.01);
        assert!(t.r_net < t.r_gross);
        assert!(t.net < 0.0 && near(r.final_budget, 500.0 + t.net));
        assert_eq!(r.stats.liquidations, 0);
    }

    #[test]
    fn stop_and_target_in_the_same_bar_count_the_unfavourable_exit() {
        let mut bars = scenario(&[(10, 160.0)]);
        let last = *bars.last().unwrap();
        // Une bougie 1 min touche à la fois le stop (150) et l'objectif (200).
        bars.push(Bar {
            time: last.time + MIN,
            open: 160.0,
            high: 201.0,
            low: 149.0,
            close: 190.0,
            volume: 0.0,
        });
        bars.extend(path(190.0, &[(5, 190.0)]).into_iter().map(|b| Bar {
            time: b.time + last.time + 2 * MIN - T0,
            ..b
        }));
        let r = run(&bars, &params());
        let t = &r.trades[0];
        assert_eq!(t.reason, ExitReason::Stop, "sortie défavorable");
        assert!(t.ambiguous);
        assert_eq!(r.stats.ambiguous, 1);
    }

    #[test]
    fn the_target_is_not_evaluated_in_the_entry_bar() {
        let mut bars = scenario(&[]);
        // Remplace la dernière bougie par une bougie qui touche le niveau puis
        // l'objectif : l'ordre des prix est inconnu, l'objectif attend la bougie suivante.
        bars.truncate(198); // dernière bougie : minute 197, clôture 162,2
        let t = bars.last().unwrap().time;
        bars.push(Bar {
            time: t + MIN,
            open: 163.0,
            high: 205.0,
            low: 161.0,
            close: 190.0,
            volume: 0.0,
        });
        bars.push(Bar {
            time: t + 2 * MIN,
            open: 190.0,
            high: 201.0,
            low: 189.0,
            close: 200.0,
            volume: 0.0,
        });
        let r = run(&bars, &params());
        let trade = &r.trades[0];
        assert_eq!(trade.entry_time, t + MIN);
        assert_eq!(trade.reason, ExitReason::Target);
        assert_eq!(
            trade.exit_time,
            t + 2 * MIN,
            "objectif à la bougie suivante"
        );
    }

    #[test]
    fn prudent_option_requires_crossing_the_level_by_a_tick() {
        let mut bars = scenario(&[]);
        bars.truncate(190);
        let t = bars.last().unwrap().time;
        // Contact exact du niveau 161,8 sans le traverser.
        bars.push(Bar {
            time: t + MIN,
            open: 170.0,
            high: 170.0,
            low: 161.8,
            close: 165.0,
            volume: 0.0,
        });
        bars.extend(path(165.0, &[(30, 175.0)]).into_iter().map(|b| Bar {
            time: b.time + t + MIN - T0 + MIN,
            ..b
        }));
        let touched = run(&bars, &params());
        assert_eq!(touched.trades.len(), 1, "simple contact : exécuté");
        let mut prudent = params();
        prudent.rules.prudent_tick = true;
        let crossed = run(&bars, &prudent);
        assert!(crossed.trades.is_empty(), "il faut traverser d'un tick");
    }

    #[test]
    fn liquidation_before_the_stop_refuses_the_trade_unless_it_becomes_the_stop() {
        // À x20 : liquidation à 161,8 × (1 − 0,05 + 0,005) = 154,5, au-dessus du stop (150).
        let mut p = params();
        p.account.leverage = 20.0;
        let bars = scenario(&[(20, 160.0)]);
        let r = run(&bars, &p);
        assert!(r.trades.is_empty());
        assert_eq!(r.stats.refused_liquidation, 1);
        assert_eq!(r.stats.refused(), 1);
        // Option : la liquidation devient le stop.
        p.rules.liquidation_as_stop = true;
        let r = run(&scenario(&[(30, 140.0)]), &p);
        assert_eq!(r.stats.refused(), 0);
        let t = &r.trades[0];
        assert_eq!(t.reason, ExitReason::Liquidation);
        assert_eq!(r.stats.liquidations, 1);
        // Toute la marge isolée est perdue, sans frais de sortie.
        assert!(near(t.gross, -t.margin));
        assert_eq!(t.fee_exit, 0.0);
        assert!(near(t.net, -t.margin - t.fee_entry - t.funding));
        assert!(near(r.final_budget, 500.0 + t.net));
        // Avec cette option, R est la distance entrée-liquidation.
        assert!(t.risk > 0.0 && t.r_net < -1.0);
    }

    #[test]
    fn leverage_above_the_maximum_is_rejected_up_front() {
        let mut p = params();
        p.account.leverage = 76.0;
        assert!(p.validate(&spec()).unwrap_err().contains("maximum"));
        p.account.leverage = 75.0;
        assert!(p.validate(&spec()).is_ok());
        p.account.r_only = true;
        p.account.leverage = 500.0;
        assert!(
            p.validate(&spec()).is_ok(),
            "le mode R seulement ignore le levier"
        );
    }

    #[test]
    fn less_than_one_contract_is_refused_and_counted() {
        let mut p = params();
        p.account.margin = 1.0; // 10 USDT engagés pour un contrat à 161,8
        let r = run(&scenario(&[(20, 160.0)]), &p);
        assert!(r.trades.is_empty());
        assert_eq!(r.stats.refused_min_size, 1);
        assert_eq!(r.final_budget, 500.0);
    }

    #[test]
    fn ruin_stops_the_simulation_when_the_margin_cannot_be_funded() {
        let mut p = params();
        p.account.initial_budget = 50.0; // moins que les 97 USDT de marge d'un trade
        let r = run(&scenario(&[(20, 160.0)]), &p);
        assert!(r.ruined);
        assert_eq!(r.ruin_time, Some(T0 + 198 * MIN));
        assert!(r.trades.is_empty());
        assert_eq!(r.final_budget, 50.0);
    }

    #[test]
    fn funding_is_paid_every_eight_hours_on_the_notional() {
        // Position ouverte vers 03:19, prix stable à 160 pendant 12 h : un
        // paiement à 08:00 (taux supposé 0,01 %).
        let bars = scenario(&[(720, 160.0)]);
        let r = run(&bars, &params());
        let t = &r.trades[0];
        assert_eq!(t.reason, ExitReason::Expiration, "fin des données");
        assert!(near(t.funding, 6.0 * 160.0 * 0.0001), "{}", t.funding);
        assert!(near(t.net, t.gross - t.fee_entry - t.fee_exit - t.funding));
        // Taux réel de 0,1 % pour cette période : il remplace le taux supposé.
        let period = (T0 + 8 * 3_600_000) / FUNDING_PERIOD_MS;
        let real = [FundingRate {
            time: period * FUNDING_PERIOD_MS,
            rate: 0.001,
        }];
        let r = simulate(&bars, None, &real, &spec(), &params());
        assert!(near(r.trades[0].funding, 6.0 * 160.0 * 0.001));
        // Une vente reçoit un funding positif.
        assert!(near(r.final_budget, 500.0 + r.trades[0].net));
    }

    #[test]
    fn a_second_setup_while_a_position_is_open_is_not_placed() {
        // Position ouverte (161,8), le prix repart à 190 sans atteindre l'objectif
        // (200), retombe à 160 puis remonte à 175 : de nouveaux pivots sont confirmés
        // pendant que la position est ouverte.
        let bars = scenario(&[(40, 190.0), (40, 168.0), (40, 190.0)]);
        let r = run(&bars, &params());
        assert!(r.stats.setups >= 2, "{:?}", r.stats);
        assert!(r.stats.skipped_busy >= 1, "{:?}", r.stats);
        assert_eq!(r.trades.iter().filter(|t| t.setup_id == 0).count(), 1);
    }

    #[test]
    fn an_unfilled_setup_expires_after_n_detection_bars() {
        let mut p = params();
        p.fibo.max_age_bars = 4; // une heure
                                 // Après la confirmation (minute 180, prix 181), le prix reste au-dessus des niveaux.
        let bars = path(
            100.0,
            &[(60, 100.0), (100, 200.0), (20, 181.0), (180, 181.0)],
        );
        let r = run(&bars, &p);
        assert!(r.trades.is_empty());
        assert_eq!(r.stats.placed, 1);
        assert_eq!(r.stats.unfilled, 1);
    }

    #[test]
    fn warm_up_bars_before_the_period_are_not_traded() {
        let mut p = params();
        p.start_ms = T0 + 300 * MIN; // après la mise en place
        let r = run(&scenario(&[(60, 210.0)]), &p);
        assert_eq!(r.stats.setups, 0);
        assert!(r.trades.is_empty());
    }

    #[test]
    fn contrary_direction_sells_the_level_with_a_stop_order() {
        let mut p = params();
        p.fibo.direction = Direction::Contrary;
        p.account.leverage = 3.0; // liquidation d'une vente à x3 : 214, au-delà du stop (200)
        let r = run(&scenario(&[(30, 100.0)]), &p);
        let t = &r.trades[0];
        assert_eq!(t.side, Side::Short);
        // Ordre stop exécuté au marché : 161,8 vendu avec 0,01 % de glissement défavorable.
        assert!(near(t.entry_price, 161.8 * (1.0 - 0.0001)));
        assert!(near(t.fee_entry, t.notional * 0.0005), "frais taker");
        assert!(near(t.stop, 200.0), "miroir : stop au point B");
        assert!(near(t.target, 100.0), "miroir : objectif à l'origine A");
        assert!(t.liquidation.unwrap() > t.entry_price);
    }

    #[test]
    fn r_multiples_do_not_depend_on_the_leverage() {
        let bars = scenario(&[(60, 210.0)]);
        let x10 = run(&bars, &params());
        let mut r_only = params();
        r_only.account.r_only = true;
        let x1 = run(&bars, &r_only);
        let (a, b) = (&x10.trades[0], &x1.trades[0]);
        assert!(near(a.r_gross, b.r_gross));
        assert!(
            near(a.r_net, b.r_net),
            "frais rapportés au risque : identiques"
        );
        assert_eq!(b.leverage, 1.0);
        assert!(b.liquidation.is_none());
        // La taille, elle, change.
        assert!(a.notional != b.notional);
        // Un autre levier (x5 : 3 contrats) donne les mêmes R.
        let mut x5 = params();
        x5.account.leverage = 5.0;
        let c = &run(&bars, &x5).trades[0];
        assert!(near(c.r_net, a.r_net));
    }

    #[test]
    fn extension_and_r_multiple_targets() {
        let mut p = params();
        p.rules.target = TargetRule::Extension { ratio: 1.272 };
        let t = &run(&scenario(&[(100, 230.0)]), &p).trades[0];
        // A + 1,272 × (B − A) = 227,2.
        assert!(near(t.target, 227.2), "{}", t.target);
        assert_eq!(t.reason, ExitReason::Target);
        p.rules.target = TargetRule::RMultiple { r: 2.0 };
        let t = &run(&scenario(&[(60, 190.0)]), &p).trades[0];
        assert!(near(t.target, 161.8 + 2.0 * 11.8));
        assert!(near(t.r_gross, 2.0));
        // Stops : origine A avec marge, et distance fixe.
        p.rules.stop = StopRule::Origin { margin_pct: 1.0 };
        p.rules.target = TargetRule::ReturnToB;
        p.account.leverage = 2.0; // liquidation à 81,7 : sous tous les stops essayés
        let t = &run(&scenario(&[(60, 190.0)]), &p).trades[0];
        assert!(near(t.stop, 100.0 * 0.99));
        p.rules.stop = StopRule::Fixed { pct: 3.0 };
        let t = &run(&scenario(&[(60, 190.0)]), &p).trades[0];
        assert!(near(t.stop, 161.8 * 0.97));
        // Stop du niveau suivant avec marge : 150 − 1 %.
        p.rules.stop = StopRule::NextLevel { margin_pct: 1.0 };
        let t = &run(&scenario(&[(60, 190.0)]), &p).trades[0];
        assert!(near(t.stop, 150.0 * 0.99));
    }

    #[test]
    fn mark_price_drives_liquidations_when_available() {
        let mut p = params();
        p.account.leverage = 20.0;
        p.rules.liquidation_as_stop = true;
        // Le dernier prix chute brièvement sous la liquidation (154,5) alors que
        // le prix de marque ne bouge pas : pas de liquidation.
        let mut bars = scenario(&[(5, 162.0)]);
        let t = bars.last().unwrap().time;
        bars.push(Bar {
            time: t + MIN,
            open: 162.0,
            high: 162.0,
            low: 150.0,
            close: 162.0,
            volume: 0.0,
        });
        bars.extend(path(162.0, &[(5, 163.0)]).into_iter().map(|b| Bar {
            time: b.time + t + 2 * MIN - T0,
            ..b
        }));
        let mark: Vec<Bar> = bars
            .iter()
            .map(|b| Bar {
                low: b.open.min(b.close),
                high: b.open.max(b.close),
                ..*b
            })
            .collect();
        let last_only = run(&bars, &p);
        assert_eq!(last_only.trades[0].reason, ExitReason::Liquidation);
        let with_mark = simulate(&bars, Some(&mark), &[], &spec(), &p);
        assert!(
            with_mark
                .trades
                .iter()
                .all(|t| t.reason != ExitReason::Liquidation),
            "{:?}",
            with_mark
                .trades
                .iter()
                .map(|t| t.reason)
                .collect::<Vec<_>>()
        );
    }

    /// Marche aléatoire 1 min déterministe (variations de ± 0,02 % environ).
    fn random_walk(n: usize, seed: u64) -> Vec<Bar> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as f64 / u64::MAX as f64 - 0.5
        };
        let mut price: f64 = 100.0;
        (0..n)
            .map(|i| {
                let open = price;
                let step = 0.0002 * (next() + next() + next() + next()) * 1.732;
                let close = open * (1.0 + step);
                price = close;
                Bar {
                    time: T0 + i as i64 * MIN,
                    open,
                    high: open.max(close) * (1.0 + 0.00005 * (next() + 0.5)),
                    low: open.min(close) * (1.0 - 0.00005 * (next() + 0.5)),
                    close,
                    volume: 0.0,
                }
            })
            .collect()
    }

    fn free_params() -> SimParams {
        let mut p = params();
        p.fibo = FiboParams::default(); // 1 h, 3 × ATR(14)
        p.costs = Costs {
            fees: Fees {
                maker_pct: 0.0,
                taker_pct: 0.0,
            },
            slippage_pct: 0.0,
            funding_pct_8h: 0.0,
            liquidation_safety_pct: 0.0,
        };
        p.account = Account {
            initial_budget: 1e9,
            margin: 100.0,
            leverage: 1.0,
            r_only: true,
        };
        p.end_ms = T0 + 4000 * DAY_MS;
        p
    }

    fn mean(v: impl Iterator<Item = f64>) -> f64 {
        let (n, s) = v.fold((0.0, 0.0), |(n, s), x| (n + 1.0, s + x));
        s / n
    }

    #[test]
    fn random_walk_has_no_edge_gross_and_loses_the_costs_net() {
        // Repère 7.1 : sans tendance, l'espérance brute en R est ≈ 0 et la nette ≈ −coûts.
        // Un biais d'anticipation ou une erreur d'exécution la ferait dériver.
        let bars = random_walk(1_500_000, 0xDEADBEEF_CAFEF00D);
        let mut p = free_params();
        let free = run(&bars, &p);
        let n = free.trades.len();
        assert!(n > 400, "assez de trades : {n}");
        let gross = mean(free.trades.iter().map(|t| t.r_gross));
        eprintln!("marche aléatoire : {n} trades, espérance brute {gross:.3} R");
        assert!(
            gross.abs() < 0.15,
            "espérance brute {gross:.3} R sur {n} trades"
        );
        assert!(free
            .trades
            .iter()
            .all(|t| t.fee_entry == 0.0 && t.funding == 0.0));

        // Objectif 2R, stop 1R : P(+t avant −s) ≈ s / (s + t) = 1/3.
        p.rules.target = TargetRule::RMultiple { r: 2.0 };
        let r2 = run(&bars, &p);
        let wins = r2
            .trades
            .iter()
            .filter(|t| t.reason == ExitReason::Target)
            .count();
        let losses = r2
            .trades
            .iter()
            .filter(|t| t.reason == ExitReason::Stop)
            .count();
        let share = wins as f64 / (wins + losses) as f64;
        eprintln!(
            "objectif 2R : {share:.3} de trades gagnants sur {} (théorie : 0,333)",
            wins + losses
        );
        assert!(
            (share - 1.0 / 3.0).abs() < 0.06,
            "{share:.3} sur {} trades",
            wins + losses
        );
        let exp = mean(
            r2.trades
                .iter()
                .filter(|t| t.reason != ExitReason::Expiration)
                .map(|t| t.r_gross),
        );
        assert!(exp.abs() < 0.15, "espérance brute 2R : {exp:.3}");

        // Avec les coûts : l'espérance nette recule exactement du coût moyen en R.
        p.costs = Costs::default();
        p.costs.funding_pct_8h = 0.0;
        let costly = run(&bars, &p);
        let net = mean(costly.trades.iter().map(|t| t.r_net));
        let gross_costly = mean(costly.trades.iter().map(|t| t.r_gross));
        let cost_r = mean(
            costly
                .trades
                .iter()
                .map(|t| (t.fee_entry + t.fee_exit) / t.risk),
        );
        eprintln!("coûts : {cost_r:.3} R par trade ; espérance nette {net:.3} R");
        assert!(cost_r > 0.0);
        assert!((gross_costly - net - cost_r).abs() < 1e-9);
        assert!(
            net < gross_costly && net < 0.05,
            "net {net:.3}, brut {gross_costly:.3}"
        );
    }

    /// Contrôle de dispersion (lent) : l'espérance brute reste ≈ 0 pour plusieurs graines.
    /// `cargo test -p trading-engine walk_seeds -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn walk_seeds() {
        let (mut all, mut count) = (0.0, 0.0);
        for seed in 1..=8u64 {
            let bars = random_walk(1_500_000, seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let r = run(&bars, &free_params());
            let g = mean(r.trades.iter().map(|t| t.r_gross));
            eprintln!("graine {seed} : {} trades, brut {g:+.3} R", r.trades.len());
            all += g * r.trades.len() as f64;
            count += r.trades.len() as f64;
        }
        eprintln!("ensemble : {count} trades, brut {:+.3} R", all / count);
        assert!((all / count).abs() < 0.08);
    }

    #[test]
    fn trades_closed_before_a_date_do_not_depend_on_later_data() {
        // Absence d'anticipation de bout en bout : couper les données à t ne change
        // aucun trade déjà clos avant t.
        let bars = random_walk(400_000, 0x1234_5678_9ABC_DEF1);
        let p = free_params();
        let full = run(&bars, &p);
        for cut in [120_000usize, 250_000, 333_333] {
            let part = run(&bars[..cut], &p);
            let t_end = bars[cut - 1].time;
            let strip = |r: &SimResult| -> Vec<String> {
                r.trades
                    .iter()
                    .filter(|t| t.exit_time < t_end && t.reason != ExitReason::Expiration)
                    .map(|t| serde_json::to_string(t).unwrap())
                    .collect()
            };
            let a = strip(&part);
            let b: Vec<String> = strip(&full).into_iter().take(a.len()).collect();
            assert!(!a.is_empty());
            assert_eq!(a, b, "coupure à {cut}");
        }
    }

    #[test]
    fn simulation_is_fast_enough_on_millions_of_bars() {
        // Environ 3 ans de 1 min (1,6 million de bougies) : quelques secondes au plus,
        // même en profil de test non optimisé.
        let bars = random_walk(1_600_000, 99);
        let start = std::time::Instant::now();
        let mut p = params();
        p.end_ms = T0 + 4000 * DAY_MS;
        let r = run(&bars, &p);
        assert!(r.bars_in_period > 1_000_000);
        assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    }

    #[test]
    fn parameters_are_validated() {
        let ok = params();
        assert!(ok.validate(&spec()).is_ok());
        let mut bad = ok.clone();
        bad.account.margin = 0.0;
        assert!(bad.validate(&spec()).is_err());
        let mut bad = ok.clone();
        bad.rules.stop = StopRule::Fixed { pct: 0.0 };
        assert!(bad.validate(&spec()).is_err());
        let mut bad = ok.clone();
        bad.rules.target = TargetRule::Extension { ratio: 0.9 };
        assert!(bad.validate(&spec()).is_err());
        let mut bad = ok.clone();
        bad.start_ms = bad.end_ms;
        assert!(bad.validate(&spec()).is_err());
        let mut bad = ok;
        bad.costs.fees.taker_pct = 5.0;
        assert!(bad.validate(&spec()).is_err());
    }
}

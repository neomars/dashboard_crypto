//! Identification mécanique des niveaux de retracement de Fibonacci, **sans
//! biais d'anticipation** : chaque décision n'utilise que des bougies déjà
//! closes à l'instant où elle est prise.
//!
//! **Sommets et creux (zigzag).** Sur l'unité de détection (15 min, 1 h ou 4 h),
//! un pivot n'est *confirmé* qu'à la clôture de la bougie où le prix s'est
//! retourné d'au moins le seuil depuis l'extrême : `k × ATR(14)` (ATR connu
//! avant la bougie), ou un pourcentage. L'extrême lui-même est daté de sa
//! bougie ; l'instant de confirmation est conservé. Un pivot confirmé ne change
//! jamais : le calcul est en ligne (une bougie après l'autre), donc les pivots
//! confirmés avant `t` sont identiques avec les données jusqu'à `t` ou avec
//! toutes les données (test `pivots_do_not_change_when_more_data_arrives`).
//! Une bougie ne peut pas confirmer un extrême qu'elle vient de former : l'ordre
//! du haut et du bas à l'intérieur d'une bougie est inconnu.
//!
//! **Mouvement de référence.** Les deux derniers pivots confirmés, A puis B.
//! Haussier : A creux, B sommet. Niveaux : `B − r × (B − A)` pour r dans
//! {0,382 ; 0,5 ; 0,618 ; 0,786}, valable aussi pour un mouvement baissier.
//!
//! **Mise en place**, à la confirmation de B : seuls les niveaux encore devant le
//! prix (sous la clôture pour un mouvement haussier, au-dessus pour un baissier)
//! sont posables ; les autres sont ignorés et comptés. Elle est annulée par un
//! nouveau pivot confirmé, un prix au-delà de l'origine A, ou après N bougies de
//! détection.

use crate::contract::Side;
use crate::market::{Bar, Interval};
use serde::{Deserialize, Serialize};

/// Période de l'ATR du seuil de retournement.
pub const ATR_PERIOD: usize = 14;
/// Niveaux usuels, par défaut.
pub const DEFAULT_RATIOS: [f64; 4] = [0.382, 0.5, 0.618, 0.786];
/// Échelle des niveaux, de la plus proche de B à l'origine A : le « niveau
/// suivant » d'une entrée (pour le stop) s'y lit, quelles que soient les cases cochées.
pub const LADDER: [f64; 5] = [0.382, 0.5, 0.618, 0.786, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PivotKind {
    High,
    Low,
}

/// Sens des trades par rapport au mouvement de référence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Acheter le repli d'un mouvement haussier, vendre le rebond d'un baissier.
    Continuation,
    /// Le contraire.
    Contrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThresholdMode {
    /// `k × ATR(14)` de l'unité de détection.
    Atr,
    /// Pourcentage du prix de l'extrême.
    Percent,
}

/// Paramètres de l'identification des points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiboParams {
    /// Unité de détection : 15 min, 1 h ou 4 h.
    pub interval: Interval,
    pub threshold_mode: ThresholdMode,
    /// k de `k × ATR(14)`.
    pub atr_mult: f64,
    /// Seuil en % si `threshold_mode` vaut `percent`.
    pub threshold_pct: f64,
    /// Niveaux de retracement utilisés (cases cochées).
    pub ratios: Vec<f64>,
    pub direction: Direction,
    /// Durée de vie d'une mise en place, en bougies de détection.
    pub max_age_bars: usize,
}

impl Default for FiboParams {
    fn default() -> Self {
        Self {
            interval: Interval::H1,
            threshold_mode: ThresholdMode::Atr,
            atr_mult: 3.0,
            threshold_pct: 1.0,
            ratios: DEFAULT_RATIOS.to_vec(),
            direction: Direction::Continuation,
            max_age_bars: 48,
        }
    }
}

impl FiboParams {
    pub fn validate(&self) -> Result<(), String> {
        let check = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        check(
            matches!(self.interval, Interval::M15 | Interval::H1 | Interval::H4),
            "L'unité de détection doit être 15 minutes, 1 heure ou 4 heures.",
        )?;
        check(
            self.atr_mult.is_finite() && (0.5..=20.0).contains(&self.atr_mult),
            "Le multiple d'ATR doit être compris entre 0,5 et 20.",
        )?;
        check(
            self.threshold_pct.is_finite() && (0.05..=50.0).contains(&self.threshold_pct),
            "Le seuil en pourcentage doit être compris entre 0,05 et 50 %.",
        )?;
        check(!self.ratios.is_empty(), "Cochez au moins un niveau.")?;
        check(
            self.ratios
                .iter()
                .all(|r| r.is_finite() && *r > 0.0 && *r < 1.0),
            "Les niveaux doivent être compris entre 0 et 1 (exclus).",
        )?;
        check(
            (1..=1000).contains(&self.max_age_bars),
            "La durée de vie d'une mise en place doit être de 1 à 1000 bougies.",
        )
    }
}

/// Sommet ou creux confirmé.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pivot {
    pub kind: PivotKind,
    pub price: f64,
    /// Index de la bougie de l'extrême (dans la suite des bougies de détection).
    pub idx: usize,
    /// Ouverture de cette bougie, en ms.
    pub time: i64,
    /// Index de la bougie dont la clôture a confirmé le pivot.
    pub confirmed_idx: usize,
    /// Instant de confirmation : clôture de cette bougie, en ms. Rien de ce qui
    /// suit le pivot n'est connu avant.
    pub confirmed_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Level {
    pub ratio: f64,
    pub price: f64,
}

/// Mise en place des niveaux, créée à la confirmation de B.
#[derive(Debug, Clone, Serialize)]
pub struct Setup {
    /// Rang de la mise en place (un par mouvement de référence).
    pub id: usize,
    pub a: Pivot,
    pub b: Pivot,
    /// Mouvement haussier : A creux, B sommet.
    pub up: bool,
    /// Clôture de la bougie qui a confirmé B.
    pub reference_close: f64,
    /// Niveaux posables, par ratio croissant.
    pub levels: Vec<Level>,
    /// Ratios déjà dépassés par le prix à la confirmation : ignorés.
    pub ignored: Vec<f64>,
    /// Sens des trades.
    pub side: Side,
    /// Dernière bougie de détection pendant laquelle la mise en place vit.
    pub expires_idx: usize,
}

impl Setup {
    /// Instant de création : clôture de la bougie qui confirme B. Les ordres sont
    /// actifs à partir de cet instant.
    pub fn created_time(&self) -> i64 {
        self.b.confirmed_time
    }

    pub fn created_idx(&self) -> usize {
        self.b.confirmed_idx
    }

    /// Le prix a-t-il atteint `level` dans une bougie `[low, high]` ? Le prix y
    /// arrive par le haut après un mouvement haussier (le plus bas touche) et par
    /// le bas après un mouvement baissier (le plus haut touche). Avec
    /// `tick > 0`, il faut traverser le niveau d'un tick.
    pub fn reached(&self, level: f64, low: f64, high: f64, tick: f64) -> bool {
        if self.up {
            low <= level - tick
        } else {
            high >= level + tick
        }
    }

    /// Le prix est-il allé au-delà de l'origine A ?
    pub fn beyond_origin(&self, low: f64, high: f64) -> bool {
        if self.up {
            low < self.a.price
        } else {
            high > self.a.price
        }
    }

    /// Ratio du niveau suivant sur l'échelle (stop « au-delà du niveau suivant »),
    /// 1,0 (origine A) après le dernier.
    pub fn next_ratio(ratio: f64) -> f64 {
        LADDER
            .iter()
            .copied()
            .find(|r| *r > ratio + 1e-9)
            .unwrap_or(1.0)
    }

    /// Prix du niveau de ratio `r` : `B − r × (B − A)`.
    pub fn price_at(&self, r: f64) -> f64 {
        self.b.price - r * (self.b.price - self.a.price)
    }
}

/// Un pivot confirmé et, à partir du deuxième, la mise en place qui en découle.
#[derive(Debug, Clone)]
pub struct Confirmation {
    pub pivot: Pivot,
    pub setup: Option<Setup>,
}

#[derive(Debug, Clone, Copy)]
enum State {
    /// Pas encore de pivot : on suit le plus haut et le plus bas.
    Seeking {
        max_high: (f64, usize),
        min_low: (f64, usize),
    },
    /// Dernier pivot : un creux. On suit le sommet candidat.
    Up { candidate: (f64, usize) },
    /// Dernier pivot : un sommet. On suit le creux candidat.
    Down { candidate: (f64, usize) },
}

/// Détecteur en ligne : une bougie de détection à la fois, dans l'ordre.
#[derive(Debug, Clone)]
pub struct Detector {
    params: FiboParams,
    interval_ms: i64,
    bars: Vec<Bar>,
    atr: Option<f64>,
    tr_sum: f64,
    state: Option<State>,
    pivots: Vec<Pivot>,
    setups: usize,
}

impl Detector {
    pub fn new(params: FiboParams) -> Self {
        Self {
            interval_ms: params.interval.millis(),
            params,
            bars: Vec::new(),
            atr: None,
            tr_sum: 0.0,
            state: None,
            pivots: Vec::new(),
            setups: 0,
        }
    }

    pub fn pivots(&self) -> &[Pivot] {
        &self.pivots
    }

    /// Bougies de détection reçues.
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// ATR après la dernière bougie reçue.
    pub fn atr(&self) -> Option<f64> {
        self.atr
    }

    fn threshold(&self, reference: f64) -> Option<f64> {
        match self.params.threshold_mode {
            ThresholdMode::Atr => self.atr.map(|a| self.params.atr_mult * a),
            ThresholdMode::Percent => Some(self.params.threshold_pct / 100.0 * reference),
        }
        .filter(|t| *t > 0.0)
    }

    /// Reçoit la bougie de détection suivante, close. Renvoie le pivot qu'elle
    /// confirme, s'il y en a un.
    pub fn push(&mut self, bar: Bar) -> Option<Confirmation> {
        let i = self.bars.len();
        self.bars.push(bar);
        let confirmed = self.step(i, bar);
        self.update_atr(i);
        confirmed
    }

    /// ATR de Wilder, identique à `ta::atr`, mis à jour après chaque bougie.
    fn update_atr(&mut self, i: usize) {
        let bar = self.bars[i];
        let tr = match i.checked_sub(1).map(|p| self.bars[p].close) {
            Some(prev) => (bar.high - bar.low)
                .max((bar.high - prev).abs())
                .max((bar.low - prev).abs()),
            None => bar.high - bar.low,
        };
        let n = ATR_PERIOD as f64;
        if i + 1 < ATR_PERIOD {
            self.tr_sum += tr;
        } else if i + 1 == ATR_PERIOD {
            self.tr_sum += tr;
            self.atr = Some(self.tr_sum / n);
        } else if let Some(prev) = self.atr {
            self.atr = Some((prev * (n - 1.0) + tr) / n);
        }
    }

    fn step(&mut self, i: usize, bar: Bar) -> Option<Confirmation> {
        let state = match self.state {
            None => State::Seeking {
                max_high: (bar.high, i),
                min_low: (bar.low, i),
            },
            Some(s) => s,
        };
        let (new_state, pivot) = match state {
            State::Seeking {
                mut max_high,
                mut min_low,
            } => {
                if bar.high > max_high.0 {
                    max_high = (bar.high, i);
                }
                if bar.low < min_low.0 {
                    min_low = (bar.low, i);
                }
                // Le retournement se lit sur un extrême formé par une bougie antérieure.
                let trough = min_low.1 < i
                    && self
                        .threshold(min_low.0)
                        .is_some_and(|t| bar.high - min_low.0 >= t);
                let peak = max_high.1 < i
                    && self
                        .threshold(max_high.0)
                        .is_some_and(|t| max_high.0 - bar.low >= t);
                // Les deux à la fois (très grande bougie) : le plus ancien d'abord.
                let take_trough = match (trough, peak) {
                    (true, true) => min_low.1 <= max_high.1,
                    (t, _) => t,
                };
                if trough && take_trough {
                    (
                        Some(State::Up {
                            candidate: self.highest_after(min_low.1, i),
                        }),
                        Some((PivotKind::Low, min_low)),
                    )
                } else if peak {
                    (
                        Some(State::Down {
                            candidate: self.lowest_after(max_high.1, i),
                        }),
                        Some((PivotKind::High, max_high)),
                    )
                } else {
                    (Some(State::Seeking { max_high, min_low }), None)
                }
            }
            State::Up { mut candidate } => {
                if bar.high > candidate.0 {
                    candidate = (bar.high, i);
                }
                let reversal = candidate.1 < i
                    && self
                        .threshold(candidate.0)
                        .is_some_and(|t| candidate.0 - bar.low >= t);
                if reversal {
                    (
                        Some(State::Down {
                            candidate: self.lowest_after(candidate.1, i),
                        }),
                        Some((PivotKind::High, candidate)),
                    )
                } else {
                    (Some(State::Up { candidate }), None)
                }
            }
            State::Down { mut candidate } => {
                if bar.low < candidate.0 {
                    candidate = (bar.low, i);
                }
                let reversal = candidate.1 < i
                    && self
                        .threshold(candidate.0)
                        .is_some_and(|t| bar.high - candidate.0 >= t);
                if reversal {
                    (
                        Some(State::Up {
                            candidate: self.highest_after(candidate.1, i),
                        }),
                        Some((PivotKind::Low, candidate)),
                    )
                } else {
                    (Some(State::Down { candidate }), None)
                }
            }
        };
        self.state = new_state;
        let (kind, (price, idx)) = pivot?;
        let pivot = Pivot {
            kind,
            price,
            idx,
            time: self.bars[idx].time,
            confirmed_idx: i,
            confirmed_time: bar.time + self.interval_ms,
        };
        let setup = self
            .pivots
            .last()
            .copied()
            .map(|a| self.make_setup(a, pivot, bar.close));
        self.pivots.push(pivot);
        Some(Confirmation { pivot, setup })
    }

    /// Plus haut des bougies `(from, to]` (le plus ancien en cas d'égalité).
    fn highest_after(&self, from: usize, to: usize) -> (f64, usize) {
        let mut best = (self.bars[from + 1].high, from + 1);
        for (k, b) in self.bars.iter().enumerate().take(to + 1).skip(from + 2) {
            if b.high > best.0 {
                best = (b.high, k);
            }
        }
        best
    }

    fn lowest_after(&self, from: usize, to: usize) -> (f64, usize) {
        let mut best = (self.bars[from + 1].low, from + 1);
        for (k, b) in self.bars.iter().enumerate().take(to + 1).skip(from + 2) {
            if b.low < best.0 {
                best = (b.low, k);
            }
        }
        best
    }

    fn make_setup(&mut self, a: Pivot, b: Pivot, close: f64) -> Setup {
        let up = b.kind == PivotKind::High;
        let mut ratios = self.params.ratios.clone();
        ratios.sort_by(f64::total_cmp);
        ratios.dedup();
        let (mut levels, mut ignored) = (Vec::new(), Vec::new());
        for r in ratios {
            let price = b.price - r * (b.price - a.price);
            // Posable : encore devant le prix.
            let ahead = if up { price < close } else { price > close };
            if ahead {
                levels.push(Level { ratio: r, price });
            } else {
                ignored.push(r);
            }
        }
        let continuation = self.params.direction == Direction::Continuation;
        let side = match (up, continuation) {
            (true, true) | (false, false) => Side::Long,
            (false, true) | (true, false) => Side::Short,
        };
        let id = self.setups;
        self.setups += 1;
        Setup {
            id,
            a,
            b,
            up,
            reference_close: close,
            levels,
            ignored,
            side,
            expires_idx: b.confirmed_idx + self.params.max_age_bars,
        }
    }
}

/// Pivots et mises en place d'une suite de bougies de détection.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub pivots: Vec<Pivot>,
    pub setups: Vec<Setup>,
}

/// Rejoue le détecteur sur `bars` (bougies de détection closes).
pub fn analyze(bars: &[Bar], params: &FiboParams) -> Analysis {
    let mut detector = Detector::new(params.clone());
    let mut setups = Vec::new();
    for bar in bars {
        if let Some(c) = detector.push(*bar) {
            setups.extend(c.setup);
        }
    }
    Analysis {
        pivots: detector.pivots().to_vec(),
        setups,
    }
}

/// Début de la période de `interval_ms` qui contient `time`.
pub fn bucket_start(time: i64, interval_ms: i64) -> i64 {
    time.div_euclid(interval_ms) * interval_ms
}

/// Agrège des bougies fines en bougies de détection, une à la fois : une
/// bougie de détection est rendue quand la première bougie fine de la période
/// suivante arrive, donc seulement quand elle est close.
#[derive(Debug, Clone)]
pub struct Aggregator {
    interval_ms: i64,
    current: Option<Bar>,
}

impl Aggregator {
    pub fn new(interval: Interval) -> Self {
        Self {
            interval_ms: interval.millis(),
            current: None,
        }
    }

    /// Ajoute une bougie fine ; rend la bougie de détection qu'elle vient de clore.
    pub fn push(&mut self, bar: &Bar) -> Option<Bar> {
        let start = bucket_start(bar.time, self.interval_ms);
        match &mut self.current {
            Some(cur) if cur.time == start => {
                cur.high = cur.high.max(bar.high);
                cur.low = cur.low.min(bar.low);
                cur.close = bar.close;
                cur.volume += bar.volume;
                None
            }
            slot => {
                let done = slot.take();
                *slot = Some(Bar {
                    time: start,
                    ..*bar
                });
                done
            }
        }
    }
}

/// Bougies de détection complètes à partir de bougies fines triées. La dernière
/// période n'est rendue que si sa dernière minute est présente.
pub fn aggregate(fine: &[Bar], fine_interval: Interval, interval: Interval) -> Vec<Bar> {
    let mut agg = Aggregator::new(interval);
    let mut out: Vec<Bar> = fine.iter().filter_map(|b| agg.push(b)).collect();
    if let (Some(cur), Some(last)) = (agg.current, fine.last()) {
        if last.time + fine_interval.millis() >= cur.time + interval.millis() {
            out.push(cur);
        }
    }
    out
}

/// État d'un niveau dans le panneau « Points actuels ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LevelStatus {
    #[serde(rename = "En attente")]
    Pending,
    #[serde(rename = "Déclenché")]
    Triggered,
    #[serde(rename = "Invalidé")]
    Invalidated,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelView {
    pub ratio: f64,
    pub price: f64,
    pub status: LevelStatus,
}

/// Panneau « Points actuels » : dernier mouvement confirmé et état de ses niveaux.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentPoints {
    pub up: bool,
    pub side: Side,
    pub a_price: f64,
    pub a_time: i64,
    pub b_price: f64,
    pub b_time: i64,
    pub confirmed_time: i64,
    pub levels: Vec<LevelView>,
    /// Niveaux déjà dépassés à la confirmation, ignorés.
    pub ignored: Vec<f64>,
    /// La mise en place peut encore déclencher un trade.
    pub active: bool,
    /// Pourquoi elle est terminée, le cas échéant.
    pub ended: Option<String>,
    /// Clôture de la dernière bougie de détection.
    pub last_close: f64,
    pub last_time: i64,
}

/// État de la dernière mise en place d'après les bougies de détection connues.
/// Un niveau déclenché consomme la mise en place (un trade par mouvement) : les
/// autres niveaux sont alors invalidés. `tick` : traversée exigée (0 = simple contact).
pub fn current_points(bars: &[Bar], params: &FiboParams, tick: f64) -> Option<CurrentPoints> {
    let analysis = analyze(bars, params);
    let setup = analysis.setups.last()?;
    let last = bars.last()?;
    let mut status: Vec<LevelStatus> = vec![LevelStatus::Pending; setup.levels.len()];
    let mut ended: Option<String> = None;
    let mut used = false;
    for (i, bar) in bars.iter().enumerate().skip(setup.created_idx() + 1) {
        if !used {
            // Premier niveau rencontré par le prix : le plus proche de B.
            let order: Vec<usize> = (0..setup.levels.len()).collect();
            for k in order {
                if setup.reached(setup.levels[k].price, bar.low, bar.high, tick) {
                    status[k] = LevelStatus::Triggered;
                    used = true;
                    break;
                }
            }
        }
        if setup.beyond_origin(bar.low, bar.high) {
            ended = Some("Prix au-delà de l'origine A".into());
        } else if i >= setup.expires_idx {
            ended = Some(format!(
                "Délai de {} bougies de détection écoulé",
                params.max_age_bars
            ));
        }
        if used || ended.is_some() {
            break;
        }
    }
    if used && ended.is_none() {
        ended = Some("Niveau déclenché : un trade par mouvement".into());
    }
    if ended.is_some() {
        for s in &mut status {
            if *s == LevelStatus::Pending {
                *s = LevelStatus::Invalidated;
            }
        }
    }
    Some(CurrentPoints {
        up: setup.up,
        side: setup.side,
        a_price: setup.a.price,
        a_time: setup.a.time,
        b_price: setup.b.price,
        b_time: setup.b.time,
        confirmed_time: setup.created_time(),
        levels: setup
            .levels
            .iter()
            .zip(&status)
            .map(|(l, s)| LevelView {
                ratio: l.ratio,
                price: l.price,
                status: *s,
            })
            .collect(),
        ignored: setup.ignored.clone(),
        active: ended.is_none(),
        ended,
        last_close: last.close,
        last_time: last.time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::DAY_MS;
    use crate::ta::atr;

    const H: i64 = 3_600_000;

    fn bar(i: usize, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar {
            time: i as i64 * H,
            open,
            high,
            low,
            close,
            volume: 1.0,
        }
    }

    /// Bougies sans mèche autour des clôtures données.
    fn bars_from(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let open = if i == 0 { c } else { closes[i - 1] };
                bar(i, open, open.max(c), open.min(c), c)
            })
            .collect()
    }

    fn pct_params(pct: f64) -> FiboParams {
        FiboParams {
            threshold_mode: ThresholdMode::Percent,
            threshold_pct: pct,
            ..FiboParams::default()
        }
    }

    /// Marche aléatoire déterministe en bougies de 1 h.
    fn random_walk(n: usize, seed: u64) -> Vec<Bar> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as f64 / u64::MAX as f64 - 0.5
        };
        let mut price = 1000.0;
        (0..n)
            .map(|i| {
                let open = price;
                let close = open * (1.0 + 0.012 * (next() + next() + next()));
                price = close;
                bar(
                    i,
                    open,
                    open.max(close) * (1.0 + 0.003 * (next() + 0.5)),
                    open.min(close) * (1.0 - 0.003 * (next() + 0.5)),
                    close,
                )
            })
            .collect()
    }

    #[test]
    fn incremental_atr_matches_the_batch_indicator() {
        let bars = random_walk(200, 7);
        let batch = atr(&bars, ATR_PERIOD);
        let mut d = Detector::new(FiboParams::default());
        for (i, b) in bars.iter().enumerate() {
            d.push(*b);
            match batch[i] {
                Some(v) => assert!((d.atr().unwrap() - v).abs() < 1e-9, "bougie {i}"),
                None => assert!(d.atr().is_none()),
            }
        }
    }

    #[test]
    fn pivots_do_not_change_when_more_data_arrives() {
        // Test obligatoire : les pivots confirmés avant t sont identiques avec les
        // données jusqu'à t et avec toutes les données.
        for (seed, params) in [
            (1u64, FiboParams::default()),
            (2, pct_params(2.0)),
            (
                3,
                FiboParams {
                    atr_mult: 1.5,
                    ..FiboParams::default()
                },
            ),
        ] {
            let bars = random_walk(1500, seed);
            let full = analyze(&bars, &params);
            assert!(
                full.pivots.len() > 10,
                "assez de pivots : {}",
                full.pivots.len()
            );
            for t in (0..bars.len()).step_by(37) {
                let upto = analyze(&bars[..=t], &params);
                let expected: Vec<Pivot> = full
                    .pivots
                    .iter()
                    .copied()
                    .filter(|p| p.confirmed_idx <= t)
                    .collect();
                assert_eq!(upto.pivots, expected, "graine {seed}, t = {t}");
                let setups: Vec<usize> = full
                    .setups
                    .iter()
                    .filter(|s| s.created_idx() <= t)
                    .map(|s| s.id)
                    .collect();
                assert_eq!(upto.setups.iter().map(|s| s.id).collect::<Vec<_>>(), setups);
                for (a, b) in upto.setups.iter().zip(&full.setups) {
                    assert_eq!(a.levels, b.levels, "niveaux identiques, t = {t}");
                    assert_eq!(a.ignored, b.ignored);
                }
            }
            // Un pivot n'est jamais confirmé avant son extrême, ni sans retournement.
            for p in &full.pivots {
                assert!(p.confirmed_idx > p.idx);
                assert!(p.confirmed_time == bars[p.confirmed_idx].time + H);
                assert_eq!(p.time, bars[p.idx].time);
            }
        }
    }

    #[test]
    fn a_pivot_is_confirmed_when_price_reverses_by_the_threshold() {
        // Seuil de 10 %. Creux à 100 (bougie 2), sommet à 130 (bougie 5), puis repli.
        let closes = [
            110.0, 105.0, 100.0, 108.0, 125.0, 130.0, 125.0, 118.0, 116.0, 120.0,
        ];
        let b = bars_from(&closes);
        let a = analyze(&b, &pct_params(10.0));
        // Le creux (100) est confirmé quand le plus haut atteint 110 : bougie 4 (open 108, high 125).
        let low = a.pivots[0];
        assert_eq!((low.kind, low.price, low.idx), (PivotKind::Low, 100.0, 2));
        assert_eq!(low.confirmed_idx, 4);
        assert_eq!(low.confirmed_time, 5 * H, "clôture de la bougie 4");
        // Le sommet (130) est confirmé quand le plus bas passe sous 117 : bougie 8
        // (118 n'est pas assez bas, 116 l'est).
        let high = a.pivots[1];
        assert_eq!(
            (high.kind, high.price, high.idx),
            (PivotKind::High, 130.0, 5)
        );
        assert_eq!(high.confirmed_idx, 8);
        // Un seul mouvement : A = creux, B = sommet.
        assert_eq!(a.setups.len(), 1);
        let s = &a.setups[0];
        assert!(s.up);
        assert_eq!((s.a.price, s.b.price), (100.0, 130.0));
        assert_eq!(s.created_time(), 9 * H);
        assert_eq!(s.reference_close, 116.0);
    }

    #[test]
    fn a_bar_cannot_confirm_the_extreme_it_just_formed() {
        // Une seule grande bougie d'abord haussière : son haut ne confirme pas son bas.
        let b = vec![
            bar(0, 100.0, 120.0, 99.0, 118.0),
            bar(1, 118.0, 119.0, 117.0, 118.0),
        ];
        let a = analyze(&b, &pct_params(5.0));
        assert_eq!(
            a.pivots.len(),
            1,
            "le creux de la bougie 0 est confirmé à la bougie 1"
        );
        assert_eq!(a.pivots[0].idx, 0);
        assert_eq!(a.pivots[0].confirmed_idx, 1);
        let one = analyze(&b[..1], &pct_params(5.0));
        assert!(one.pivots.is_empty());
    }

    #[test]
    fn levels_follow_b_minus_r_times_b_minus_a() {
        let params = FiboParams {
            ratios: DEFAULT_RATIOS.to_vec(),
            ..pct_params(5.0)
        };
        // Mouvement haussier : A = 100, B = 200.
        let up = bars_from(&[100.0, 110.0, 150.0, 200.0, 195.0, 188.0]);
        let s = &analyze(&up, &params).setups[0];
        let prices: Vec<f64> = s
            .levels
            .iter()
            .map(|l| (l.price * 1000.0).round() / 1000.0)
            .collect();
        assert_eq!(prices, vec![161.8, 150.0, 138.2, 121.4]);
        assert_eq!(s.side, Side::Long, "continuation : acheter le repli");
        assert!(s.ignored.is_empty());
        // Mouvement baissier : A = 200, B = 100.
        let down = bars_from(&[200.0, 190.0, 150.0, 100.0, 105.0, 112.0]);
        let s = &analyze(&down, &params).setups[0];
        assert!(!s.up);
        let prices: Vec<f64> = s
            .levels
            .iter()
            .map(|l| (l.price * 1000.0).round() / 1000.0)
            .collect();
        assert_eq!(prices, vec![138.2, 150.0, 161.8, 178.6]);
        assert_eq!(s.side, Side::Short, "continuation : vendre le rebond");
        assert!((s.price_at(0.5) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn levels_already_passed_at_confirmation_are_ignored_and_counted() {
        // B = 200, la confirmation arrive sur une clôture à 150 (chute rapide) :
        // 161,8 (0,382) et 150 (0,5, égal à la clôture) sont déjà dépassés.
        let b = bars_from(&[100.0, 120.0, 200.0, 150.0]);
        let s = &analyze(&b, &pct_params(5.0)).setups[0];
        assert_eq!(s.reference_close, 150.0);
        assert_eq!(s.ignored, vec![0.382, 0.5]);
        let kept: Vec<f64> = s.levels.iter().map(|l| l.ratio).collect();
        assert_eq!(kept, vec![0.618, 0.786]);
        assert!(s.levels.iter().all(|l| l.price < s.reference_close));
        // Mouvement baissier : mêmes règles, à l'envers.
        let b = bars_from(&[200.0, 180.0, 100.0, 150.0]);
        let s = &analyze(&b, &pct_params(5.0)).setups[0];
        assert_eq!(s.ignored, vec![0.382, 0.5]);
        assert!(s.levels.iter().all(|l| l.price > s.reference_close));
    }

    #[test]
    fn contrary_direction_flips_the_side() {
        let params = FiboParams {
            direction: Direction::Contrary,
            ..pct_params(5.0)
        };
        let up = bars_from(&[100.0, 110.0, 150.0, 200.0, 195.0, 188.0]);
        assert_eq!(analyze(&up, &params).setups[0].side, Side::Short);
        let down = bars_from(&[200.0, 190.0, 150.0, 100.0, 105.0, 112.0]);
        assert_eq!(analyze(&down, &params).setups[0].side, Side::Long);
    }

    #[test]
    fn cancellation_conditions() {
        let params = FiboParams {
            max_age_bars: 3,
            ..pct_params(5.0)
        };
        let b = bars_from(&[100.0, 110.0, 150.0, 200.0, 195.0, 188.0]);
        let s = analyze(&b, &params).setups.remove(0);
        assert_eq!(s.expires_idx, s.created_idx() + 3);
        // Prix au-delà de l'origine A (creux à 100) : mouvement haussier.
        assert!(
            !s.beyond_origin(100.0, 130.0),
            "toucher A n'est pas le dépasser"
        );
        assert!(s.beyond_origin(99.9, 130.0));
        // Niveaux : le plus bas doit toucher (haussier), avec ou sans tick.
        assert!(s.reached(150.0, 150.0, 160.0, 0.0));
        assert!(
            !s.reached(150.0, 150.0, 160.0, 0.1),
            "il faut traverser d'un tick"
        );
        assert!(s.reached(150.0, 149.9, 160.0, 0.1));
        assert!(!s.reached(150.0, 150.1, 160.0, 0.0));
        // Mouvement baissier : le plus haut doit toucher ; A est un sommet.
        let d = bars_from(&[200.0, 190.0, 150.0, 100.0, 105.0, 112.0]);
        let s = analyze(&d, &params).setups.remove(0);
        assert!(s.reached(150.0, 120.0, 150.0, 0.0));
        assert!(!s.reached(150.0, 120.0, 150.0, 0.1));
        assert!(s.beyond_origin(190.0, 200.1));
        assert!(!s.beyond_origin(190.0, 200.0));
    }

    #[test]
    fn a_new_pivot_creates_a_new_setup_with_the_previous_pivot_as_origin() {
        // Creux 100, sommet 200, creux 140 : deux mouvements.
        let closes = [
            100.0, 130.0, 200.0, 180.0, 160.0, 140.0, 150.0, 165.0, 170.0,
        ];
        let a = analyze(&bars_from(&closes), &pct_params(10.0));
        assert_eq!(a.pivots.len(), 3);
        assert_eq!(a.setups.len(), 2);
        assert_eq!(a.setups[1].a.price, 200.0, "l'ancien B devient l'origine");
        assert_eq!(a.setups[1].b.price, 140.0);
        assert!(!a.setups[1].up);
        assert_eq!(a.setups[1].id, 1);
        assert!(a.setups[0].created_idx() < a.setups[1].created_idx());
    }

    #[test]
    fn current_points_panel_reports_level_statuses() {
        let params = FiboParams {
            max_age_bars: 20,
            ..pct_params(5.0)
        };
        // Mouvement haussier 100 → 200, confirmé à 188, puis repli jusqu'à 150.
        let mut closes = vec![100.0, 110.0, 150.0, 200.0, 195.0, 188.0];
        let none = current_points(&bars_from(&closes), &params, 0.0).unwrap();
        assert!(none.up && none.active && none.ended.is_none());
        assert_eq!((none.a_price, none.b_price), (100.0, 200.0));
        assert!(none.levels.iter().all(|l| l.status == LevelStatus::Pending));
        assert_eq!(none.confirmed_time, 6 * H);
        assert_eq!(none.a_time, 0);
        assert_eq!(none.b_time, 3 * H);
        // Le prix descend à 150 : le 0,382 (161,8) est touché, le setup est consommé.
        closes.extend([170.0, 160.0]);
        let used = current_points(&bars_from(&closes), &params, 0.0).unwrap();
        assert_eq!(used.levels[0].status, LevelStatus::Triggered);
        assert!(used.levels[1..]
            .iter()
            .all(|l| l.status == LevelStatus::Invalidated));
        assert!(!used.active);
        // Prix au-delà de A : tous les niveaux non touchés sont invalidés.
        let mut broken = vec![100.0, 110.0, 150.0, 200.0, 195.0, 188.0];
        broken.extend([192.0, 195.0, 190.0]);
        let ok = current_points(&bars_from(&broken), &params, 0.0).unwrap();
        assert!(ok.active);
        broken.push(90.0);
        let gone = current_points(&bars_from(&broken), &params, 0.0).unwrap();
        assert!(!gone.active);
        assert!(gone.ended.as_deref().unwrap().contains("origine"));
        assert!(current_points(&bars_from(&[100.0, 101.0]), &params, 0.0).is_none());
    }

    #[test]
    fn the_age_limit_ends_a_setup() {
        let params = FiboParams {
            max_age_bars: 2,
            ..pct_params(5.0)
        };
        let mut closes = vec![100.0, 110.0, 150.0, 200.0, 195.0, 188.0];
        closes.extend([189.0, 190.0, 191.0]);
        let p = current_points(&bars_from(&closes), &params, 0.0).unwrap();
        assert!(!p.active);
        assert!(p.ended.as_deref().unwrap().contains("Délai"));
        assert!(p
            .levels
            .iter()
            .all(|l| l.status == LevelStatus::Invalidated));
    }

    #[test]
    fn minutes_are_aggregated_into_complete_detection_bars() {
        let minute = |t: i64, o: f64, h: f64, l: f64, c: f64| Bar {
            time: t * 60_000,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 1.0,
        };
        // 2 périodes de 15 minutes complètes + 1 partielle.
        let mut fine = Vec::new();
        for t in 0..35 {
            let p = 100.0 + t as f64;
            fine.push(minute(t, p, p + 0.5, p - 0.5, p + 0.2));
        }
        let out = aggregate(&fine, Interval::M1, Interval::M15);
        assert_eq!(out.len(), 2, "la période en cours est exclue");
        assert_eq!(out[0].time, 0);
        assert_eq!(out[1].time, 15 * 60_000);
        assert_eq!(out[0].open, 100.0);
        assert_eq!(out[0].close, 114.2);
        assert_eq!(out[0].high, 114.5);
        assert_eq!(out[0].low, 99.5);
        // Une période dont la dernière minute est présente est complète.
        let out = aggregate(&fine[..30], Interval::M1, Interval::M15);
        assert_eq!(out.len(), 2);
        // Périodes alignées sur l'epoch (UTC).
        assert_eq!(
            bucket_start(DAY_MS + 5 * 3_600_000 + 1234, 4 * 3_600_000),
            DAY_MS + 4 * 3_600_000
        );
    }

    #[test]
    fn parameters_are_validated() {
        assert!(FiboParams::default().validate().is_ok());
        for bad in [
            FiboParams {
                interval: Interval::D1,
                ..FiboParams::default()
            },
            FiboParams {
                atr_mult: 0.0,
                ..FiboParams::default()
            },
            FiboParams {
                ratios: vec![],
                ..FiboParams::default()
            },
            FiboParams {
                ratios: vec![1.2],
                ..FiboParams::default()
            },
            FiboParams {
                max_age_bars: 0,
                ..FiboParams::default()
            },
            FiboParams {
                threshold_pct: 0.0,
                ..FiboParams::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert_eq!(Setup::next_ratio(0.618), 0.786);
        assert_eq!(Setup::next_ratio(0.786), 1.0);
        assert_eq!(Setup::next_ratio(0.45), 0.5);
        assert_eq!(Setup::next_ratio(0.9), 1.0);
    }
}

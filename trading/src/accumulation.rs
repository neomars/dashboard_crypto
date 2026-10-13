//! Accumulation de BTC : achats au comptant à intervalle régulier (DCA), en plus
//! grande quantité quand le bitcoin est bon marché. Le module ne vend jamais.
//!
//! Le montant de chaque achat est `montant de base × poids`. Le poids vient de
//! l'un des modes de pondération :
//! - **fixe** : 1 (la référence) ;
//! - **MVRV par paliers** : multiplicateur selon la valeur du MVRV (par défaut
//!   ×3 sous 1, ×1,5 sous 2, ×1 sous 3, ×0,5 au-delà) ;
//! - **MVRV inverse** : 1 / MVRV ;
//! - **prix / moyenne 200 jours** : moyenne simple 200 j des clôtures / dernière
//!   clôture. Sert aussi de secours quand le MVRV manque.
//!
//! Aucune règle « tout ou rien » : le poids ne tombe jamais à zéro par une
//! condition du type « acheter seulement si MVRV < 1 » (le MVRV n'est resté sous
//! 1 que quelques semaines depuis 2023, et ses sommets baissent à chaque cycle).
//! Un palier peut en revanche avoir un multiplicateur de 0 si l'utilisateur le
//! choisit. Un multiplicateur maximal borne tous les poids.
//!
//! **Chronologie, sans biais d'anticipation** : pour un achat le jour D, le poids
//! n'utilise que la dernière clôture journalière complète (celle du jour D−1) et
//! le MVRV daté de D−1 au plus tard. L'ordre est exécuté à l'ouverture de la
//! bougie journalière du jour D, au prix glissé, frais déduits du montant.
//!
//! **Comparaisons** (mêmes dates, mêmes prix d'exécution, mêmes frais) :
//! - à budget égal : un DCA fixe qui dépense exactement le même total, en
//!   montants égaux. L'écart en BTC est le chiffre principal : il dit ce que la
//!   pondération apporte, indépendamment de ce qu'on dépense ;
//! - au même montant de base : un DCA fixe de `montant de base` à chaque date,
//!   donc à budget total différent.

use crate::feed::MarketFeed;
use crate::market::{date_ms, fmt_time, Bar, Instrument, Interval, Market, DAY_MS};
use crate::mvrv::{value_on_or_before, MvrvData, MvrvInfo, STALE_AFTER_DAYS};
use crate::ta::sma;
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Période de la moyenne mobile du mode « prix / moyenne 200 jours ».
pub const MA_DAYS: usize = 200;
/// Plus petit palier de MVRV utile (sert de borne inférieure du MVRV 1 / x).
const MAX_TIERS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weighting {
    Fixed,
    MvrvTiers,
    MvrvInverse,
    PriceMa200,
}

impl Weighting {
    pub fn label(self) -> &'static str {
        match self {
            Weighting::Fixed => "Fixe (référence)",
            Weighting::MvrvTiers => "MVRV par paliers",
            Weighting::MvrvInverse => "MVRV inverse (1 / MVRV)",
            Weighting::PriceMa200 => "Prix / moyenne 200 jours",
        }
    }

    pub fn uses_mvrv(self) -> bool {
        matches!(self, Weighting::MvrvTiers | Weighting::MvrvInverse)
    }
}

pub fn default_tier_bounds() -> Vec<f64> {
    vec![1.0, 2.0, 3.0]
}

pub fn default_tier_multipliers() -> Vec<f64> {
    vec![3.0, 1.5, 1.0, 0.5]
}

fn default_max_multiplier() -> f64 {
    3.0
}

/// Paramètres saisis dans l'interface (pourcentages en %).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccumulationParams {
    pub start: NaiveDate,
    /// Incluse : un achat prévu ce jour-là est simulé.
    pub end: NaiveDate,
    /// Montant de base par achat, en USDT.
    pub base_amount: f64,
    pub frequency: Frequency,
    /// Jour de la semaine (1 = lundi … 7 = dimanche), pour la fréquence hebdomadaire.
    pub weekday: u32,
    /// Jour du mois (1 à 31), pour la fréquence mensuelle ; un mois plus court
    /// est acheté son dernier jour.
    pub month_day: u32,
    pub weighting: Weighting,
    /// Bornes supérieures (exclues) des paliers du MVRV, croissantes.
    #[serde(default = "default_tier_bounds")]
    pub tier_bounds: Vec<f64>,
    /// Multiplicateurs des paliers : un de plus que les bornes (le dernier vaut à partir de la dernière borne).
    #[serde(default = "default_tier_multipliers")]
    pub tier_multipliers: Vec<f64>,
    /// Plafond de tous les poids (multiplicateur maximal).
    #[serde(default = "default_max_multiplier")]
    pub max_multiplier: f64,
    /// Frais par ordre, en % du montant.
    pub fee_pct: f64,
    /// Glissement par ordre, en % du cours.
    pub slippage_pct: f64,
    /// BTC déjà détenus au départ (facultatif).
    #[serde(default)]
    pub initial_btc: f64,
}

impl AccumulationParams {
    pub fn validate(&self) -> Result<(), String> {
        let check = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        check(
            self.start <= self.end,
            "La date de début ne peut pas suivre la date de fin.",
        )?;
        check(
            self.base_amount.is_finite() && self.base_amount > 0.0 && self.base_amount <= 1e9,
            "Le montant de base doit être positif.",
        )?;
        check(
            (1..=7).contains(&self.weekday),
            "Le jour de la semaine doit être compris entre 1 (lundi) et 7 (dimanche).",
        )?;
        check(
            (1..=31).contains(&self.month_day),
            "Le jour du mois doit être compris entre 1 et 31.",
        )?;
        check(
            !self.tier_bounds.is_empty() && self.tier_bounds.len() <= MAX_TIERS,
            "Il faut entre 1 et 8 bornes de paliers.",
        )?;
        check(
            self.tier_bounds.iter().all(|b| b.is_finite() && *b > 0.0)
                && self.tier_bounds.windows(2).all(|w| w[0] < w[1]),
            "Les bornes des paliers doivent être positives et strictement croissantes.",
        )?;
        check(
            self.tier_multipliers.len() == self.tier_bounds.len() + 1,
            "Il faut un multiplicateur de plus que de bornes de paliers.",
        )?;
        check(
            self.tier_multipliers
                .iter()
                .all(|m| m.is_finite() && (0.0..=100.0).contains(m)),
            "Les multiplicateurs des paliers doivent être compris entre 0 et 100.",
        )?;
        check(
            self.max_multiplier.is_finite() && (1.0..=100.0).contains(&self.max_multiplier),
            "Le multiplicateur maximal doit être compris entre 1 et 100.",
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
            self.initial_btc.is_finite() && self.initial_btc >= 0.0,
            "Le stack de BTC initial ne peut pas être négatif.",
        )
    }

    /// Multiplicateur du palier où tombe `mvrv` (borne supérieure exclue).
    pub fn tier_multiplier(&self, mvrv: f64) -> f64 {
        let k = self.tier_bounds.iter().take_while(|b| mvrv >= **b).count();
        self.tier_multipliers[k]
    }

    /// Premier jour dont la clôture est nécessaire : 200 jours avant le début.
    pub fn data_start(&self) -> NaiveDate {
        self.start - Duration::days(MA_DAYS as i64 + 10)
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (y, m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1)
        .and_then(|d| d.pred_opt())
        .map_or(28, |d| d.day())
}

/// Dates d'achat prévues dans `[start, end]` (bornes incluses).
pub fn purchase_dates(params: &AccumulationParams) -> Vec<NaiveDate> {
    let mut dates = Vec::new();
    let mut day = params.start;
    while day <= params.end {
        let due = match params.frequency {
            Frequency::Weekly => day.weekday().number_from_monday() == params.weekday,
            Frequency::Monthly => {
                day.day() == params.month_day.min(days_in_month(day.year(), day.month()))
            }
        };
        if due {
            dates.push(day);
        }
        let Some(next) = day.succ_opt() else { break };
        day = next;
    }
    dates
}

/// D'où vient le poids d'un achat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WeightSource {
    #[serde(rename = "Fixe")]
    Fixed,
    #[serde(rename = "MVRV")]
    Mvrv,
    #[serde(rename = "Prix / moyenne 200 j")]
    PriceMa200,
    /// Secours : MVRV absent ou périmé à cette date.
    #[serde(rename = "Prix / moyenne 200 j (secours)")]
    PriceMa200Fallback,
    /// Ni MVRV ni moyenne 200 j calculable : montant de base.
    #[serde(rename = "Montant de base (pas de donnée)")]
    NoData,
}

/// Poids d'un achat, plafonné par le multiplicateur maximal.
/// `mvrv` : MVRV connu à la date de l'achat ; `ma_ratio` : moyenne 200 j / dernière clôture.
pub fn weight(
    params: &AccumulationParams,
    mvrv: Option<f64>,
    ma_ratio: Option<f64>,
) -> (f64, WeightSource) {
    let cap = |w: f64| w.min(params.max_multiplier);
    match params.weighting {
        Weighting::Fixed => (1.0, WeightSource::Fixed),
        Weighting::PriceMa200 => match ma_ratio {
            Some(r) => (cap(r), WeightSource::PriceMa200),
            None => (1.0, WeightSource::NoData),
        },
        Weighting::MvrvTiers | Weighting::MvrvInverse => match (mvrv, ma_ratio) {
            (Some(m), _) => {
                let w = if params.weighting == Weighting::MvrvTiers {
                    params.tier_multiplier(m)
                } else {
                    1.0 / m
                };
                (cap(w), WeightSource::Mvrv)
            }
            (None, Some(r)) => (cap(r), WeightSource::PriceMa200Fallback),
            (None, None) => (1.0, WeightSource::NoData),
        },
    }
}

/// Un achat simulé.
#[derive(Debug, Clone, Serialize)]
pub struct Purchase {
    pub time: i64,
    pub date: String,
    /// Cours d'ouverture de la bougie du jour.
    pub open: f64,
    /// Cours d'exécution, glissement inclus.
    pub price: f64,
    /// Dernière clôture complète (veille), sur laquelle le poids est calculé.
    pub prev_close: f64,
    pub mvrv: Option<f64>,
    /// Moyenne 200 j / dernière clôture (supérieur à 1 : prix sous la moyenne).
    pub ma_ratio: Option<f64>,
    pub weight: f64,
    pub source: WeightSource,
    /// Montant dépensé, en USDT (frais compris).
    pub amount: f64,
    pub fee: f64,
    /// BTC reçus, frais déduits.
    pub btc: f64,
}

/// État à la clôture de chaque jour, du premier achat à la fin.
#[derive(Debug, Clone, Serialize)]
pub struct AccPoint {
    pub time: i64,
    pub close: f64,
    /// Stack de BTC (stack initial compris) : DCA pondéré.
    pub btc: f64,
    /// DCA fixe qui dépense le même total, en montants égaux.
    pub btc_equal_budget: f64,
    /// DCA fixe au montant de base.
    pub btc_fixed_base: f64,
    /// MVRV connu ce jour-là.
    pub mvrv: Option<f64>,
}

/// Un DCA fixe de comparaison, face au DCA pondéré.
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    /// USDT dépensés par la référence.
    pub spent: f64,
    /// BTC accumulés par la référence (stack initial exclu).
    pub btc: f64,
    pub avg_cost: Option<f64>,
    /// BTC en plus du DCA pondéré par rapport à cette référence (négatif : en moins).
    pub extra_btc: f64,
    /// Même écart, en % des BTC de la référence.
    pub extra_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccumulationSummary {
    pub first_date: String,
    pub last_date: String,
    pub purchases: usize,
    /// Achats prévus mais non simulés (pas de bougie, pas de clôture précédente).
    pub skipped: usize,
    /// USDT dépensés.
    pub spent: f64,
    pub fees: f64,
    /// BTC accumulés par les achats (stack initial exclu).
    pub btc_bought: f64,
    pub initial_btc: f64,
    /// Stack final : stack initial + BTC accumulés.
    pub btc_total: f64,
    /// Coût moyen par BTC accumulé, frais et glissement compris.
    pub avg_cost: Option<f64>,
    pub last_close: f64,
    /// Valeur du stack final à la dernière clôture, en USDT.
    pub value_usdt: f64,
    /// Poids moyen : dépensé / (achats × montant de base).
    pub avg_weight: f64,
    pub max_weight: f64,
    /// Achats pondérés par le prix / moyenne 200 j faute de MVRV.
    pub fallback_purchases: usize,
    /// Achats faits au montant de base faute de toute donnée.
    pub no_data_purchases: usize,
    /// **Chiffre principal** : DCA fixe qui dépense exactement le même total.
    pub equal_budget: Comparison,
    /// DCA fixe au même montant de base (budget total différent).
    pub fixed_base: Comparison,
}

#[derive(Debug, Clone, Serialize)]
pub struct Accumulation {
    pub params: AccumulationParams,
    pub purchases: Vec<Purchase>,
    pub points: Vec<AccPoint>,
    pub summary: AccumulationSummary,
    /// Sources et contrôle d'écart du MVRV, si le mode l'utilise.
    pub mvrv: Option<MvrvInfo>,
    pub warnings: Vec<String>,
}

/// Index de la bougie journalière du jour `date`.
fn bar_index(bars: &[Bar], date: NaiveDate) -> Option<usize> {
    let t = date_ms(date);
    bars.binary_search_by_key(&t, |b| b.time).ok()
}

/// BTC reçus pour `amount` USDT à `price` (déjà glissé), frais déduits du montant.
fn btc_for(amount: f64, fee_rate: f64, price: f64) -> f64 {
    amount * (1.0 - fee_rate) / price
}

/// Simule l'accumulation sur les bougies journalières BTC_USDT `bars` (closes,
/// triées, avec au moins 200 jours avant le premier achat pour la moyenne).
/// `mvrv` : historique du MVRV, ou l'erreur rencontrée en le chargeant. `today`
/// sert à juger la fraîcheur du MVRV.
pub fn accumulate(
    bars: &[Bar],
    mvrv: Result<&MvrvData, &str>,
    params: &AccumulationParams,
    today: NaiveDate,
) -> Result<Accumulation, String> {
    params.validate()?;
    let (Some(first_bar), Some(last_bar)) = (bars.first(), bars.last()) else {
        return Err("Aucune bougie BTC_USDT disponible.".into());
    };
    let (first_day, last_day) = (first_bar.date(), last_bar.date());
    let fee_rate = params.fee_pct / 100.0;
    let slip = params.slippage_pct / 100.0;
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let ma = sma(&closes, MA_DAYS);
    let mvrv_points: &[(NaiveDate, f64)] = match &mvrv {
        Ok(data) if params.weighting.uses_mvrv() => &data.points,
        _ => &[],
    };

    let mut warnings = Vec::new();
    let (mut before_history, mut after_data, mut missing) = (0, 0, 0);
    let mut purchases: Vec<Purchase> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    for date in purchase_dates(params) {
        if date < first_day {
            before_history += 1;
            continue;
        }
        if date > last_day {
            after_data += 1;
            continue;
        }
        let Some(k) = bar_index(bars, date).filter(|&k| k >= 1) else {
            missing += 1;
            continue;
        };
        // Dernière clôture complète : celle de la veille. MVRV daté de la veille au plus tard.
        let prev = &bars[k - 1];
        let yesterday = date - Duration::days(1);
        let ma_ratio = ma[k - 1].map(|m| m / prev.close);
        let mvrv_value = value_on_or_before(mvrv_points, yesterday, STALE_AFTER_DAYS).map(|p| p.1);
        let (w, source) = weight(params, mvrv_value, ma_ratio);
        let amount = params.base_amount * w;
        let open = bars[k].open;
        let price = open * (1.0 + slip);
        purchases.push(Purchase {
            time: bars[k].time,
            date: fmt_time(bars[k].time, Interval::D1),
            open,
            price,
            prev_close: prev.close,
            mvrv: mvrv_value,
            ma_ratio,
            weight: w,
            source,
            amount,
            fee: amount * fee_rate,
            btc: btc_for(amount, fee_rate, price),
        });
        indices.push(k);
    }
    if purchases.is_empty() {
        return Err(format!(
            "Aucun achat à simuler : les bougies BTC_USDT couvrent du {first_day} au {last_day} et {} achat(s) tombent hors de cette période.",
            before_history + after_data + missing
        ));
    }
    let skipped = before_history + after_data + missing;
    if before_history > 0 {
        warnings.push(format!(
            "{before_history} achat(s) prévus avant le début de l'historique Poloniex ({first_day}) ne sont pas simulés."
        ));
    }
    if after_data > 0 {
        warnings.push(format!(
            "{after_data} achat(s) prévus après la dernière bougie complète ({last_day}) ne sont pas simulés : la bougie du jour n'est close qu'à minuit UTC."
        ));
    }
    if missing > 0 {
        warnings.push(format!(
            "{missing} achat(s) sans bougie journalière ou sans clôture de la veille ne sont pas simulés."
        ));
    }

    // Fraîcheur du MVRV et secours.
    let mut mvrv_info = None;
    if params.weighting.uses_mvrv() {
        match &mvrv {
            Err(e) => warnings.push(format!(
                "MVRV indisponible ({e}) : tous les achats sont pondérés par prix / moyenne 200 j."
            )),
            Ok(data) => {
                warnings.extend(data.warnings.iter().cloned());
                mvrv_info = Some(data.info.clone());
                match data.info.latest {
                    None => warnings.push("Aucune donnée MVRV.".into()),
                    Some(latest) => {
                        let age = (today - latest).num_days();
                        if age > STALE_AFTER_DAYS {
                            warnings.push(format!(
                                "Le MVRV le plus récent date du {latest} (il y a {age} jours, plus de {STALE_AFTER_DAYS}) : les achats après cette date sont pondérés par prix / moyenne 200 j."
                            ));
                        }
                    }
                }
            }
        }
    }
    let fallback_purchases = purchases
        .iter()
        .filter(|p| p.source == WeightSource::PriceMa200Fallback)
        .count();
    if fallback_purchases > 0 {
        let first = purchases
            .iter()
            .find(|p| p.source == WeightSource::PriceMa200Fallback)
            .map(|p| p.date.clone())
            .unwrap_or_default();
        warnings.push(format!(
            "{fallback_purchases} achat(s) sur {} pondérés par prix / moyenne 200 j faute de MVRV (premier : {first})."
        , purchases.len()));
    }
    let no_data_purchases = purchases
        .iter()
        .filter(|p| p.source == WeightSource::NoData)
        .count();
    if no_data_purchases > 0 {
        warnings.push(format!(
            "{no_data_purchases} achat(s) au montant de base (×1) : moins de {MA_DAYS} jours de bougies avant la date, la moyenne 200 j n'est pas calculable."
        ));
    }

    // Totaux et comparaisons.
    let n = purchases.len() as f64;
    let spent: f64 = purchases.iter().map(|p| p.amount).sum();
    let fees: f64 = purchases.iter().map(|p| p.fee).sum();
    let btc_bought: f64 = purchases.iter().map(|p| p.btc).sum();
    let equal_amount = spent / n;
    let btc_of = |amount: f64| -> f64 {
        purchases
            .iter()
            .map(|p| btc_for(amount, fee_rate, p.price))
            .sum()
    };
    let btc_equal = btc_of(equal_amount);
    let btc_base = btc_of(params.base_amount);
    let compare = |spent: f64, btc: f64| Comparison {
        spent,
        btc,
        avg_cost: (btc > 0.0).then(|| spent / btc),
        extra_btc: btc_bought - btc,
        extra_pct: (btc > 0.0).then(|| (btc_bought / btc - 1.0) * 100.0),
    };

    // Courbes quotidiennes, du premier achat à la fin.
    let first_k = indices[0];
    let end_ms = date_ms(params.end) + DAY_MS;
    let mut points = Vec::new();
    let (mut cum, mut cum_equal, mut cum_base) =
        (params.initial_btc, params.initial_btc, params.initial_btc);
    let mut next = 0;
    for (k, bar) in bars.iter().enumerate().skip(first_k) {
        if bar.time >= end_ms {
            break;
        }
        while next < purchases.len() && indices[next] == k {
            let p = &purchases[next];
            cum += p.btc;
            cum_equal += btc_for(equal_amount, fee_rate, p.price);
            cum_base += btc_for(params.base_amount, fee_rate, p.price);
            next += 1;
        }
        let known_mvrv = value_on_or_before(mvrv_points, bar.date(), STALE_AFTER_DAYS).map(|p| p.1);
        points.push(AccPoint {
            time: bar.time,
            close: bar.close,
            btc: cum,
            btc_equal_budget: cum_equal,
            btc_fixed_base: cum_base,
            mvrv: known_mvrv,
        });
    }
    let last_close = points.last().map_or(last_bar.close, |p| p.close);
    let btc_total = params.initial_btc + btc_bought;

    let summary = AccumulationSummary {
        first_date: purchases[0].date.clone(),
        last_date: purchases.last().map(|p| p.date.clone()).unwrap_or_default(),
        purchases: purchases.len(),
        skipped,
        spent,
        fees,
        btc_bought,
        initial_btc: params.initial_btc,
        btc_total,
        avg_cost: (btc_bought > 0.0).then(|| spent / btc_bought),
        last_close,
        value_usdt: btc_total * last_close,
        avg_weight: spent / (n * params.base_amount),
        max_weight: purchases.iter().map(|p| p.weight).fold(0.0, f64::max),
        fallback_purchases,
        no_data_purchases,
        equal_budget: compare(spent, btc_equal),
        fixed_base: compare(n * params.base_amount, btc_base),
    };

    Ok(Accumulation {
        params: params.clone(),
        purchases,
        points,
        summary,
        mvrv: mvrv_info,
        warnings,
    })
}

/// Charge les bougies BTC_USDT (et le MVRV si le mode l'utilise) puis simule.
pub async fn run_with_feed(
    feed: &MarketFeed,
    params: &AccumulationParams,
    today: NaiveDate,
) -> Result<Accumulation, String> {
    params.validate()?;
    let btc = Instrument::new("BTC", Market::Spot)?;
    let bars = feed.bars(&btc, Interval::D1, params.data_start()).await?;
    if params.weighting.uses_mvrv() {
        let data = feed.mvrv(today).await;
        accumulate(
            &bars,
            data.as_deref().map_err(String::as_str),
            params,
            today,
        )
    } else {
        let none = MvrvData {
            points: Vec::new(),
            info: MvrvInfo::default(),
            warnings: Vec::new(),
            complete: true,
        };
        accumulate(&bars, Ok(&none), params, today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Bougies journalières dès `start` : ouverture = `opens[i]`, clôture = `closes[i]`.
    fn bars_from(start: &str, opens: &[f64], closes: &[f64]) -> Vec<Bar> {
        let t0 = date_ms(d(start));
        opens
            .iter()
            .zip(closes)
            .enumerate()
            .map(|(i, (&o, &c))| Bar {
                time: t0 + i as i64 * DAY_MS,
                open: o,
                high: o.max(c),
                low: o.min(c),
                close: c,
                volume: 0.0,
            })
            .collect()
    }

    fn flat_bars(start: &str, n: usize, price: f64) -> Vec<Bar> {
        bars_from(start, &vec![price; n], &vec![price; n])
    }

    fn params() -> AccumulationParams {
        AccumulationParams {
            start: d("2024-01-01"),
            end: d("2024-12-31"),
            base_amount: 100.0,
            frequency: Frequency::Weekly,
            weekday: 1,
            month_day: 1,
            weighting: Weighting::MvrvTiers,
            tier_bounds: default_tier_bounds(),
            tier_multipliers: default_tier_multipliers(),
            max_multiplier: 3.0,
            fee_pct: 0.0,
            slippage_pct: 0.0,
            initial_btc: 0.0,
        }
    }

    fn mvrv_data(points: &[(&str, f64)]) -> MvrvData {
        let points: Vec<(NaiveDate, f64)> = points.iter().map(|(s, v)| (d(s), *v)).collect();
        MvrvData {
            info: MvrvInfo {
                latest: points.last().map(|p| p.0),
                ..Default::default()
            },
            points,
            warnings: Vec::new(),
            complete: true,
        }
    }

    #[test]
    fn weekly_and_monthly_schedules() {
        let mut p = params();
        p.start = d("2024-01-01"); // un lundi
        p.end = d("2024-01-29");
        let weekly = purchase_dates(&p);
        assert_eq!(weekly.len(), 5, "les deux bornes (lundis) sont incluses");
        assert_eq!(weekly[0], d("2024-01-01"));
        assert_eq!(weekly[4], d("2024-01-29"));
        p.weekday = 5; // vendredi
        assert_eq!(purchase_dates(&p)[0], d("2024-01-05"));

        p.frequency = Frequency::Monthly;
        p.month_day = 31;
        p.end = d("2024-05-31");
        assert_eq!(
            purchase_dates(&p),
            vec![
                d("2024-01-31"),
                d("2024-02-29"), // bissextile : dernier jour du mois
                d("2024-03-31"),
                d("2024-04-30"),
                d("2024-05-31"),
            ]
        );
        p.month_day = 15;
        assert_eq!(purchase_dates(&p).len(), 5);
    }

    #[test]
    fn tiers_boundaries_are_exclusive_upper_bounds() {
        let p = params();
        for (mvrv, expected) in [
            (0.5, 3.0),
            (0.999, 3.0),
            (1.0, 1.5),
            (1.99, 1.5),
            (2.0, 1.0),
            (2.99, 1.0),
            (3.0, 0.5),
            (4.7, 0.5),
        ] {
            assert_eq!(p.tier_multiplier(mvrv), expected, "MVRV {mvrv}");
        }
        // Paliers modifiés.
        let mut custom = p.clone();
        custom.tier_bounds = vec![1.5];
        custom.tier_multipliers = vec![2.0, 0.0];
        assert_eq!(custom.tier_multiplier(1.49), 2.0);
        assert_eq!(custom.tier_multiplier(1.5), 0.0);
    }

    #[test]
    fn weights_per_mode_and_cap() {
        let mut p = params();
        assert_eq!(weight(&p, Some(0.5), None), (3.0, WeightSource::Mvrv));
        assert_eq!(weight(&p, Some(2.5), None), (1.0, WeightSource::Mvrv));
        p.weighting = Weighting::MvrvInverse;
        assert_eq!(weight(&p, Some(0.5), None), (2.0, WeightSource::Mvrv));
        let (w, _) = weight(&p, Some(4.0), None);
        assert!((w - 0.25).abs() < 1e-12);
        p.max_multiplier = 1.5;
        assert_eq!(weight(&p, Some(0.5), None).0, 1.5, "plafonné");
        p.weighting = Weighting::PriceMa200;
        assert_eq!(
            weight(&p, Some(0.5), Some(1.25)),
            (1.25, WeightSource::PriceMa200)
        );
        assert_eq!(weight(&p, None, Some(2.0)).0, 1.5, "plafonné");
        assert_eq!(weight(&p, None, None), (1.0, WeightSource::NoData));
        p.weighting = Weighting::Fixed;
        assert_eq!(weight(&p, Some(0.1), Some(5.0)), (1.0, WeightSource::Fixed));
        // Secours du MVRV.
        p.weighting = Weighting::MvrvTiers;
        assert_eq!(
            weight(&p, None, Some(0.8)),
            (0.8, WeightSource::PriceMa200Fallback)
        );
        assert_eq!(weight(&p, None, None), (1.0, WeightSource::NoData));
    }

    /// Deux achats (lundis 1er et 8 janvier 2024) à 50 puis 100, sans frais.
    fn two_purchase_setup() -> (Vec<Bar>, AccumulationParams) {
        // Données dès le 29/12/2023 : indices 0 (29/12) … ; 1er janvier = 3, 8 janvier = 10.
        let mut opens = vec![60.0; 14];
        opens[3] = 50.0;
        opens[10] = 100.0;
        let closes = vec![70.0; 14];
        let mut p = params();
        p.start = d("2024-01-01");
        p.end = d("2024-01-10");
        (bars_from("2023-12-29", &opens, &closes), p)
    }

    #[test]
    fn budget_comparisons_by_hand() {
        let (bars, p) = two_purchase_setup();
        // MVRV de la veille : 0,5 le 31/12 (×3), 2,5 le 07/01 (×1).
        let mvrv = mvrv_data(&[("2023-12-31", 0.5), ("2024-01-07", 2.5)]);
        let a = accumulate(&bars, Ok(&mvrv), &p, d("2024-01-09")).unwrap();
        assert_eq!(a.purchases.len(), 2);
        assert_eq!(a.purchases[0].amount, 300.0);
        assert_eq!(a.purchases[1].amount, 100.0);
        let s = &a.summary;
        assert_eq!(s.spent, 400.0);
        assert!((s.btc_bought - (300.0 / 50.0 + 100.0 / 100.0)).abs() < 1e-12); // 7
        assert_eq!(s.avg_cost.map(|c| (c * 1e6).round() / 1e6), Some(57.142857));
        // Budget égal : 400 USDT en 2 × 200, soit 4 + 2 = 6 BTC. Pondéré : 7. Écart : +1 BTC.
        assert!((s.equal_budget.btc - 6.0).abs() < 1e-12);
        assert!((s.equal_budget.extra_btc - 1.0).abs() < 1e-12);
        assert!((s.equal_budget.extra_pct.unwrap() - 100.0 / 6.0).abs() < 1e-9);
        assert_eq!(s.equal_budget.spent, 400.0);
        // Même montant de base : 2 × 100 USDT, soit 2 + 1 = 3 BTC, pour 200 USDT.
        assert!((s.fixed_base.btc - 3.0).abs() < 1e-12);
        assert_eq!(s.fixed_base.spent, 200.0);
        assert!((s.fixed_base.extra_btc - 4.0).abs() < 1e-12);
        assert_eq!(s.avg_weight, 2.0);
        assert_eq!(s.max_weight, 3.0);
        // La dernière valeur des courbes est le total.
        let last = a.points.last().unwrap();
        assert!((last.btc - 7.0).abs() < 1e-12);
        assert!((last.btc_equal_budget - 6.0).abs() < 1e-12);
        assert!((last.btc_fixed_base - 3.0).abs() < 1e-12);
    }

    #[test]
    fn execution_uses_the_open_with_slippage_and_fees_deducted() {
        let (bars, mut p) = two_purchase_setup();
        p.weighting = Weighting::Fixed;
        p.fee_pct = 0.5;
        p.slippage_pct = 1.0;
        p.end = d("2024-01-02");
        let none = mvrv_data(&[]);
        let a = accumulate(&bars, Ok(&none), &p, d("2024-01-09")).unwrap();
        let buy = &a.purchases[0];
        assert_eq!(buy.open, 50.0);
        assert!(
            (buy.price - 50.5).abs() < 1e-12,
            "ouverture + 1 % de glissement"
        );
        assert_eq!(buy.amount, 100.0);
        assert!((buy.fee - 0.5).abs() < 1e-12, "0,5 % de 100");
        assert!(
            (buy.btc - 99.5 / 50.5).abs() < 1e-12,
            "frais déduits du montant"
        );
        assert!((a.summary.fees - 0.5).abs() < 1e-12);
    }

    #[test]
    fn weights_use_only_yesterdays_information() {
        // La clôture et le MVRV du jour d'achat sont des pièges : seuls ceux de la veille comptent.
        let (mut bars, p) = two_purchase_setup();
        bars[3].close = 1.0; // clôture du 1er janvier (jour d'achat)
        let mvrv = mvrv_data(&[
            ("2023-12-30", 5.0), // ancien : remplacé par celui du 31/12
            ("2023-12-31", 0.5), // veille du 1er : ×3
            ("2024-01-01", 5.0), // jour de l'achat : interdit (×0,5 s'il était utilisé)
            ("2024-01-07", 1.5), // veille du 8 : ×1,5
            ("2024-01-08", 0.1), // jour de l'achat : interdit
        ]);
        let a = accumulate(&bars, Ok(&mvrv), &p, d("2024-01-09")).unwrap();
        assert_eq!(a.purchases[0].mvrv, Some(0.5));
        assert_eq!(a.purchases[0].weight, 3.0);
        assert_eq!(a.purchases[1].mvrv, Some(1.5));
        assert_eq!(a.purchases[1].weight, 1.5);
        // Moyenne 200 j : clôture de la veille, pas celle du jour.
        assert_eq!(a.purchases[0].prev_close, 70.0);
        assert_eq!(a.purchases[1].prev_close, 70.0);
    }

    #[test]
    fn price_ma200_weight_is_average_over_last_close() {
        // 250 jours à 100 puis un creux : clôture de la veille à 50.
        let mut closes = vec![100.0; 250];
        closes[249] = 50.0;
        let opens = vec![100.0; 250];
        let bars = bars_from("2023-01-01", &opens, &closes);
        let mut p = params();
        p.weighting = Weighting::PriceMa200;
        p.frequency = Frequency::Monthly;
        p.month_day = 7; // 7 septembre 2023 = indice 249 + 1
        p.start = d("2023-09-07");
        p.end = d("2023-09-07");
        let day = bar_index(&bars, d("2023-09-07"));
        assert_eq!(day, Some(249), "le jour d'achat est le dernier jour");
        let a = accumulate(&bars, Err("sans MVRV"), &p, d("2023-09-08")).unwrap();
        let w = a.purchases[0].weight;
        assert!((w - 1.0).abs() < 1e-12, "veille à 100, moyenne à 100");
        // Même chose avec un creux la veille de l'achat.
        let mut closes = vec![100.0; 250];
        closes[248] = 50.0;
        let bars = bars_from("2023-01-01", &opens, &closes);
        let a = accumulate(&bars, Err("sans MVRV"), &p, d("2023-09-08")).unwrap();
        let ma = (199.0 * 100.0 + 50.0) / 200.0;
        assert!((a.purchases[0].weight - ma / 50.0).abs() < 1e-12);
        assert!(a.purchases[0].weight > 1.9 && a.purchases[0].weight <= 3.0);
    }

    #[test]
    fn stale_mvrv_falls_back_to_price_over_ma200_with_a_warning() {
        // 300 jours de bougies. MVRV disponible jusqu'au 1er septembre 2023 seulement.
        let bars = flat_bars("2023-01-01", 300, 100.0);
        let mut p = params();
        p.start = d("2023-08-14"); // lundi
        p.end = d("2023-10-23"); // dernier lundi de la période
        let mvrv = mvrv_data(&[("2023-08-13", 0.5), ("2023-09-01", 0.5)]);
        let a = accumulate(&bars, Ok(&mvrv), &p, d("2023-10-26")).unwrap();
        // Les lundis jusqu'au 4 septembre (veille = dimanche 3, MVRV du 1er : 2 jours) sont couverts.
        let by_date = |s: &str| a.purchases.iter().find(|p| p.date == s).unwrap();
        assert_eq!(by_date("2023-08-14").source, WeightSource::Mvrv);
        assert_eq!(by_date("2023-09-04").source, WeightSource::Mvrv);
        assert_eq!(by_date("2023-09-04").weight, 3.0);
        // 11 septembre : veille = 10, MVRV du 1er = 9 jours de retard.
        assert_eq!(
            by_date("2023-09-11").source,
            WeightSource::PriceMa200Fallback
        );
        assert!(
            (by_date("2023-09-11").weight - 1.0).abs() < 1e-12,
            "prix = moyenne"
        );
        assert!(a.summary.fallback_purchases > 0);
        assert!(
            a.warnings
                .iter()
                .any(|w| w.contains("2023-09-01") && w.contains("plus de 3")),
            "{:?}",
            a.warnings
        );
        assert!(a.warnings.iter().any(|w| w.contains("faute de MVRV")));
    }

    #[test]
    fn no_mvrv_at_all_uses_the_fallback_everywhere() {
        let bars = flat_bars("2023-01-01", 300, 100.0);
        let mut p = params();
        p.start = d("2023-09-04");
        p.end = d("2023-10-23");
        let a = accumulate(&bars, Err("réseau"), &p, d("2023-10-26")).unwrap();
        assert!(a
            .purchases
            .iter()
            .all(|b| b.source == WeightSource::PriceMa200Fallback));
        assert!(a
            .warnings
            .iter()
            .any(|w| w.contains("MVRV indisponible (réseau)")));
        // Mode fixe : MVRV inutile, pas d'avertissement correspondant.
        p.weighting = Weighting::Fixed;
        let fixed = accumulate(&bars, Err("réseau"), &p, d("2023-10-26")).unwrap();
        assert!(fixed.warnings.is_empty(), "{:?}", fixed.warnings);
        assert!(fixed.mvrv.is_none());
    }

    #[test]
    fn fixed_mode_has_no_edge_over_itself() {
        let (bars, mut p) = two_purchase_setup();
        p.weighting = Weighting::Fixed;
        p.fee_pct = 0.075;
        p.slippage_pct = 0.05;
        let none = mvrv_data(&[]);
        let a = accumulate(&bars, Ok(&none), &p, d("2024-01-09")).unwrap();
        let s = &a.summary;
        assert_eq!(s.avg_weight, 1.0);
        assert!(s.equal_budget.extra_btc.abs() < 1e-12);
        assert!(s.fixed_base.extra_btc.abs() < 1e-12);
        assert_eq!(s.equal_budget.spent, s.spent);
    }

    #[test]
    fn buys_never_sell_and_the_stack_never_shrinks() {
        let opens: Vec<f64> = (0..400)
            .map(|i| 100.0 * (1.0 + 0.5 * (i as f64 / 30.0).sin()))
            .collect();
        let bars = bars_from("2023-01-01", &opens, &opens);
        let mut p = params();
        p.start = d("2023-09-01");
        p.end = d("2024-01-31");
        p.initial_btc = 0.25;
        p.fee_pct = 0.1;
        p.slippage_pct = 0.1;
        p.weighting = Weighting::PriceMa200;
        let a = accumulate(&bars, Err("sans MVRV"), &p, d("2024-02-02")).unwrap();
        assert!(a.purchases.iter().all(|b| b.btc > 0.0 && b.amount > 0.0));
        assert!(a.points.windows(2).all(|w| w[1].btc >= w[0].btc));
        assert!(a
            .points
            .windows(2)
            .all(|w| w[1].btc_equal_budget >= w[0].btc_equal_budget));
        assert!(a.points[0].btc >= 0.25, "le stack initial est inclus");
        let s = &a.summary;
        assert!((s.btc_total - (0.25 + s.btc_bought)).abs() < 1e-12);
        assert!((a.points.last().unwrap().btc - s.btc_total).abs() < 1e-9);
        assert!((s.value_usdt - s.btc_total * s.last_close).abs() < 1e-9);
        // Pas de règle tout ou rien : chaque achat prévu est exécuté.
        assert_eq!(a.purchases.len() + s.skipped, purchase_dates(&p).len());
        assert!(a.purchases.iter().all(|b| b.weight > 0.0));
    }

    #[test]
    fn purchases_without_candles_are_reported_not_simulated() {
        let bars = flat_bars("2024-01-01", 20, 100.0);
        let mut p = params();
        p.weighting = Weighting::Fixed;
        p.start = d("2023-12-18");
        p.end = d("2024-02-05");
        let a = accumulate(&bars, Err("-"), &p, d("2024-02-05")).unwrap();
        // Lundis : 18 et 25 décembre (avant l'historique), 1 et 15 janvier simulés ;
        // 8 janvier aussi ; 22 janvier, 29 janvier, 5 février : après la dernière bougie (20 janvier).
        assert_eq!(
            a.purchases.len(),
            2,
            "1er est sans veille : {:?}",
            a.purchases.iter().map(|p| &p.date).collect::<Vec<_>>()
        );
        assert!(a.summary.skipped >= 5);
        assert!(a.warnings.iter().any(|w| w.contains("avant le début")));
        assert!(a
            .warnings
            .iter()
            .any(|w| w.contains("après la dernière bougie")));
        // Rien du tout : erreur explicite.
        p.start = d("2025-01-01");
        p.end = d("2025-03-01");
        assert!(accumulate(&bars, Err("-"), &p, d("2025-03-01")).is_err());
    }

    #[test]
    fn first_candle_has_no_previous_close_and_early_weights_default_to_one() {
        // 120 jours seulement : pas de moyenne 200 j, achat au montant de base.
        let bars = flat_bars("2024-01-01", 120, 100.0);
        let mut p = params();
        p.weighting = Weighting::PriceMa200;
        p.start = d("2024-02-05");
        p.end = d("2024-03-04");
        let a = accumulate(&bars, Err("-"), &p, d("2024-04-01")).unwrap();
        assert!(a
            .purchases
            .iter()
            .all(|b| b.source == WeightSource::NoData && b.weight == 1.0));
        assert!(a
            .warnings
            .iter()
            .any(|w| w.contains("moyenne 200 j n'est pas calculable")));
    }

    #[test]
    fn invalid_params_are_rejected() {
        let ok = params();
        assert!(ok.validate().is_ok());
        let cases: Vec<AccumulationParams> = vec![
            AccumulationParams {
                base_amount: 0.0,
                ..ok.clone()
            },
            AccumulationParams {
                base_amount: f64::NAN,
                ..ok.clone()
            },
            AccumulationParams {
                weekday: 0,
                ..ok.clone()
            },
            AccumulationParams {
                month_day: 32,
                ..ok.clone()
            },
            AccumulationParams {
                start: d("2025-01-01"),
                ..ok.clone()
            },
            AccumulationParams {
                tier_bounds: vec![2.0, 1.0],
                tier_multipliers: vec![1.0; 3],
                ..ok.clone()
            },
            AccumulationParams {
                tier_multipliers: vec![3.0, 1.0],
                ..ok.clone()
            },
            AccumulationParams {
                tier_multipliers: vec![3.0, -1.0, 1.0, 1.0],
                ..ok.clone()
            },
            AccumulationParams {
                max_multiplier: 0.5,
                ..ok.clone()
            },
            AccumulationParams {
                fee_pct: 5.0,
                ..ok.clone()
            },
            AccumulationParams {
                initial_btc: -1.0,
                ..ok.clone()
            },
        ];
        for c in cases {
            assert!(c.validate().is_err(), "{c:?}");
        }
    }

    #[test]
    fn params_deserialize_from_the_ui_with_defaults() {
        let json = r#"{"start":"2020-01-01","end":"2024-01-01","base_amount":100,
            "frequency":"weekly","weekday":1,"month_day":1,"weighting":"mvrv_tiers",
            "max_multiplier":3,"fee_pct":0.075,"slippage_pct":0.05}"#;
        let p: AccumulationParams = serde_json::from_str(json).unwrap();
        assert_eq!(p.tier_bounds, vec![1.0, 2.0, 3.0]);
        assert_eq!(p.tier_multipliers, vec![3.0, 1.5, 1.0, 0.5]);
        assert_eq!(p.initial_btc, 0.0);
        assert!(p.validate().is_ok());
        assert_eq!(
            serde_json::to_value(Weighting::PriceMa200).unwrap(),
            "price_ma200"
        );
    }

    #[test]
    fn data_start_covers_the_average() {
        let p = params();
        assert_eq!(p.data_start(), p.start - Duration::days(210));
    }

    /// Contrôle sur l'historique réel de Coin Metrics, dont le prix de référence
    /// (`PriceUSD`) remplace ici les bougies Poloniex (inaccessibles hors ligne) :
    /// `COINMETRICS_CSV=btc.csv cargo test -p trading-engine real_history -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_history_with_coinmetrics_prices() {
        use crate::mvrv::{merge, parse_coinmetrics};
        let text =
            std::fs::read_to_string(std::env::var("COINMETRICS_CSV").expect("COINMETRICS_CSV"))
                .unwrap();
        let mvrv = merge(&parse_coinmetrics(&text).unwrap(), &[]);
        let header: Vec<&str> = text.lines().next().unwrap().split(',').collect();
        let (tc, pc) = (
            header.iter().position(|c| *c == "time").unwrap(),
            header.iter().position(|c| *c == "PriceUSD").unwrap(),
        );
        let bars: Vec<Bar> = text
            .lines()
            .skip(1)
            .filter_map(|l| {
                let c: Vec<&str> = l.split(',').collect();
                let date = d(c[tc]);
                let price: f64 = c[pc].parse().ok()?;
                (date >= d("2016-01-01")).then(|| Bar {
                    time: date_ms(date),
                    open: price,
                    high: price,
                    low: price,
                    close: price,
                    volume: 0.0,
                })
            })
            .collect();
        let today = bars.last().unwrap().date() + Duration::days(1);
        for weighting in [
            Weighting::Fixed,
            Weighting::MvrvTiers,
            Weighting::MvrvInverse,
            Weighting::PriceMa200,
        ] {
            for (start, end) in [
                ("2018-01-01", "2026-05-20"),
                ("2023-01-01", "2026-05-20"),
                ("2021-11-01", "2026-05-20"),
            ] {
                let mut p = params();
                p.start = d(start);
                p.end = d(end);
                p.weighting = weighting;
                p.fee_pct = 0.075;
                p.slippage_pct = 0.05;
                let a = accumulate(&bars, Ok(&mvrv), &p, today).unwrap();
                let s = &a.summary;
                println!(
                    "{:<26} {start}→{end} : {} achats, {:.0} USDT, {:.4} BTC, coût {:.0} | budget égal : {:+.4} BTC ({:+.1} %) | base : {:+.4} BTC | poids moyen {:.2} | secours {}",
                    weighting.label(), s.purchases, s.spent, s.btc_bought, s.avg_cost.unwrap(),
                    s.equal_budget.extra_btc, s.equal_budget.extra_pct.unwrap(), s.fixed_base.extra_btc,
                    s.avg_weight, s.fallback_purchases
                );
            }
        }
    }
}

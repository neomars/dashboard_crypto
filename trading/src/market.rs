//! Instruments, intervalles et bougies manipulés par le moteur de trading.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Millisecondes dans une journée.
pub const DAY_MS: i64 = 86_400_000;
/// Millisecondes entre deux paiements de funding (Poloniex : toutes les 8 h).
pub const FUNDING_PERIOD_MS: i64 = 8 * 3_600_000;

/// Marché négocié : comptant (spot) ou contrat perpétuel marginé en USDT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Market {
    Spot,
    Perp,
}

impl Market {
    pub fn label(self) -> &'static str {
        match self {
            Market::Spot => "spot",
            Market::Perp => "perpétuel",
        }
    }

    /// Le spot ne permet que d'être acheteur ou en USDT ; le perpétuel permet la vente à découvert.
    pub fn allows_short(self) -> bool {
        self == Market::Perp
    }
}

/// Durée d'une bougie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Interval {
    #[serde(rename = "HOUR_4")]
    H4,
    #[serde(rename = "DAY_1")]
    D1,
}

impl Interval {
    /// Nom attendu par l'API Poloniex.
    pub fn api_name(self) -> &'static str {
        match self {
            Interval::H4 => "HOUR_4",
            Interval::D1 => "DAY_1",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Interval::H4 => "4 heures",
            Interval::D1 => "1 jour",
        }
    }

    pub fn millis(self) -> i64 {
        match self {
            Interval::H4 => 4 * 3_600_000,
            Interval::D1 => DAY_MS,
        }
    }

    pub fn bars_per_day(self) -> f64 {
        DAY_MS as f64 / self.millis() as f64
    }

    /// Les cryptos cotent en continu : 365 jours par an.
    pub fn bars_per_year(self) -> f64 {
        365.0 * self.bars_per_day()
    }

    /// Nombre de bougies couvrant `days` jours.
    pub fn bars_for_days(self, days: usize) -> usize {
        (days as f64 * self.bars_per_day()).round() as usize
    }
}

/// Actif coté contre USDT sur un marché donné (ex. BTC spot → `BTC_USDT`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Instrument {
    pub base: String,
    pub market: Market,
}

impl Instrument {
    /// Valide le nom de l'actif (lettres et chiffres, il entre dans l'URL de l'API).
    pub fn new(base: &str, market: Market) -> Result<Self, String> {
        let base = base.trim().to_uppercase();
        if base.is_empty() || base.len() > 15 || !base.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!("Actif invalide : « {base} » (ex. BTC, ETH, SOL)."));
        }
        Ok(Self { base, market })
    }

    /// Symbole Poloniex : `BTC_USDT` (spot) ou `BTC_USDT_PERP` (perpétuel).
    pub fn symbol(&self) -> String {
        match self.market {
            Market::Spot => format!("{}_USDT", self.base),
            Market::Perp => format!("{}_USDT_PERP", self.base),
        }
    }

    /// Libellé lisible : `BTC/USDT spot`.
    pub fn label(&self) -> String {
        format!("{}/USDT {}", self.base, self.market.label())
    }
}

/// Bougie : `time` est l'heure d'ouverture (UTC, millisecondes) ; `volume` est
/// exprimé en USDT (montant en devise de cotation), identique pour le spot et
/// le perpétuel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Bar {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Bar {
    pub fn datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.time).unwrap_or_default()
    }

    pub fn date(&self) -> NaiveDate {
        self.datetime().date_naive()
    }

    /// Prix cohérents : finis, positifs, plus bas ≤ ouverture/clôture ≤ plus haut.
    pub fn is_valid(&self) -> bool {
        let prices = [self.open, self.high, self.low, self.close];
        prices.iter().all(|p| p.is_finite() && *p > 0.0)
            && self.low <= self.open.min(self.close)
            && self.high >= self.open.max(self.close)
    }
}

/// Taux de funding payé à `time` (UTC, millisecondes). Positif : les acheteurs
/// (long) paient les vendeurs (short).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FundingRate {
    pub time: i64,
    pub rate: f64,
}

/// Date du jour (UTC).
pub fn today() -> NaiveDate {
    Utc::now().date_naive()
}

/// Minuit UTC du jour `date`, en millisecondes.
pub fn date_ms(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or_default()
}

/// Horodatage lisible par Plotly et dans un CSV : `2024-01-31` pour les bougies
/// journalières, `2024-01-31 04:00` sinon.
pub fn fmt_time(ms: i64, interval: Interval) -> String {
    let dt = DateTime::from_timestamp_millis(ms).unwrap_or_default();
    match interval {
        Interval::D1 => dt.format("%Y-%m-%d").to_string(),
        Interval::H4 => dt.format("%Y-%m-%d %H:%M").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_follow_poloniex_conventions() {
        let spot = Instrument::new(" btc ", Market::Spot).unwrap();
        assert_eq!(spot.symbol(), "BTC_USDT");
        assert_eq!(spot.label(), "BTC/USDT spot");
        let perp = Instrument::new("eth", Market::Perp).unwrap();
        assert_eq!(perp.symbol(), "ETH_USDT_PERP");
    }

    #[test]
    fn invalid_assets_are_rejected() {
        for bad in ["", "BTC/USDT", "../x", "BTC_USDT", "ABCDEFGHIJKLMNOP"] {
            assert!(Instrument::new(bad, Market::Spot).is_err(), "{bad}");
        }
    }

    #[test]
    fn interval_conversions() {
        assert_eq!(Interval::D1.bars_per_year(), 365.0);
        assert_eq!(Interval::H4.bars_per_year(), 2190.0);
        assert_eq!(Interval::H4.bars_for_days(30), 180);
        assert_eq!(Interval::D1.api_name(), "DAY_1");
    }

    #[test]
    fn times_are_formatted_for_charts() {
        let ms = date_ms(NaiveDate::from_ymd_opt(2024, 1, 31).unwrap());
        assert_eq!(fmt_time(ms, Interval::D1), "2024-01-31");
        assert_eq!(
            fmt_time(ms + 4 * 3_600_000, Interval::H4),
            "2024-01-31 04:00"
        );
    }

    #[test]
    fn bar_validity() {
        let bar = Bar {
            time: 0,
            open: 10.0,
            high: 12.0,
            low: 9.0,
            close: 11.0,
            volume: 1.0,
        };
        assert!(bar.is_valid());
        assert!(!Bar { low: 10.5, ..bar }.is_valid());
        assert!(!Bar {
            close: f64::NAN,
            ..bar
        }
        .is_valid());
    }
}

//! Séries temporelles et fenêtres glissantes (équivalents des `rolling` /
//! `ewm` de pandas utilisés par la version Python).

use chrono::{Datelike, Duration, NaiveDate};

/// Bougie journalière (ou hebdomadaire) d'un actif.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Point de prix de clôture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricePoint {
    pub date: NaiveDate,
    pub close: f64,
}

/// Moyenne glissante sur `window` valeurs ; `None` tant que la fenêtre n'est pas pleine.
pub fn rolling_mean(values: &[f64], window: usize) -> Vec<Option<f64>> {
    rolling_mean_min_periods(values, window, window)
}

/// Moyenne glissante acceptant une fenêtre partielle d'au moins `min_periods` valeurs.
pub fn rolling_mean_min_periods(
    values: &[f64],
    window: usize,
    min_periods: usize,
) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= window {
            sum -= values[i - window];
        }
        let count = (i + 1).min(window);
        out.push(if count >= min_periods.max(1) {
            Some(sum / count as f64)
        } else {
            None
        });
    }
    out
}

/// Écart-type glissant (échantillon, ddof = 1 comme pandas) ; ignore les fenêtres
/// contenant une valeur non finie.
pub fn rolling_std(values: &[f64], window: usize) -> Vec<Option<f64>> {
    (0..values.len())
        .map(|i| {
            if window < 2 || i + 1 < window {
                return None;
            }
            let slice = &values[i + 1 - window..=i];
            if slice.iter().any(|v| !v.is_finite()) {
                return None;
            }
            let mean = slice.iter().sum::<f64>() / window as f64;
            let var = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (window - 1) as f64;
            Some(var.sqrt())
        })
        .collect()
}

/// Moyenne mobile exponentielle (`ewm(span, adjust=False)` de pandas).
pub fn ewm_mean(values: &[f64], span: usize) -> Vec<f64> {
    let alpha = 2.0 / (span as f64 + 1.0);
    let mut out = Vec::with_capacity(values.len());
    let mut prev: Option<f64> = None;
    for &v in values {
        let next = match prev {
            None => v,
            Some(p) => alpha * v + (1.0 - alpha) * p,
        };
        out.push(next);
        prev = Some(next);
    }
    out
}

/// Lundi de la semaine contenant `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// Regroupe des bougies journalières en bougies hebdomadaires (semaines
/// commençant le lundi, comme l'intervalle `1wk` de Yahoo Finance).
pub fn weekly_candles(daily: &[Candle]) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::new();
    for c in daily {
        let week = week_start(c.date);
        match out.last_mut() {
            Some(w) if w.date == week => {
                w.high = w.high.max(c.high);
                w.low = w.low.min(c.low);
                w.close = c.close;
                w.volume += c.volume;
            }
            _ => out.push(Candle { date: week, ..*c }),
        }
    }
    out
}

/// Formate une date pour Plotly.
pub fn fmt_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Convertit un `Option<f64>` en valeur JSON (null si absent ou non fini).
pub fn opt_json(v: Option<f64>) -> serde_json::Value {
    match v {
        Some(x) if x.is_finite() => serde_json::json!(x),
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_mean_waits_for_full_window() {
        let r = rolling_mean(&[1.0, 2.0, 3.0, 4.0], 2);
        assert_eq!(r, vec![None, Some(1.5), Some(2.5), Some(3.5)]);
    }

    #[test]
    fn rolling_mean_min_periods_one_starts_immediately() {
        let r = rolling_mean_min_periods(&[2.0, 4.0, 6.0], 2, 1);
        assert_eq!(r, vec![Some(2.0), Some(3.0), Some(5.0)]);
    }

    #[test]
    fn rolling_std_matches_sample_std() {
        let r = rolling_std(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0], 8);
        let expected = (32.0f64 / 7.0).sqrt();
        assert!((r[7].unwrap() - expected).abs() < 1e-12);
        assert!(r[6].is_none());
    }

    #[test]
    fn ewm_matches_pandas_adjust_false() {
        // pandas: Series([1,2,3]).ewm(span=3, adjust=False).mean() -> 1, 1.5, 2.25
        assert_eq!(ewm_mean(&[1.0, 2.0, 3.0], 3), vec![1.0, 1.5, 2.25]);
    }

    #[test]
    fn weekly_candles_aggregate_monday_weeks() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let c = |date, o, h, l, cl| Candle {
            date: d(date),
            open: o,
            high: h,
            low: l,
            close: cl,
            volume: 1.0,
        };
        let daily = vec![
            c("2024-01-07", 1.0, 2.0, 0.5, 1.5), // dimanche -> semaine du 01/01
            c("2024-01-08", 10.0, 12.0, 9.0, 11.0),
            c("2024-01-10", 11.0, 15.0, 8.0, 14.0),
        ];
        let w = weekly_candles(&daily);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].date, d("2024-01-01"));
        assert_eq!(
            w[1],
            Candle {
                date: d("2024-01-08"),
                open: 10.0,
                high: 15.0,
                low: 8.0,
                close: 14.0,
                volume: 2.0
            }
        );
    }
}

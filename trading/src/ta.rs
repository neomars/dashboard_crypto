//! Indicateurs techniques de signal du moteur de trading.
//!
//! Tous sont causaux : la valeur à l'indice `i` n'utilise que les données
//! jusqu'à `i` inclus. `None` signifie « pas encore assez d'historique ».
//! Ils sont distincts des indicateurs de marché du tableau de bord
//! (`dashboard-core`), qui servent à l'analyse et ne déclenchent aucun ordre.

use crate::market::Bar;

/// Moyenne mobile simple sur `n` valeurs.
pub fn sma(values: &[f64], n: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for (i, v) in values.iter().enumerate() {
        sum += v;
        if n > 0 && i >= n {
            sum -= values[i - n];
        }
        out.push((n > 0 && i + 1 >= n).then(|| sum / n as f64));
    }
    out
}

/// Moyenne mobile exponentielle (α = 2 / (n + 1)), amorcée par la moyenne
/// simple des `n` premières valeurs (convention TA-Lib) : pas de valeur avant
/// `n` bougies.
pub fn ema(values: &[f64], n: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if n == 0 || values.len() < n {
        return out;
    }
    let alpha = 2.0 / (n as f64 + 1.0);
    let mut prev = values[..n].iter().sum::<f64>() / n as f64;
    out[n - 1] = Some(prev);
    for i in n..values.len() {
        prev = alpha * values[i] + (1.0 - alpha) * prev;
        out[i] = Some(prev);
    }
    out
}

/// Vrai range : max(haut − bas, |haut − clôture précédente|, |bas − clôture précédente|).
pub fn true_range(bars: &[Bar]) -> Vec<f64> {
    bars.iter()
        .enumerate()
        .map(|(i, b)| match i.checked_sub(1).map(|p| bars[p].close) {
            Some(prev) => (b.high - b.low)
                .max((b.high - prev).abs())
                .max((b.low - prev).abs()),
            None => b.high - b.low,
        })
        .collect()
}

/// Average True Range lissé à la manière de Wilder sur `n` bougies.
pub fn atr(bars: &[Bar], n: usize) -> Vec<Option<f64>> {
    let tr = true_range(bars);
    let mut out = vec![None; tr.len()];
    if n == 0 || tr.len() < n {
        return out;
    }
    let mut prev = tr[..n].iter().sum::<f64>() / n as f64;
    out[n - 1] = Some(prev);
    for i in n..tr.len() {
        prev = (prev * (n - 1) as f64 + tr[i]) / n as f64;
        out[i] = Some(prev);
    }
    out
}

/// Plus haute valeur des `n` dernières, indice courant inclus.
pub fn highest(values: &[f64], n: usize) -> Vec<Option<f64>> {
    window(values, n, f64::max)
}

/// Plus basse valeur des `n` dernières, indice courant inclus.
pub fn lowest(values: &[f64], n: usize) -> Vec<Option<f64>> {
    window(values, n, f64::min)
}

fn window(values: &[f64], n: usize, pick: fn(f64, f64) -> f64) -> Vec<Option<f64>> {
    (0..values.len())
        .map(|i| {
            (n > 0 && i + 1 >= n).then(|| {
                values[i + 1 - n..=i]
                    .iter()
                    .copied()
                    .reduce(pick)
                    .unwrap_or(f64::NAN)
            })
        })
        .collect()
}

/// Décale d'une bougie : `out[i] = v[i − 1]`. Sert aux canaux de Donchian,
/// comparés à la clôture courante sans l'inclure.
pub fn previous(v: &[Option<f64>]) -> Vec<Option<f64>> {
    std::iter::once(None)
        .chain(v.iter().copied())
        .take(v.len())
        .collect()
}

/// Rendements logarithmiques de clôture à clôture (aucun pour la première bougie).
pub fn log_returns(closes: &[f64]) -> Vec<Option<f64>> {
    std::iter::once(None)
        .chain(closes.windows(2).map(|w| Some((w[1] / w[0]).ln())))
        .collect()
}

/// Volatilité réalisée annualisée (fraction : 0,5 = 50 %) : écart-type
/// (échantillon) des `n` derniers rendements logarithmiques × √(bougies par an).
pub fn realized_vol(closes: &[f64], n: usize, bars_per_year: f64) -> Vec<Option<f64>> {
    let returns = log_returns(closes);
    (0..closes.len())
        .map(|i| {
            if n < 2 || i < n {
                return None;
            }
            let slice: Vec<f64> = returns[i + 1 - n..=i].iter().flatten().copied().collect();
            if slice.len() < n || slice.iter().any(|r| !r.is_finite()) {
                return None;
            }
            let mean = slice.iter().sum::<f64>() / n as f64;
            let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
            Some(var.sqrt() * bars_per_year.sqrt())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar {
            time: 0,
            open,
            high,
            low,
            close,
            volume: 0.0,
        }
    }

    fn approx(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    #[test]
    fn sma_waits_for_a_full_window() {
        assert_eq!(
            sma(&[1.0, 2.0, 3.0, 4.0], 2),
            vec![None, Some(1.5), Some(2.5), Some(3.5)]
        );
    }

    #[test]
    fn ema_is_seeded_with_the_sma() {
        // n = 3, α = 0,5 : amorce (1 + 2 + 3) / 3 = 2, puis 0,5 × 4 + 0,5 × 2 = 3.
        let e = ema(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert_eq!(e[..2], [None, None]);
        assert!(approx(e[2], 2.0) && approx(e[3], 3.0) && approx(e[4], 4.0));
        assert!(ema(&[1.0], 3).iter().all(Option::is_none));
    }

    #[test]
    fn true_range_includes_gaps() {
        let bars = [bar(10.0, 11.0, 9.0, 10.0), bar(13.0, 14.0, 12.5, 13.5)];
        // Écart d'ouverture : |14 − 10| = 4 dépasse haut − bas = 1,5.
        assert_eq!(true_range(&bars), vec![2.0, 4.0]);
    }

    #[test]
    fn atr_uses_wilder_smoothing() {
        let bars: Vec<Bar> = [2.0, 4.0, 6.0, 8.0]
            .iter()
            .map(|r| bar(100.0, 100.0 + r / 2.0, 100.0 - r / 2.0, 100.0))
            .collect();
        let a = atr(&bars, 2);
        assert_eq!(a[0], None);
        assert!(approx(a[1], 3.0)); // (2 + 4) / 2
        assert!(approx(a[2], 4.5)); // (3 × 1 + 6) / 2
        assert!(approx(a[3], 6.25)); // (4,5 × 1 + 8) / 2
    }

    #[test]
    fn highest_and_lowest_include_the_current_value() {
        let v = [3.0, 1.0, 4.0, 1.0, 5.0];
        assert_eq!(
            highest(&v, 3),
            vec![None, None, Some(4.0), Some(4.0), Some(5.0)]
        );
        assert_eq!(
            lowest(&v, 3),
            vec![None, None, Some(1.0), Some(1.0), Some(1.0)]
        );
        assert_eq!(
            previous(&highest(&v, 3)),
            vec![None, None, None, Some(4.0), Some(4.0)]
        );
    }

    #[test]
    fn realized_vol_of_constant_growth_is_zero() {
        let closes: Vec<f64> = (0..40).map(|i| 100.0 * 1.01f64.powi(i)).collect();
        let v = realized_vol(&closes, 30, 365.0);
        assert!(v[29].is_none());
        assert!(v[30].is_some_and(|x| x.abs() < 1e-9));
    }

    #[test]
    fn realized_vol_matches_hand_computation() {
        // Rendements log alternés ±r : écart-type échantillon = r × √(n / (n − 1)).
        let r = 0.02f64;
        let mut closes = vec![100.0];
        for i in 0..10 {
            let last = *closes.last().unwrap();
            closes.push(last * if i % 2 == 0 { r.exp() } else { (-r).exp() });
        }
        let v = realized_vol(&closes, 10, 365.0);
        let expected = r * (10.0f64 / 9.0).sqrt() * 365f64.sqrt();
        assert!(approx(v[10], expected), "{:?}", v[10]);
    }
}

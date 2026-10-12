//! Mesures de performance d'une courbe de capital.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Performance {
    pub final_equity: f64,
    pub total_return_pct: f64,
    /// Rendement annualisé composé ; absent sur moins d'un jour.
    pub cagr_pct: Option<f64>,
    /// Volatilité annualisée des rendements par bougie.
    pub volatility_pct: Option<f64>,
    /// Rendement moyen / écart-type, annualisé, taux sans risque nul.
    pub sharpe: Option<f64>,
    /// Comme le Sharpe, mais rapporté à la seule volatilité des baisses.
    pub sortino: Option<f64>,
    /// Plus forte baisse depuis un plus haut (négative, en %).
    pub max_drawdown_pct: f64,
    pub max_drawdown_time: Option<i64>,
    /// CAGR / |drawdown max|.
    pub calmar: Option<f64>,
}

/// Performance d'une courbe de capital `equity` (une valeur par bougie, à la
/// clôture) partant de `initial`, avec `bars_per_year` bougies par an.
pub fn performance(times: &[i64], equity: &[f64], initial: f64, bars_per_year: f64) -> Performance {
    let final_equity = equity.last().copied().unwrap_or(initial);
    let returns: Vec<f64> = std::iter::once(initial)
        .chain(equity.iter().copied())
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| if w[0] > 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
        .collect();
    let n = returns.len() as f64;
    let years = n / bars_per_year;

    let cagr = (years * 365.0 >= 1.0 && initial > 0.0)
        .then(|| ((final_equity.max(0.0) / initial).powf(1.0 / years) - 1.0) * 100.0);
    let mean = returns.iter().sum::<f64>() / n.max(1.0);
    let std = (returns.len() >= 2)
        .then(|| (returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt());
    let downside = (returns.len() >= 2)
        .then(|| (returns.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / n).sqrt());
    let ann = bars_per_year.sqrt();
    // Sous 1e-12, l'écart-type n'est que du bruit d'arrondi : ratio non défini.
    let ratio = |d: Option<f64>| d.filter(|d| *d > 1e-12).map(|d| mean / d * ann);

    let mut peak = initial;
    let (mut max_dd, mut max_dd_time) = (0.0f64, None);
    for (t, e) in times.iter().zip(equity) {
        peak = peak.max(*e);
        let dd = if peak > 0.0 { e / peak - 1.0 } else { 0.0 };
        if dd < max_dd {
            max_dd = dd;
            max_dd_time = Some(*t);
        }
    }
    let max_drawdown_pct = max_dd * 100.0;

    Performance {
        final_equity,
        total_return_pct: if initial > 0.0 {
            (final_equity / initial - 1.0) * 100.0
        } else {
            0.0
        },
        cagr_pct: cagr,
        volatility_pct: std.map(|s| s * ann * 100.0),
        sharpe: ratio(std),
        sortino: ratio(downside),
        max_drawdown_pct,
        max_drawdown_time: max_dd_time,
        calmar: cagr
            .filter(|_| max_drawdown_pct < 0.0)
            .map(|c| c / max_drawdown_pct.abs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_growth_has_no_drawdown() {
        // 365 bougies journalières à +0,1 % : CAGR = 1,001^365 − 1.
        let equity: Vec<f64> = (1..=365).map(|i| 100.0 * 1.001f64.powi(i)).collect();
        let times: Vec<i64> = (0..365).collect();
        let p = performance(&times, &equity, 100.0, 365.0);
        let expected = (1.001f64.powi(365) - 1.0) * 100.0;
        assert!((p.cagr_pct.unwrap() - expected).abs() < 1e-9);
        assert!((p.total_return_pct - expected).abs() < 1e-9);
        assert_eq!(p.max_drawdown_pct, 0.0);
        assert_eq!(p.calmar, None);
        assert_eq!(p.sharpe, None, "volatilité nulle : Sharpe non défini");
    }

    #[test]
    fn drawdown_is_measured_from_the_peak() {
        let equity = [110.0, 120.0, 90.0, 100.0, 130.0];
        let p = performance(&[1, 2, 3, 4, 5], &equity, 100.0, 365.0);
        assert!((p.max_drawdown_pct + 25.0).abs() < 1e-9, "120 → 90");
        assert_eq!(p.max_drawdown_time, Some(3));
        assert!((p.total_return_pct - 30.0).abs() < 1e-9);
        assert!(p.sharpe.unwrap() > 0.0);
        assert!(p.sortino.unwrap() > p.sharpe.unwrap(), "peu de baisses");
    }

    #[test]
    fn short_history_has_no_cagr() {
        let p = performance(&[], &[], 100.0, 365.0);
        assert_eq!(p.cagr_pct, None);
        assert_eq!(p.final_equity, 100.0);
        assert_eq!(p.total_return_pct, 0.0);
    }
}

//! Net Realized Profit / Loss hebdomadaire (style Glassnode « Profit Taking »).
//!
//! Chaque semaine, une bulle verte (profit net) ou rouge (perte nette) est
//! placée sur le prix BTC, avec une taille proportionnelle au montant net
//! réalisé. Données : NRPL journalier de BGeometrics, cumulé par semaine.

use super::{IndicatorOutput, Level};
use crate::bgeometrics;
use crate::data::DataProvider;
use crate::figure::Figure;
use crate::series::{fmt_date, week_start, Candle};
use chrono::NaiveDate;
use serde_json::json;
use std::collections::BTreeMap;

const MIN_BUBBLE: f64 = 6.0;
const MAX_BUBBLE: f64 = 40.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyPnl {
    pub week: NaiveDate,
    pub net: f64,
}

/// Somme des valeurs journalières par semaine (commençant le lundi).
pub fn weekly_net_realized(daily: &[(NaiveDate, f64)]) -> Vec<WeeklyPnl> {
    let mut weeks: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for (date, value) in daily {
        *weeks.entry(week_start(*date)).or_default() += value;
    }
    weeks
        .into_iter()
        .map(|(week, net)| WeeklyPnl { week, net })
        .collect()
}

/// Taille de bulle proportionnelle à la racine de |valeur| (aire ∝ montant).
pub fn bubble_sizes(values: &[f64], min_size: f64, max_size: f64) -> Vec<f64> {
    let mag: Vec<f64> = values.iter().map(|v| v.abs().sqrt()).collect();
    let peak = mag.iter().cloned().fold(0.0, f64::max);
    if peak == 0.0 {
        return vec![min_size; values.len()];
    }
    mag.iter()
        .map(|m| min_size + (max_size - min_size) * m / peak)
        .collect()
}

/// Montant lisible, en dollars ou en bitcoins selon l'unité de la série.
fn format_amount(value: f64, btc: bool) -> String {
    let abs = value.abs();
    let unit = if btc { " BTC" } else { " $" };
    for (threshold, suffix) in [(1e9, " Md"), (1e6, " M"), (1e3, " k")] {
        if abs >= threshold {
            return format!("{:.2}{suffix}{unit}", value / threshold);
        }
    }
    format!("{value:.0}{unit}")
}

pub(crate) fn build_figure(weekly: &[WeeklyPnl], daily: &[Candle], btc_unit: bool) -> Figure {
    // Clôture de fin de semaine (dernier jour disponible), indexée par le lundi.
    let mut week_close: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for c in daily {
        week_close.insert(week_start(c.date), c.close);
    }
    let points: Vec<(&WeeklyPnl, f64)> = weekly
        .iter()
        .filter_map(|w| Some((w, *week_close.get(&w.week)?)))
        .collect();
    let sizes = bubble_sizes(
        &points.iter().map(|(w, _)| w.net).collect::<Vec<_>>(),
        MIN_BUBBLE,
        MAX_BUBBLE,
    );

    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin : Net Realized Profit / Loss hebdomadaire"},
        "height": 700, "hovermode": "closest",
        "yaxis": {"title": {"text": "Prix BTC (USD)"}}, "xaxis": {"title": {"text": "Date"}},
        "legend": {"orientation": "h", "y": 1.05, "x": 0}
    }));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC",
        "x": daily.iter().map(|c| fmt_date(c.date)).collect::<Vec<_>>(), "y": daily.iter().map(|c| c.close).collect::<Vec<_>>(),
        "line": {"color": "rgba(255, 255, 255, 0.85)", "width": 1.5},
        "hovertemplate": "%{x|%d/%m/%Y}<br>%{y:,.0f} $<extra></extra>"}));

    for (label, positive, fill, line) in [
        (
            "Net realized profit (semaine)",
            true,
            "rgba(46, 204, 113, 0.55)",
            "rgba(46, 204, 113, 1)",
        ),
        (
            "Net realized loss (semaine)",
            false,
            "rgba(231, 76, 60, 0.55)",
            "rgba(231, 76, 60, 1)",
        ),
    ] {
        let idx: Vec<usize> = (0..points.len())
            .filter(|&i| (points[i].0.net >= 0.0) == positive)
            .collect();
        let text: Vec<String> = idx
            .iter()
            .map(|&i| {
                let (w, price) = points[i];
                format!(
                    "Semaine du {}<br>Prix : {:.0} $<br><b>Net réalisé : {}</b>",
                    w.week.format("%d/%m/%Y"),
                    price,
                    format_amount(w.net, btc_unit)
                )
            })
            .collect();
        fig.trace(json!({"type": "scatter", "mode": "markers", "name": label,
            "x": idx.iter().map(|&i| fmt_date(points[i].0.week)).collect::<Vec<_>>(),
            "y": idx.iter().map(|&i| points[i].1).collect::<Vec<_>>(),
            "marker": {"size": idx.iter().map(|&i| sizes[i]).collect::<Vec<_>>(), "color": fill, "line": {"width": 1, "color": line}},
            "text": text, "hovertemplate": "%{text}<extra></extra>"}));
    }
    fig
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let nrpl = data.bgeometrics("nrpl").await?;
    let (column, daily_net) = bgeometrics::single_series(&nrpl.rows)?;
    let weekly = weekly_net_realized(&daily_net);
    let start = weekly.first().ok_or("Aucune donnée NRPL.")?.week;
    let prices = data
        .ticker_range("BTC-USD", Some(start), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;
    let btc_unit = column.to_lowercase().contains("btc");
    let out = IndicatorOutput::from(build_figure(&weekly, &prices, btc_unit));
    Ok(match nrpl.stale_notice() {
        Some(n) => out.with_notice(Level::Warning, n),
        None => out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

    #[test]
    fn daily_values_are_summed_per_monday_week() {
        let daily = [
            (d("2024-01-01"), 100.0),
            (d("2024-01-07"), -30.0),
            (d("2024-01-08"), -40.0),
        ];
        assert_eq!(
            weekly_net_realized(&daily),
            vec![
                WeeklyPnl {
                    week: d("2024-01-01"),
                    net: 70.0
                },
                WeeklyPnl {
                    week: d("2024-01-08"),
                    net: -40.0
                }
            ]
        );
    }

    #[test]
    fn bubble_sizes_scale_with_magnitude() {
        assert_eq!(
            bubble_sizes(&[0.0, 100.0, -400.0], 5.0, 25.0),
            vec![5.0, 15.0, 25.0]
        );
        assert_eq!(bubble_sizes(&[0.0, 0.0], 5.0, 25.0), vec![5.0, 5.0]);
    }

    #[test]
    fn figure_colours_bubbles_by_sign() {
        let weekly = vec![
            WeeklyPnl {
                week: d("2024-01-01"),
                net: 10.0,
            },
            WeeklyPnl {
                week: d("2024-01-08"),
                net: -5.0,
            },
        ];
        let daily: Vec<Candle> = (0..14)
            .map(|i| {
                let c = (i + 1) as f64;
                Candle {
                    date: d("2024-01-01") + chrono::Duration::days(i),
                    open: c,
                    high: c,
                    low: c,
                    close: c,
                    volume: 0.0,
                }
            })
            .collect();
        let fig = build_figure(&weekly, &daily, false);
        assert_eq!(fig.data[1]["x"], json!(["2024-01-01"]));
        assert_eq!(fig.data[1]["y"], json!([7.0]), "clôture du dimanche");
        assert_eq!(fig.data[2]["y"], json!([14.0]));
    }

    #[test]
    fn amount_formatting() {
        assert_eq!(format_amount(2_500_000_000.0, false), "2.50 Md $");
        assert_eq!(format_amount(-1_500_000.0, false), "-1.50 M $");
        assert_eq!(format_amount(12.0, false), "12 $");
        assert_eq!(format_amount(-2_500.0, true), "-2.50 k BTC");
    }
}

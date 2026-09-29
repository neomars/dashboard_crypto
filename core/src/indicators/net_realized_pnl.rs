//! Net Realized Profit / Loss hebdomadaire (style Glassnode « Profit Taking »).
//!
//! Chaque semaine, une bulle verte (profit net) ou rouge (perte nette) est
//! placée sur le prix BTC, avec une taille proportionnelle au montant net
//! réalisé. Les données viennent d'une requête Dune (SQL de référence :
//! `dune_queries/net_realized_pnl.sql`) dont l'ID se configure sous la clé
//! `net_realized_pnl`.

use super::{IndicatorOutput, Level};
use crate::data::DataProvider;
use crate::dune::{self, Row};
use crate::figure::Figure;
use crate::series::{fmt_date, week_start, Candle};
use chrono::NaiveDate;
use serde_json::json;
use std::collections::BTreeMap;

const TIME_COLUMNS: [&str; 5] = ["week", "time", "date", "day", "block_time"];
const MIN_BUBBLE: f64 = 6.0;
const MAX_BUBBLE: f64 = 40.0;

pub const NOT_CONFIGURED: &str = "Aucune requête Dune configurée pour cet indicateur. Créez une requête sur dune.com avec le SQL \
de dune_queries/net_realized_pnl.sql, puis renseignez son ID dans la configuration (onglet Accueil) ou dans config.ini \
(section [DUNE_QUERIES], clé net_realized_pnl), ou via la variable d'environnement DUNE_QUERY_NET_REALIZED_PNL.";

#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyPnl {
    pub week: NaiveDate,
    pub profit: f64,
    pub loss: f64,
}

impl WeeklyPnl {
    pub fn net(&self) -> f64 {
        self.profit - self.loss
    }
}

/// Regroupe les lignes Dune par semaine (commençant le lundi).
///
/// Accepte soit des colonnes profit/perte séparées (perte signée ou non), soit
/// une seule colonne nette.
pub fn weekly_net_realized(rows: &[Row]) -> Result<Vec<WeeklyPnl>, String> {
    let cols = dune::columns(rows);
    let time_col = TIME_COLUMNS
        .iter()
        .find(|c| cols.iter().any(|k| k == *c))
        .ok_or_else(|| format!("Aucune colonne temporelle trouvée. Colonnes dispos : {cols:?}"))?;
    let lower = |c: &String| c.to_lowercase();
    let profit_col = cols.iter().find(|c| {
        let l = lower(c);
        l.contains("profit") && !l.contains("loss") && !l.contains("net")
    });
    let loss_col = cols.iter().find(|c| {
        let l = lower(c);
        l.contains("loss") && !l.contains("profit") && !l.contains("net")
    });
    let net_col = cols.iter().find(|c| lower(c).contains("net"));

    let mut weeks: BTreeMap<NaiveDate, (f64, f64)> = BTreeMap::new();
    for row in rows {
        let Some(date) = dune::date(row, time_col) else {
            continue;
        };
        let (profit, loss) = match (profit_col, loss_col, net_col) {
            (Some(p), Some(l), _) => (
                dune::num(row, p).unwrap_or(0.0),
                dune::num(row, l).unwrap_or(0.0).abs(),
            ),
            (_, _, Some(n)) => {
                let net = dune::num(row, n).unwrap_or(0.0);
                (net.max(0.0), (-net).max(0.0))
            }
            _ => {
                return Err(format!(
                    "Colonnes profit/perte introuvables. Colonnes dispos : {cols:?}"
                ))
            }
        };
        let e = weeks.entry(week_start(date)).or_default();
        e.0 += profit;
        e.1 += loss;
    }
    Ok(weeks
        .into_iter()
        .map(|(week, (profit, loss))| WeeklyPnl { week, profit, loss })
        .collect())
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

fn format_usd(value: f64) -> String {
    let abs = value.abs();
    for (threshold, suffix) in [(1e9, "Md"), (1e6, "M"), (1e3, "k")] {
        if abs >= threshold {
            return format!("{:.2} {suffix}$", value / threshold);
        }
    }
    format!("{value:.0} $")
}

pub(crate) fn build_figure(weekly: &[WeeklyPnl], daily: &[Candle]) -> Figure {
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
        &points.iter().map(|(w, _)| w.net()).collect::<Vec<_>>(),
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
            .filter(|&i| (points[i].0.net() >= 0.0) == positive)
            .collect();
        let text: Vec<String> = idx.iter().map(|&i| {
            let (w, price) = points[i];
            format!("Semaine du {}<br>Prix : {:.0} $<br>Profit réalisé : {}<br>Perte réalisée : {}<br><b>Net : {}</b>",
                w.week.format("%d/%m/%Y"), price, format_usd(w.profit), format_usd(w.loss), format_usd(w.net()))
        }).collect();
        fig.trace(json!({"type": "scatter", "mode": "markers", "name": label,
            "x": idx.iter().map(|&i| fmt_date(points[i].0.week)).collect::<Vec<_>>(),
            "y": idx.iter().map(|&i| points[i].1).collect::<Vec<_>>(),
            "marker": {"size": idx.iter().map(|&i| sizes[i]).collect::<Vec<_>>(), "color": fill, "line": {"width": 1, "color": line}},
            "text": text, "hovertemplate": "%{text}<extra></extra>"}));
    }
    fig
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let query_id = data.config().dune_query_id("net_realized_pnl", "");
    if query_id.is_empty() {
        return Err(NOT_CONFIGURED.into());
    }
    let rows = data.dune_results(&query_id).await?;
    if rows.is_empty() {
        return Err("Aucune donnée retournée par la requête Dune.".into());
    }
    let weekly = weekly_net_realized(&rows)?;
    let start = weekly
        .first()
        .ok_or("Aucune date valide dans les données Dune.")?
        .week;
    let daily = data
        .ticker_range("BTC-USD", Some(start), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;
    let out = IndicatorOutput::from(build_figure(&weekly, &daily));
    Ok(if weekly.len() < 4 {
        out.with_notice(
            Level::Warning,
            "Moins de 4 semaines de données : vérifiez la requête Dune.",
        )
    } else {
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;
    use serde_json::json;

    fn rows(v: serde_json::Value) -> Vec<Row> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_object().unwrap().clone())
            .collect()
    }

    #[test]
    fn weekly_aggregation_from_profit_and_loss_columns() {
        let r = rows(json!([
            {"day": "2024-01-01", "realized_profit_usd": 100.0, "realized_loss_usd": -20.0},
            {"day": "2024-01-03", "realized_profit_usd": 50.0, "realized_loss_usd": -30.0},
            {"day": "2024-01-08", "realized_profit_usd": 10.0, "realized_loss_usd": 40.0}
        ]));
        let w = weekly_net_realized(&r).unwrap();
        assert_eq!(
            w,
            vec![
                WeeklyPnl {
                    week: d("2024-01-01"),
                    profit: 150.0,
                    loss: 50.0
                },
                WeeklyPnl {
                    week: d("2024-01-08"),
                    profit: 10.0,
                    loss: 40.0
                },
            ]
        );
        assert_eq!(w[1].net(), -30.0);
    }

    #[test]
    fn single_net_column_is_split() {
        let r = rows(json!([
            {"week": "2024-01-01 00:00:00.000 UTC", "net_realized_pnl": 25.0},
            {"week": "2024-01-08 00:00:00.000 UTC", "net_realized_pnl": "-5"}
        ]));
        let w = weekly_net_realized(&r).unwrap();
        assert_eq!(
            (w[0].profit, w[0].loss, w[1].profit, w[1].loss),
            (25.0, 0.0, 0.0, 5.0)
        );
    }

    #[test]
    fn missing_columns_are_explained() {
        assert!(weekly_net_realized(&rows(json!([{"profit": 1}])))
            .unwrap_err()
            .contains("temporelle"));
        assert!(
            weekly_net_realized(&rows(json!([{"week": "2024-01-01", "foo": 1}])))
                .unwrap_err()
                .contains("profit/perte")
        );
    }

    #[test]
    fn bubble_sizes_scale_with_magnitude() {
        let s = bubble_sizes(&[0.0, 100.0, -400.0], 5.0, 25.0);
        assert_eq!(s, vec![5.0, 15.0, 25.0]);
        assert_eq!(bubble_sizes(&[0.0, 0.0], 5.0, 25.0), vec![5.0, 5.0]);
    }

    #[test]
    fn figure_colours_bubbles_by_sign() {
        let weekly = vec![
            WeeklyPnl {
                week: d("2024-01-01"),
                profit: 10.0,
                loss: 0.0,
            },
            WeeklyPnl {
                week: d("2024-01-08"),
                profit: 0.0,
                loss: 5.0,
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
        let fig = build_figure(&weekly, &daily);
        assert_eq!(fig.data[1]["x"], json!(["2024-01-01"]));
        assert_eq!(fig.data[1]["y"], json!([7.0]), "clôture du dimanche");
        assert_eq!(fig.data[2]["y"], json!([14.0]));
    }

    #[test]
    fn usd_formatting() {
        assert_eq!(format_usd(2_500_000_000.0), "2.50 Md$");
        assert_eq!(format_usd(-1_500_000.0), "-1.50 M$");
        assert_eq!(format_usd(12.0), "12 $");
    }
}

use super::IndicatorOutput;
use crate::data::{halving_dates, DataProvider, KNOWN_HALVINGS};
use crate::figure::{dates, Figure};
use crate::series::{fmt_date, PricePoint};
use chrono::{Duration, NaiveDate};
use serde_json::json;

/// Sommets et creux de chaque cycle entre deux halvings :
/// - top : plus haut entre le halving et « prochain halving - 150 jours » ;
/// - bottom : plus bas entre « halving + 600 jours » et le prochain halving.
pub(crate) fn tops_and_bottoms(
    prices: &[PricePoint],
    halvings: &[NaiveDate],
) -> (Vec<PricePoint>, Vec<PricePoint>) {
    let mut tops = Vec::new();
    let mut bottoms = Vec::new();
    for w in halvings.windows(2) {
        let (start, next) = (w[0], w[1]);
        let top_end = next - Duration::days(150);
        let top = prices
            .iter()
            .filter(|p| p.date >= start && p.date < top_end)
            .max_by(|a, b| a.close.total_cmp(&b.close));
        tops.extend(top.copied());
        let bottom_start = start + Duration::days(600);
        let bottom = prices
            .iter()
            .filter(|p| p.date >= bottom_start && p.date < next)
            .min_by(|a, b| a.close.total_cmp(&b.close));
        bottoms.extend(bottom.copied());
    }
    (tops, bottoms)
}

/// Jours écoulés depuis le dernier halving et restant avant le suivant.
pub(crate) fn days_info(date: NaiveDate, halvings: &[NaiveDate]) -> (i64, i64) {
    let after = halvings
        .iter()
        .rev()
        .find(|h| **h <= date)
        .map(|h| (date - *h).num_days())
        .unwrap_or(0);
    let before = halvings
        .iter()
        .find(|h| **h > date)
        .map(|h| (*h - date).num_days())
        .unwrap_or(0);
    (after, before)
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let estimated = data.estimate_next_halving().await;
    let mut halvings = halving_dates();
    halvings.push(estimated);

    let btc = data.ticker_history("BTC-USD").await?;
    let prices: Vec<PricePoint> = btc
        .iter()
        .map(|c| PricePoint {
            date: c.date,
            close: c.close,
        })
        .collect();
    let first = prices
        .first()
        .map(|p| p.date)
        .ok_or("Historique BTC vide.")?;
    let info: Vec<[i64; 2]> = prices
        .iter()
        .map(|p| {
            let (a, b) = days_info(p.date, &halvings);
            [a, b]
        })
        .collect();

    let mut fig = Figure::new(json!({
        "title": {"text": "BTC Price, Halvings & Tops de Cycles"},
        "xaxis": {"title": {"text": "Date"}},
        "yaxis": {"title": {"text": "BTC Price (USD)"}, "type": "log"},
        "height": 800, "hovermode": "x unified"
    }));
    fig.trace(json!({
        "type": "scatter", "mode": "lines", "name": "BTC Price",
        "x": dates(prices.iter().map(|p| p.date)), "y": prices.iter().map(|p| p.close).collect::<Vec<_>>(),
        "line": {"color": "#FF9900", "width": 2}, "customdata": info,
        "hovertemplate": "<b>Date: %{x|%d %b %Y}</b><br>Prix: $%{y:,.0f}<br>Jours après halving: %{customdata[0]}<br>Jours avant prochain: %{customdata[1]}<br><extra></extra>"
    }));

    for h in KNOWN_HALVINGS
        .iter()
        .filter(|h| crate::data::parse_ymd(h.date) >= first)
    {
        fig.vline(h.date, "red", 1.0, "dash", "paper");
        fig.annotation(json!({"x": h.date, "y": 1, "yref": "paper", "yanchor": "top", "text": format!("Halving {}", h.date),
            "showarrow": false, "font": {"color": "red", "size": 10}}));
    }

    let (tops, bottoms) = tops_and_bottoms(&prices, &halvings);
    for top in &tops {
        fig.trace(json!({
            "type": "scatter", "mode": "markers+text", "name": "Cycle Top", "showlegend": false,
            "x": [fmt_date(top.date)], "y": [top.close], "textposition": "top center",
            "text": [format!("Top: {}j après", days_info(top.date, &halvings).0)],
            "marker": {"color": "white", "size": 14, "symbol": "star", "line": {"color": "black", "width": 1}}
        }));
    }
    for bottom in &bottoms {
        fig.trace(json!({
            "type": "scatter", "mode": "markers+text", "name": "Cycle Bottom", "showlegend": false,
            "x": [fmt_date(bottom.date)], "y": [bottom.close], "textposition": "bottom center",
            "text": [format!("Bottom: {}j après", days_info(bottom.date, &halvings).0)],
            "marker": {"color": "red", "size": 12, "symbol": "x"}
        }));
    }

    let est = fmt_date(estimated);
    fig.vline(&est, "lime", 2.0, "dot", "paper");
    fig.annotation(json!({"x": est, "y": 0.9, "yref": "paper", "yanchor": "top", "text": format!("Prochain Halving<br>~{est}"),
        "bgcolor": "lime", "font": {"color": "black", "size": 10}, "showarrow": true}));
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

    #[test]
    fn days_info_counts_both_ways() {
        let h = [d("2020-05-11"), d("2024-04-20")];
        assert_eq!(days_info(d("2020-05-21"), &h), (10, 1430));
        assert_eq!(days_info(d("2019-01-01"), &h), (0, 496));
    }

    #[test]
    fn top_window_excludes_last_150_days() {
        let h = [d("2020-01-01"), d("2022-01-01")];
        let prices = vec![
            PricePoint {
                date: d("2020-06-01"),
                close: 10.0,
            },
            PricePoint {
                date: d("2021-10-01"),
                close: 50.0,
            }, // < 150 j avant le halving : exclu du top
            PricePoint {
                date: d("2021-09-01"),
                close: 5.0,
            }, // après halving + 600 j : candidat bottom
        ];
        let (tops, bottoms) = tops_and_bottoms(&prices, &h);
        assert_eq!(tops[0].close, 10.0);
        assert_eq!(bottoms[0].close, 5.0);
    }
}

use super::IndicatorOutput;
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use chrono::NaiveDate;
use serde_json::json;
use std::collections::HashMap;

fn color(fg: f64) -> String {
    let r = (255.0 * (1.0 - fg / 100.0)) as i64;
    let g = (255.0 * (fg / 100.0)) as i64;
    format!("rgb({r},{g},0)")
}

/// Valeur Fear & Greed de chaque jour : valeur du jour, sinon la dernière
/// connue, 50 (neutre) avant le début de l'indice.
pub(crate) fn align(days: &[NaiveDate], fg: &[(NaiveDate, f64)]) -> Vec<f64> {
    let by_day: HashMap<_, _> = fg.iter().copied().collect();
    let mut last = None;
    days.iter()
        .map(|d| {
            if let Some(v) = by_day.get(d) {
                last = Some(*v);
            }
            last.unwrap_or(50.0)
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let fg = data.fear_greed().await?;
    let btc = data.ticker_history("BTC-USD").await?;
    let days: Vec<NaiveDate> = btc.iter().map(|c| c.date).collect();
    let closes: Vec<f64> = btc.iter().map(|c| c.close).collect();
    let values = align(&days, &fg);
    let x = dates(days.iter().copied());

    let mut fig = Figure::new(json!({
        "title": {"text": "BTC Price (depuis 2010) — Coloré par Fear & Greed Index"},
        "xaxis": {"title": {"text": "Date"}},
        "yaxis": {"title": {"text": "BTC Price (USD)"}},
        "hovermode": "x unified", "height": 720, "showlegend": false
    }));

    // Une trace par série de jours consécutifs dans la même tranche de 5 points
    // (une trace par jour serait beaucoup trop lente à afficher).
    let bucket = |v: f64| (v / 5.0).floor() * 5.0;
    let mut start = 0;
    while start < values.len() {
        let b = bucket(values[start]);
        let mut end = start + 1;
        while end < values.len() && bucket(values[end]) == b {
            end += 1;
        }
        let seg_end = (end + 1).min(values.len()); // relie visuellement les segments
        fig.trace(json!({
            "type": "scatter", "mode": "lines", "x": &x[start..seg_end], "y": &closes[start..seg_end],
            "line": {"color": color(b), "width": 3}, "hoverinfo": "skip"
        }));
        start = end;
    }

    fig.trace(json!({
        "type": "scatter", "mode": "markers", "x": x, "y": closes,
        "marker": {"size": 0.1, "color": "rgba(0,0,0,0)"}, "customdata": values,
        "hovertemplate": "<b>%{x|%Y-%m-%d}</b><br>BTC: $%{y:,.0f}<br>Fear & Greed: %{customdata:.0f}/100<br><extra></extra>"
    }));
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

    #[test]
    fn align_forward_fills_and_defaults_to_neutral() {
        let days = [
            d("2018-01-30"),
            d("2018-02-01"),
            d("2018-02-02"),
            d("2018-02-03"),
        ];
        let fg = [(d("2018-02-01"), 30.0), (d("2018-02-03"), 70.0)];
        assert_eq!(align(&days, &fg), vec![50.0, 30.0, 30.0, 70.0]);
    }

    #[test]
    fn color_goes_from_red_to_green() {
        assert_eq!(color(0.0), "rgb(255,0,0)");
        assert_eq!(color(100.0), "rgb(0,255,0)");
    }
}

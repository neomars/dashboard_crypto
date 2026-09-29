use super::{IndicatorOutput, Level};
use crate::bgeometrics;
use crate::data::DataProvider;
use crate::figure::{dates, merge, two_rows, Figure};
use crate::series::opt_json;
use crate::table::{self, Row};
use chrono::NaiveDate;
use serde_json::json;
use std::collections::BTreeMap;

/// Séries à empiler : une par ETF si la réponse les détaille (en écartant
/// alors un éventuel total), sinon la seule série disponible.
pub(crate) fn holding_columns(rows: &[Row]) -> Vec<String> {
    let cols = bgeometrics::value_columns(rows);
    let detailed: Vec<String> = cols
        .iter()
        .filter(|c| !c.to_lowercase().contains("total"))
        .cloned()
        .collect();
    if detailed.len() > 1 {
        detailed
    } else {
        cols
    }
}

/// Dernière valeur connue à chaque date de `days` (0 avant la première).
pub(crate) fn align_holdings(series: &BTreeMap<NaiveDate, f64>, days: &[NaiveDate]) -> Vec<f64> {
    days.iter()
        .map(|d| {
            series
                .range(..=d)
                .next_back()
                .map(|(_, v)| *v)
                .unwrap_or(0.0)
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let etf = data.bgeometrics("etf").await?;
    let cols = holding_columns(&etf.rows);
    if cols.is_empty() {
        return Err(format!(
            "Aucune valeur numérique dans les données ETF. Colonnes : {:?}",
            table::columns(&etf.rows)
        ));
    }
    let rows = bgeometrics::dated_rows(&etf.rows);
    let start = rows
        .first()
        .ok_or("Aucune date valide dans les données ETF.")?
        .0;
    let btc = data
        .ticker_range("BTC-USD", Some(start), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;
    let days: Vec<NaiveDate> = btc.iter().map(|c| c.date).collect();
    let x = dates(days.iter().copied());

    let mut layout = two_rows(
        [0.65, 0.35],
        0.08,
        Some([
            "Prix du Bitcoin (USD)",
            "Bitcoin détenus par les ETF spot (BTC)",
        ]),
    );
    merge(
        &mut layout,
        json!({
            "title": {"text": "Bitcoin - Holdings des ETF vs Prix"},
            "xaxis2": {"title": {"text": "Date"}},
            "yaxis": {"title": {"text": "Prix BTC (USD)"}, "type": "log"},
            "yaxis2": {"title": {"text": "Holdings (BTC)"}},
            "height": 900, "hovermode": "x unified",
            "legend": {"orientation": "v", "yanchor": "top", "y": 1, "xanchor": "left", "x": 1.02},
            "margin": {"r": 150}
        }),
    );
    let mut fig = Figure::new(layout);
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x,
        "y": btc.iter().map(|c| c.close).collect::<Vec<_>>(), "line": {"color": "#00CCFF", "width": 2}}));
    for col in &cols {
        let series: BTreeMap<NaiveDate, f64> = rows
            .iter()
            .filter_map(|(d, r)| Some((*d, table::num(r, col)?)))
            .collect();
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": col, "x": x,
            "y": align_holdings(&series, &days).into_iter().map(|v| opt_json(Some(v))).collect::<Vec<_>>(),
            "xaxis": "x2", "yaxis": "y2", "stackgroup": "one", "line": {"width": 0.5}, "hovertemplate": "%{y:,.0f} BTC"}));
    }
    let out = IndicatorOutput::from(fig);
    Ok(match etf.stale_notice() {
        Some(n) => out.with_notice(Level::Warning, n),
        None => out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;
    use serde_json::json;

    #[test]
    fn holdings_forward_fill_and_start_at_zero() {
        let s: BTreeMap<_, _> = [(d("2024-01-02"), 10.0), (d("2024-01-04"), 30.0)]
            .into_iter()
            .collect();
        let days = [
            d("2024-01-01"),
            d("2024-01-02"),
            d("2024-01-03"),
            d("2024-01-05"),
        ];
        assert_eq!(align_holdings(&s, &days), vec![0.0, 10.0, 10.0, 30.0]);
    }

    #[test]
    fn per_etf_columns_exclude_total() {
        let rows: Vec<Row> = vec![
            json!({"d": "2024-01-11", "unixTs": 1, "ibit": 10, "fbtc": 5, "etfTotal": 15})
                .as_object()
                .unwrap()
                .clone(),
        ];
        assert_eq!(holding_columns(&rows), vec!["ibit", "fbtc"]);
        let total_only: Vec<Row> = vec![json!({"d": "2024-01-11", "unixTs": 1, "etfBtcTotal": 15})
            .as_object()
            .unwrap()
            .clone()];
        assert_eq!(holding_columns(&total_only), vec!["etfBtcTotal"]);
    }
}

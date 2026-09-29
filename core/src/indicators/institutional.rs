use super::IndicatorOutput;
use crate::data::DataProvider;
use crate::dune;
use crate::figure::{dates, merge, two_rows, Figure};
use chrono::NaiveDate;
use serde_json::json;
use std::collections::BTreeMap;

/// Avoirs par émetteur, alignés sur `days` : dernière valeur connue à cette
/// date (0 avant la première).
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
    let query_id = data.config().dune_query_id("institutional", "3382000");
    let rows = data.dune_results(&query_id).await?;
    if rows.is_empty() {
        return Err("Aucune donnée retournée par la requête Dune.".into());
    }
    let time_col = dune::find_column(&rows, &["time", "date", "block_time", "day"]);
    let ticker_col = dune::find_column(&rows, &["etf_ticker", "ticker", "symbol"]);
    let val_col = dune::find_column(&rows, &["tvl", "holding", "btc_held", "amount"]);
    let (Some(time_col), Some(ticker_col), Some(val_col)) = (time_col, ticker_col, val_col) else {
        return Err(format!(
            "Structure de données Dune inattendue. Colonnes : {:?}",
            dune::columns(&rows)
        ));
    };

    // Pivot : une série par émetteur
    let mut by_ticker: BTreeMap<String, BTreeMap<NaiveDate, f64>> = BTreeMap::new();
    for r in rows.iter() {
        if let (Some(d), Some(v)) = (dune::date(r, &time_col), dune::num(r, &val_col)) {
            by_ticker
                .entry(dune::value_as_string(&r[&ticker_col]))
                .or_default()
                .insert(d, v);
        }
    }
    let start = by_ticker
        .values()
        .filter_map(|s| s.keys().next())
        .min()
        .copied()
        .ok_or("Aucune date valide dans les données Dune.")?;
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
            "Breakdown des Holdings Institutionnels (BTC)",
        ]),
    );
    merge(
        &mut layout,
        json!({
            "title": {"text": "Bitcoin - Holdings Institutionnels Détaillés vs Prix"},
            "xaxis2": {"title": {"text": "Date"}},
            "yaxis": {"title": {"text": "Prix BTC (USD)"}, "type": "log"},
            "yaxis2": {"title": {"text": "Holdings (BTC)"}},
            "height": 900, "hovermode": "x unified",
            "legend": {"orientation": "v", "yanchor": "top", "y": 1, "xanchor": "left", "x": 1.02},
            "margin": {"r": 150}
        }),
    );
    let mut fig = Figure::new(layout);
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x, "y": btc.iter().map(|c| c.close).collect::<Vec<_>>(),
        "line": {"color": "#00CCFF", "width": 2}}));
    for (ticker, series) in &by_ticker {
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": ticker, "x": x, "y": align_holdings(series, &days),
            "xaxis": "x2", "yaxis": "y2", "stackgroup": "one", "line": {"width": 0.5}, "hovertemplate": "%{y:,.0f} BTC"}));
    }
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

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
}

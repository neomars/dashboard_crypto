use super::{close_by_date, IndicatorOutput};
use crate::data::DataProvider;
use crate::dune::{self, Row};
use crate::figure::{dates, Figure};
use crate::series::opt_json;
use chrono::NaiveDate;
use serde_json::json;

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let query_id = data.config().dune_query_id("sopr", "6764134");
    let rows = data.dune_results(&query_id).await?;
    if rows.is_empty() {
        return Err("Aucune donnée retournée par la requête Dune.".into());
    }
    let cols = dune::columns(&rows);
    let time_col = dune::find_column(&rows, &["time", "block_time", "date", "day"])
        .ok_or_else(|| format!("Impossible de trouver une colonne temporelle dans les données. Colonnes dispos : {cols:?}"))?;
    let sopr_cols: Vec<String> = cols
        .iter()
        .filter(|c| c.to_lowercase().contains("sopr"))
        .cloned()
        .collect();
    if sopr_cols.is_empty() {
        return Err(format!(
            "Aucune colonne SOPR détectée. Colonnes dispos : {cols:?}"
        ));
    }

    let mut rows: Vec<(NaiveDate, &Row)> = rows
        .iter()
        .filter_map(|r| Some((dune::date(r, &time_col)?, r)))
        .collect();
    rows.sort_by_key(|(d, _)| *d);
    let start = rows
        .first()
        .ok_or("Aucune date valide dans les données Dune.")?
        .0;
    let btc = data
        .ticker_range("BTC-USD", Some(start), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;
    let price = close_by_date(&btc);

    let x = dates(rows.iter().map(|(d, _)| *d));
    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin - STH-SOPR (Short Term Holder Output Profit Ratio)"},
        "xaxis": {"title": {"text": "Date"}},
        "yaxis": {"title": {"text": "STH-SOPR"}, "type": "log", "showgrid": true, "gridcolor": "rgba(128, 128, 128, 0.2)"},
        "yaxis2": {"title": {"text": "Prix BTC (USD)"}, "type": "log", "overlaying": "y", "side": "right", "showgrid": false},
        "height": 700, "hovermode": "x unified", "legend": {"x": 0.01, "y": 0.99}
    }));
    let colors = ["#00FFAA", "#FF00AA", "#AAFF00", "#00AAFF"];
    for (i, col) in sopr_cols.iter().enumerate() {
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": col.to_uppercase().replace('_', " "), "x": x,
            "y": rows.iter().map(|(_, r)| opt_json(dune::num(r, col))).collect::<Vec<_>>(),
            "line": {"color": colors[i % colors.len()], "width": 2}}));
    }
    fig.trace(
        json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x, "yaxis": "y2",
        "y": rows.iter().map(|(d, _)| opt_json(price.get(d).copied())).collect::<Vec<_>>(),
        "line": {"color": "rgba(255, 255, 255, 0.4)", "width": 1.5}}),
    );
    fig.hline(1.0, "orange", "dash", "y", "paper");
    Ok(fig.into())
}

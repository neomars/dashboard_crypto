use super::{param_str, IndicatorOutput};
use crate::data::DataProvider;
use crate::dune::{self, Row};
use crate::figure::{dates, Figure};
use crate::series::opt_json;
use chrono::NaiveDate;
use serde_json::{json, Value};

pub const PAIRS: [&str; 6] = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"];

fn is_position_col(c: &str, side: &str) -> bool {
    c.contains(side) && (c.contains("oi") || c.contains("size") || c.contains("position"))
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let pair = param_str(params, "pair", "BTC").to_uppercase();
    if !PAIRS.contains(&pair.as_str()) {
        return Err(format!("Paire inconnue : {pair}"));
    }
    let mode = param_str(params, "mode", "Long vs Short");

    let query_id = data.config().dune_query_id("long_short", "3089944");
    let all = data.dune_results(&query_id).await?;
    if all.is_empty() {
        return Err(
            "Aucune donnée retournée par Dune. Vérifiez votre Query ID ou vos paramètres.".into(),
        );
    }
    let date_col = dune::find_column_by(&all, |c| ["date", "block_time", "time"].contains(&c));
    let asset_col = dune::find_column_by(&all, |c| {
        ["asset", "symbol", "pair", "market", "token"]
            .iter()
            .any(|s| c.contains(s))
    });
    let rows: Vec<&Row> = match &asset_col {
        Some(col) => all
            .iter()
            .filter(|r| {
                dune::value_as_string(&r[col])
                    .to_uppercase()
                    .contains(&pair)
            })
            .collect(),
        None => all.iter().collect(),
    };
    if rows.is_empty() {
        return Err(format!(
            "Aucune donnée trouvée pour la paire {pair} dans les résultats Dune."
        ));
    }
    let cols = dune::columns(&all);
    let long_col = cols
        .iter()
        .find(|c| is_position_col(&c.to_lowercase(), "long"));
    let short_col = cols
        .iter()
        .find(|c| is_position_col(&c.to_lowercase(), "short"));
    let (Some(date_col), Some(long_col), Some(short_col)) = (date_col, long_col, short_col) else {
        return Err(format!("Colonnes Long/Short introuvables. Vérifiez la structure des données Dune. Colonnes disponibles : {cols:?}"));
    };

    let mut points: Vec<(NaiveDate, f64, f64)> = rows
        .iter()
        .filter_map(|r| {
            Some((
                dune::date(r, &date_col)?,
                dune::num(r, long_col)?,
                dune::num(r, short_col)?,
            ))
        })
        .collect();
    points.sort_by_key(|p| p.0);
    let start = points
        .first()
        .ok_or("Aucune ligne exploitable dans les résultats Dune.")?
        .0;
    let x = dates(points.iter().map(|p| p.0));
    let longs: Vec<f64> = points.iter().map(|p| p.1).collect();
    let shorts: Vec<f64> = points.iter().map(|p| p.2).collect();

    let mut fig = Figure::new(json!({
        "height": 650, "hovermode": "x unified",
        "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1},
        "yaxis": {"title": {"text": "Open Interest / Ratio"}, "side": "left"},
        "yaxis2": {"title": {"text": format!("Prix {pair} (USD)")}, "overlaying": "y", "side": "right", "showgrid": false},
        "margin": {"t": 80, "b": 100}
    }));
    let title = match mode {
        "Ratio Long/Short" => {
            let ratio: Vec<Value> = points
                .iter()
                .map(|p| opt_json(if p.2 != 0.0 { Some(p.1 / p.2) } else { None }))
                .collect();
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Ratio Long/Short", "x": x, "y": ratio, "line": {"color": "#FFD600", "width": 2.5}}));
            fig.hline(1.0, "white", "dash", "y", "paper");
            fig.annotation(json!({"xref": "paper", "yref": "y", "x": 1, "y": 1.0, "xanchor": "right", "yanchor": "bottom", "text": "Équilibre", "showarrow": false}));
            format!("{pair} - Ratio Long/Short")
        }
        "Open Interest Cumulé" => {
            let cum = |v: &[f64]| {
                v.iter()
                    .scan(0.0, |s, x| {
                        *s += x;
                        Some(*s)
                    })
                    .collect::<Vec<f64>>()
            };
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Cumulative Long", "x": x, "y": cum(&longs),
                "line": {"color": "#00C853", "width": 2.5}, "fill": "tozeroy", "fillcolor": "rgba(0,200,83,0.1)"}));
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Cumulative Short", "x": x, "y": cum(&shorts),
                "line": {"color": "#FF1744", "width": 2.5}, "fill": "tozeroy", "fillcolor": "rgba(255,23,68,0.1)"}));
            format!("{pair} - Volume OI Cumulé")
        }
        _ => {
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("Long OI ({long_col})"), "x": x, "y": longs, "line": {"color": "#00C853", "width": 2.5}}));
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("Short OI ({short_col})"), "x": x, "y": shorts, "line": {"color": "#FF1744", "width": 2.5}}));
            format!("{pair} - Positions Ouvertes (GMX V2)")
        }
    };
    fig.layout["title"] = json!({"text": title});

    // Prix de l'actif superposé (dernier prix connu à chaque date)
    if let Ok(prices) = data
        .ticker_range(&format!("{pair}-USD"), Some(start), None)
        .await
    {
        let by_day: std::collections::BTreeMap<NaiveDate, f64> =
            prices.iter().map(|c| (c.date, c.close)).collect();
        let y: Vec<Value> = points
            .iter()
            .map(|p| opt_json(by_day.range(..=p.0).next_back().map(|(_, v)| *v)))
            .collect();
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("Prix {pair} (USD)"), "x": x, "y": y, "yaxis": "y2",
            "line": {"color": "#00CCFF", "width": 1.5, "dash": "dot"}}));
    }
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::is_position_col;

    #[test]
    fn detects_position_columns() {
        assert!(is_position_col("long_oi_usd", "long"));
        assert!(is_position_col("short_position_size", "short"));
        assert!(!is_position_col("long_count", "long"));
    }
}

use super::{IndicatorOutput, Level};
use crate::data::DataProvider;
use crate::dune::{self, Row, MISSING_KEY};
use crate::figure::{dates, Figure};
use crate::series::opt_json;
use chrono::NaiveDate;
use serde_json::json;
use std::sync::Arc;

const BANDS: [(&str, &str); 13] = [
    ("10y+", "#004d33"),
    ("7y-10y", "#0f5a3f"),
    ("5y-7y", "#1f694e"),
    ("3y-5y", "#2e7c5e"),
    ("2y-3y", "#3d8f6e"),
    ("18m-2y", "#4a9c8f"),
    ("12m-18m", "#6b4fa0"),
    ("6m-12m", "#c4458f"),
    ("3m-6m", "#e6b87d"),
    ("1m-3m", "#c38c5f"),
    ("1w-1m", "#9c6644"),
    ("1d-1w", "#808080"),
    ("0d-1d", "#666666"),
];

/// Colonnes à empiler : les bandes d'âge connues, sinon toutes les colonnes
/// numériques autres que la date.
pub(crate) fn band_columns(rows: &[Row], date_col: &str) -> Vec<String> {
    let cols = dune::columns(rows);
    let known: Vec<String> = BANDS
        .iter()
        .map(|(b, _)| b.to_string())
        .filter(|b| cols.contains(b))
        .collect();
    if !known.is_empty() {
        return known;
    }
    cols.into_iter()
        .filter(|c| c != date_col && rows.iter().take(20).any(|r| dune::num(r, c).is_some()))
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    // Requête légère : Bitcoin UTXO Age Bands (mensuelle) - https://dune.com/queries/7611528
    let query_id = data.config().dune_query_id("realized_cap_utxo", "7611528");
    let mut notice = None;
    let rows: Arc<Vec<Row>> = match data.dune_results(&query_id).await {
        Ok(rows) if !rows.is_empty() => rows,
        Err(e) if e == MISSING_KEY => return Err(e),
        _ => {
            notice =
                Some("Aucun résultat en cache sur Dune : la requête a été exécutée (30 à 60 s).");
            data.dune_execute(&query_id).await?
        }
    };
    let date_col =
        dune::find_column(&rows, &["date", "month", "time", "block_time"]).ok_or_else(|| {
            format!(
                "Aucune colonne de date. Colonnes : {:?}",
                dune::columns(&rows)
            )
        })?;
    let bands = band_columns(&rows, &date_col);
    if bands.is_empty() {
        return Err(format!(
            "Aucune bande d'âge dans les données. Colonnes : {:?}",
            dune::columns(&rows)
        ));
    }
    let mut rows: Vec<(NaiveDate, &Row)> = rows
        .iter()
        .filter_map(|r| Some((dune::date(r, &date_col)?, r)))
        .collect();
    rows.sort_by_key(|(d, _)| *d);
    let x = dates(rows.iter().map(|(d, _)| *d));

    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin: Realized Cap - UTXO Age Bands (%)"},
        "height": 750, "hovermode": "x unified",
        "legend": {"orientation": "h", "y": -0.15, "x": 0.5, "xanchor": "center"},
        "yaxis": {"title": {"text": "Pourcentage du Realized Cap (%)"}, "range": [0, 100], "ticksuffix": "%"},
        "xaxis": {"title": {"text": "Date"}}, "margin": {"t": 50, "b": 100}
    }));
    for band in &bands {
        let color = BANDS
            .iter()
            .find(|(b, _)| b == band)
            .map(|(_, c)| *c)
            .unwrap_or("#888888");
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": band, "x": x, "stackgroup": "one", "fillcolor": color,
            "y": rows.iter().map(|(_, r)| opt_json(dune::num(r, band))).collect::<Vec<_>>(),
            "line": {"width": 0.5, "color": "rgba(255,255,255,0.2)"}}));
    }
    let out = IndicatorOutput::from(fig);
    Ok(match notice {
        Some(n) => out.with_notice(Level::Info, n),
        None => out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(v: serde_json::Value) -> Vec<Row> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_object().unwrap().clone())
            .collect()
    }

    #[test]
    fn prefers_known_bands_in_display_order() {
        let r = rows(json!([{"month": "2024-01-01", "0d-1d": 1, "10y+": 5, "other": 3}]));
        assert_eq!(band_columns(&r, "month"), vec!["10y+", "0d-1d"]);
    }

    #[test]
    fn falls_back_to_numeric_columns() {
        let r =
            rows(json!([{"month": "2024-01-01", "lt_1y": "40.5", "gt_1y": 59.5, "label": "x"}]));
        assert_eq!(band_columns(&r, "month"), vec!["lt_1y", "gt_1y"]);
    }
}

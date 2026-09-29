//! Lignes de données tabulaires au format JSON (`{"colonne": valeur}`) et
//! lecture souple des colonnes : les API ne nomment pas toujours leurs champs
//! de la même façon.

use crate::data::parse_flexible_date;
use chrono::NaiveDate;
use serde_json::{Map, Value};

pub type Row = Map<String, Value>;

/// Noms de colonnes (ordre de la première ligne, puis colonnes additionnelles).
pub fn columns(rows: &[Row]) -> Vec<String> {
    let mut cols: Vec<String> = Vec::new();
    for row in rows.iter().take(50) {
        for k in row.keys() {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
        }
    }
    cols
}

/// Colonnes numériques (au moins une valeur numérique dans les premières
/// lignes), hors colonnes exclues.
pub fn numeric_columns(rows: &[Row], exclude: &[&str]) -> Vec<String> {
    columns(rows)
        .into_iter()
        .filter(|c| !exclude.contains(&c.as_str()))
        .filter(|c| rows.iter().take(50).any(|r| num(r, c).is_some()))
        .collect()
}

pub fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

pub fn num(row: &Row, col: &str) -> Option<f64> {
    row.get(col).and_then(value_as_f64)
}

pub fn date(row: &Row, col: &str) -> Option<NaiveDate> {
    row.get(col)
        .and_then(Value::as_str)
        .and_then(parse_flexible_date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn column_helpers() {
        let rows: Vec<Row> = vec![
            json!({"d": "2024-01-01", "unixTs": 1704067200, "sthSopr": "1.02", "x": null})
                .as_object()
                .unwrap()
                .clone(),
        ];
        assert_eq!(numeric_columns(&rows, &["d", "unixTs"]), vec!["sthSopr"]);
        assert_eq!(num(&rows[0], "sthSopr"), Some(1.02));
        assert_eq!(num(&rows[0], "x"), None);
        assert_eq!(date(&rows[0], "d"), NaiveDate::from_ymd_opt(2024, 1, 1));
    }
}

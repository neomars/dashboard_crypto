//! Réponses Dune Analytics : gestion d'erreur unifiée et lecture souple des
//! colonnes (les requêtes publiques n'ont pas toutes les mêmes noms).

use crate::data::parse_flexible_date;
use chrono::NaiveDate;
use serde_json::{Map, Value};

pub type Row = Map<String, Value>;

pub const MISSING_KEY: &str =
    "Clé API Dune manquante. Configurez-la dans l'onglet Accueil ou dans config.ini.";

/// Traduit la réponse de `GET /query/{id}/results` en lignes ou en message d'erreur.
pub fn parse_results_response(status: u16, body: &str, query_id: &str) -> Result<Vec<Row>, String> {
    match status {
        200 => {}
        401 => return Err("Erreur API Dune : 401 (Non autorisé). Vérifiez votre clé API dans config.ini ou l'onglet Accueil.".into()),
        404 => return Err(format!("Erreur API Dune : 404 (Non trouvé). La requête avec l'ID {query_id} n'existe pas ou est privée.")),
        400 => {
            let details = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| body.to_string());
            return Err(format!("Erreur API Dune : 400 (Requête invalide). Détails : {details}"));
        }
        other => return Err(format!("Erreur API Dune : {other}")),
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("Erreur lors du traitement des données Dune : {e}"))?;
    rows_from_json(&v)
}

pub fn rows_from_json(v: &Value) -> Result<Vec<Row>, String> {
    let rows = v["result"]["rows"]
        .as_array()
        .ok_or("Erreur lors du traitement des données Dune : champ result.rows absent.")?;
    Ok(rows.iter().filter_map(|r| r.as_object().cloned()).collect())
}

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

/// Première colonne existante parmi `candidates` (dans l'ordre des candidats).
pub fn find_column(rows: &[Row], candidates: &[&str]) -> Option<String> {
    let cols = columns(rows);
    candidates
        .iter()
        .find(|c| cols.iter().any(|k| k == *c))
        .map(|c| c.to_string())
}

/// Première colonne (dans l'ordre des données) dont le nom en minuscules vérifie `pred`.
pub fn find_column_by(rows: &[Row], pred: impl Fn(&str) -> bool) -> Option<String> {
    columns(rows).into_iter().find(|c| pred(&c.to_lowercase()))
}

pub fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

pub fn value_as_date(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(parse_flexible_date)
}

pub fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn num(row: &Row, col: &str) -> Option<f64> {
    row.get(col).and_then(value_as_f64)
}

pub fn date(row: &Row, col: &str) -> Option<NaiveDate> {
    row.get(col).and_then(value_as_date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_returns_rows() {
        let rows =
            parse_results_response(200, r#"{"result":{"rows":[{"a":1},{"a":2}]}}"#, "1").unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn http_errors_are_explained() {
        assert!(parse_results_response(401, "", "1")
            .unwrap_err()
            .contains("401"));
        assert!(parse_results_response(404, "", "123")
            .unwrap_err()
            .contains("123"));
        assert!(
            parse_results_response(400, r#"{"error":"colonne invalide"}"#, "1")
                .unwrap_err()
                .contains("colonne invalide")
        );
        assert!(parse_results_response(500, "", "1")
            .unwrap_err()
            .contains("500"));
    }

    #[test]
    fn column_helpers() {
        let rows: Vec<Row> = vec![
            json!({"day": "2024-01-01 00:00:00.000 UTC", "sth_sopr": "1.02", "x": null})
                .as_object()
                .unwrap()
                .clone(),
        ];
        assert_eq!(find_column(&rows, &["time", "day"]).as_deref(), Some("day"));
        assert_eq!(
            find_column_by(&rows, |c| c.contains("sopr")).as_deref(),
            Some("sth_sopr")
        );
        assert_eq!(num(&rows[0], "sth_sopr"), Some(1.02));
        assert_eq!(num(&rows[0], "x"), None);
        assert_eq!(date(&rows[0], "day"), NaiveDate::from_ymd_opt(2024, 1, 1));
    }
}

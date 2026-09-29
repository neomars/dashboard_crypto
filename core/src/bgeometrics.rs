//! API publique BGeometrics (<https://bitcoin-data.com>) : métriques on-chain
//! Bitcoin gratuites, sans clé (SOPR, NRPL, HODL waves, ETF...).
//!
//! Offre gratuite limitée à 8 requêtes par heure et 15 par jour, sur les 4
//! dernières années : les réponses sont donc gardées sur disque (données mises
//! à jour une fois par jour) et réutilisées si l'API refuse une requête.
//!
//! Les noms d'endpoint n'étant pas tous documentés publiquement, chaque
//! métrique a une liste de noms candidats : le premier qui répond est retenu
//! (et mémorisé avec le cache). Un nom peut être imposé dans la configuration.

use crate::table::{self, Row};
use chrono::NaiveDate;
use serde_json::Value;

pub const BASE_URL: &str = "https://bitcoin-data.com/v1/";

/// Champs communs à toutes les réponses (date et horodatage).
pub const META_FIELDS: [&str; 2] = ["d", "unixTs"];

pub struct Metric {
    /// Clé de configuration (`[BGEOMETRICS]` de config.ini).
    pub key: &'static str,
    pub label: &'static str,
    /// Noms d'endpoint essayés dans l'ordre.
    pub candidates: &'static [&'static str],
}

pub const METRICS: [Metric; 5] = [
    Metric {
        key: "sth_sopr",
        label: "STH-SOPR",
        candidates: &["sth-sopr"],
    },
    Metric {
        key: "lth_sopr",
        label: "LTH-SOPR",
        candidates: &["lth-sopr"],
    },
    Metric {
        key: "etf",
        label: "BTC détenus par les ETF",
        candidates: &["etf-btc-total", "etf-btc", "etf-btc-held"],
    },
    Metric {
        key: "realized_cap_hodl_waves",
        label: "Realized Cap HODL Waves",
        candidates: &[
            "realized-cap-hodl-waves",
            "hodl-waves-realized-cap",
            "rcap-hodl-waves",
        ],
    },
    Metric {
        key: "nrpl",
        label: "Net Realized Profit/Loss",
        candidates: &["nrpl", "nrpl-usd"],
    },
];

pub fn metric(key: &str) -> Option<&'static Metric> {
    METRICS.iter().find(|m| m.key == key)
}

/// Un nom d'endpoint valide ne contient que des minuscules, chiffres et tirets.
pub fn is_valid_endpoint(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Résultat d'une requête HTTP à l'API.
pub enum Reply {
    Rows(Vec<Row>),
    /// Endpoint inexistant : essayer le nom candidat suivant.
    NotFound,
    Error(String),
}

/// Décode une réponse de l'API (tableau `[{"d": "2024-01-01", "unixTs": ..., "<métrique>": ...}]`).
pub fn parse_reply(status: u16, body: &str) -> Reply {
    match status {
        200 => {}
        404 => return Reply::NotFound,
        429 => {
            return Reply::Error(
                "Limite de l'API gratuite BGeometrics atteinte (8 requêtes par heure, 15 par jour). Réessayez plus tard.".into(),
            )
        }
        other => return Reply::Error(format!("Erreur API BGeometrics : HTTP {other}")),
    }
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Array(items)) => {
            let rows: Vec<Row> = items
                .into_iter()
                .filter_map(|v| v.as_object().cloned())
                .collect();
            if rows.is_empty() {
                Reply::NotFound
            } else {
                Reply::Rows(rows)
            }
        }
        Ok(other) => {
            let msg = other
                .get("message")
                .or_else(|| other.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("format inattendu");
            Reply::Error(format!("Réponse BGeometrics inattendue : {msg}"))
        }
        Err(e) => Reply::Error(format!("Réponse BGeometrics illisible : {e}")),
    }
}

/// Lignes datées, triées par date (colonne `d`).
pub fn dated_rows(rows: &[Row]) -> Vec<(NaiveDate, &Row)> {
    let mut out: Vec<(NaiveDate, &Row)> = rows
        .iter()
        .filter_map(|r| Some((table::date(r, "d")?, r)))
        .collect();
    out.sort_by_key(|(d, _)| *d);
    out.dedup_by_key(|(d, _)| *d);
    out
}

/// Colonnes de valeurs (tout sauf la date et l'horodatage).
pub fn value_columns(rows: &[Row]) -> Vec<String> {
    table::numeric_columns(rows, &META_FIELDS)
}

/// Série à une seule valeur par jour (première colonne de valeur).
pub fn single_series(rows: &[Row]) -> Result<(String, Vec<(NaiveDate, f64)>), String> {
    let col = value_columns(rows).into_iter().next().ok_or_else(|| {
        format!(
            "Aucune valeur numérique dans la réponse BGeometrics (colonnes : {:?}).",
            table::columns(rows)
        )
    })?;
    let points = dated_rows(rows)
        .into_iter()
        .filter_map(|(d, r)| Some((d, table::num(r, &col)?)))
        .collect();
    Ok((col, points))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_and_errors() {
        let body = r#"[{"d":"2024-01-02","unixTs":1704153600,"sthSopr":1.01},{"d":"2024-01-01","unixTs":1704067200,"sthSopr":"0.99"}]"#;
        let Reply::Rows(rows) = parse_reply(200, body) else {
            panic!("lignes attendues")
        };
        let (col, s) = single_series(&rows).unwrap();
        assert_eq!(col, "sthSopr");
        assert_eq!(
            s,
            vec![
                (NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 0.99),
                (NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1.01)
            ]
        );

        assert!(matches!(parse_reply(404, ""), Reply::NotFound));
        assert!(matches!(parse_reply(200, "[]"), Reply::NotFound));
        assert!(matches!(parse_reply(429, ""), Reply::Error(e) if e.contains("8 requêtes")));
        assert!(
            matches!(parse_reply(200, r#"{"message":"Too many"}"#), Reply::Error(e) if e.contains("Too many"))
        );
    }

    #[test]
    fn endpoint_names_are_restricted() {
        assert!(is_valid_endpoint("realized-cap-hodl-waves"));
        assert!(!is_valid_endpoint("../etc"));
        assert!(!is_valid_endpoint("sopr?token=x"));
        assert!(!is_valid_endpoint(""));
        for m in METRICS {
            assert!(
                m.candidates.iter().all(|c| is_valid_endpoint(c)),
                "{}",
                m.key
            );
        }
    }
}

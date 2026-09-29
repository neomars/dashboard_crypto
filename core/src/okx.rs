//! API publique OKX (statistiques de trading, sans clé) : ratio de comptes
//! long/short et open interest des contrats perpétuels.

use chrono::{DateTime, NaiveDate};
use serde_json::Value;

pub const BASE_URL: &str = "https://www.okx.com/api/v5/rubik/stat/contracts/";
/// Nombre maximal de points par requête.
pub const PAGE_SIZE: usize = 100;

pub fn inst_id(pair: &str) -> String {
    format!("{}-USDT-SWAP", pair.to_uppercase())
}

/// Décode `{"code": "0", "data": [["<ts ms>", "<v1>", ...], ...]}` en lignes
/// (date, valeurs), triées par date croissante.
pub fn parse_rows(body: &Value) -> Result<Vec<(i64, NaiveDate, Vec<f64>)>, String> {
    let code = body["code"].as_str().unwrap_or("");
    if code != "0" {
        let msg = body["msg"]
            .as_str()
            .filter(|m| !m.is_empty())
            .unwrap_or("réponse inattendue");
        return Err(format!("Erreur API OKX ({code}) : {msg}"));
    }
    let mut rows: Vec<(i64, NaiveDate, Vec<f64>)> = body["data"]
        .as_array()
        .ok_or("Réponse OKX sans données.")?
        .iter()
        .filter_map(|entry| {
            let fields = entry.as_array()?;
            let ts = crate::table::value_as_f64(fields.first()?)? as i64;
            let date = DateTime::from_timestamp_millis(ts)?.date_naive();
            let values = fields[1..]
                .iter()
                .map(|v| crate::table::value_as_f64(v).unwrap_or(f64::NAN))
                .collect();
            Some((ts, date, values))
        })
        .collect();
    rows.sort_by_key(|r| r.0);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_newest_first_data() {
        let v = json!({"code": "0", "msg": "", "data": [["1704153600000", "1.2"], ["1704067200000", "0.8"]]});
        let rows = parse_rows(&v).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
        assert_eq!(rows[0].2, vec![0.8]);
    }

    #[test]
    fn api_errors_are_reported() {
        let v = json!({"code": "51001", "msg": "Instrument ID does not exist", "data": []});
        assert!(parse_rows(&v).unwrap_err().contains("Instrument ID"));
    }

    #[test]
    fn instrument_ids() {
        assert_eq!(inst_id("eth"), "ETH-USDT-SWAP");
    }
}

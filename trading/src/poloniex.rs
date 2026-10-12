//! API publique de Poloniex (sans clé ni compte) : bougies spot, bougies du
//! perpétuel et historique du funding. Ce module construit les requêtes et
//! décode les réponses ; [`crate::feed`] les envoie.
//!
//! Formats vérifiés sur l'API en octobre 2026 :
//! - spot `GET /markets/{symbol}/candles` : tableau de bougies
//!   `[low, high, open, close, amount, quantity, buyTakerAmount,
//!   buyTakerQuantity, tradeCount, ts, weightedAverage, interval, startTime,
//!   closeTime]`, prix en chaînes, horodatages en millisecondes, ordre croissant ;
//! - perpétuel `GET /v3/market/candles` : `{code: 200, msg, data}` avec des
//!   bougies `[l, h, o, c, amt, qty, tC, sT, cT]`, toutes en chaînes ;
//! - funding `GET /v3/market/fundingRate/history` : `{code: 200, msg, data}`
//!   avec des entrées `{s, fR, fT}` (chaînes), plus récentes d'abord, une
//!   toutes les 8 h, 100 au plus par requête, 180 derniers jours seulement.

use crate::market::{Bar, FundingRate, Instrument, Interval, Market};
use serde_json::Value;

pub const BASE_URL: &str = "https://api.poloniex.com";
/// Bougies maximales par requête (spot et perpétuel).
pub const CANDLE_LIMIT: usize = 500;
/// Taux de funding par requête (l'API refuse une limite de 1 000).
pub const FUNDING_LIMIT: usize = 100;
/// Profondeur de l'historique du funding servi par Poloniex.
pub const FUNDING_HISTORY_DAYS: i64 = 180;

/// Requête GET : URL et paramètres.
pub type Request = (String, Vec<(&'static str, String)>);

/// Page de bougies dont l'heure d'ouverture est dans `[start, end]` (ms).
pub fn candles_request(inst: &Instrument, interval: Interval, start: i64, end: i64) -> Request {
    let limit = CANDLE_LIMIT.to_string();
    let interval = interval.api_name().to_string();
    match inst.market {
        Market::Spot => (
            format!("{BASE_URL}/markets/{}/candles", inst.symbol()),
            vec![
                ("interval", interval),
                ("limit", limit),
                ("startTime", start.to_string()),
                ("endTime", end.to_string()),
            ],
        ),
        Market::Perp => (
            format!("{BASE_URL}/v3/market/candles"),
            vec![
                ("symbol", inst.symbol()),
                ("interval", interval),
                ("limit", limit),
                ("sTime", start.to_string()),
                ("eTime", end.to_string()),
            ],
        ),
    }
}

/// Page de l'historique du funding entre `start` et `end` (ms).
pub fn funding_request(inst: &Instrument, start: i64, end: i64) -> Request {
    (
        format!("{BASE_URL}/v3/market/fundingRate/history"),
        vec![
            ("symbol", inst.symbol()),
            ("limit", FUNDING_LIMIT.to_string()),
            ("sT", start.to_string()),
            ("eT", end.to_string()),
        ],
    )
}

/// Nombre écrit en JSON comme nombre ou comme chaîne.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

/// Message d'erreur d'une réponse `{code, msg|message}`, si c'en est une.
pub fn api_error(body: &Value) -> Option<String> {
    let obj = body.as_object()?;
    let code = obj.get("code").and_then(num);
    if code == Some(200.0) {
        return None;
    }
    let msg = obj
        .get("msg")
        .or_else(|| obj.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("réponse inattendue");
    Some(match code {
        Some(c) => format!("Poloniex (code {c}) : {msg}"),
        None => format!("Poloniex : {msg}"),
    })
}

/// Données `data` d'une réponse `{code: 200, msg, data}` (perpétuel).
fn v3_data(body: &Value) -> Result<&Vec<Value>, String> {
    if let Some(err) = api_error(body) {
        return Err(err);
    }
    body["data"]
        .as_array()
        .ok_or_else(|| "Réponse Poloniex sans données.".to_string())
}

/// Décode une page de bougies ; les bougies incohérentes sont écartées.
pub fn parse_candles(market: Market, body: &Value) -> Result<Vec<Bar>, String> {
    // Positions de (bas, haut, ouverture, clôture, montant USDT, heure d'ouverture).
    let (rows, idx) = match market {
        Market::Spot => {
            let rows = body.as_array().ok_or_else(|| {
                api_error(body)
                    .unwrap_or_else(|| "Réponse Poloniex inattendue (bougies spot).".into())
            })?;
            (rows, [0, 1, 2, 3, 4, 12])
        }
        Market::Perp => (v3_data(body)?, [0, 1, 2, 3, 4, 7]),
    };
    let mut bars: Vec<Bar> = rows
        .iter()
        .filter_map(|row| {
            let row = row.as_array()?;
            let f = |k: usize| row.get(idx[k]).and_then(num);
            Some(Bar {
                low: f(0)?,
                high: f(1)?,
                open: f(2)?,
                close: f(3)?,
                volume: f(4).unwrap_or(0.0),
                time: f(5)? as i64,
            })
        })
        .filter(Bar::is_valid)
        .collect();
    bars.sort_by_key(|b| b.time);
    Ok(bars)
}

/// Décode une page de l'historique du funding (triée par date croissante).
pub fn parse_funding(body: &Value) -> Result<Vec<FundingRate>, String> {
    let mut out: Vec<FundingRate> = v3_data(body)?
        .iter()
        .filter_map(|e| {
            Some(FundingRate {
                time: num(&e["fT"])? as i64,
                rate: num(&e["fR"])?,
            })
        })
        .collect();
    out.sort_by_key(|f| f.time);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_use_the_documented_paths() {
        let spot = Instrument::new("BTC", Market::Spot).unwrap();
        let (url, q) = candles_request(&spot, Interval::D1, 1, 2);
        assert_eq!(url, "https://api.poloniex.com/markets/BTC_USDT/candles");
        assert!(
            q.contains(&("interval", "DAY_1".into())) && q.contains(&("startTime", "1".into()))
        );

        let perp = Instrument::new("ETH", Market::Perp).unwrap();
        let (url, q) = candles_request(&perp, Interval::H4, 1, 2);
        assert_eq!(url, "https://api.poloniex.com/v3/market/candles");
        assert!(
            q.contains(&("symbol", "ETH_USDT_PERP".into())) && q.contains(&("eTime", "2".into()))
        );

        let (url, q) = funding_request(&perp, 1, 2);
        assert!(url.ends_with("/v3/market/fundingRate/history"));
        assert!(q.contains(&("limit", "100".into())));
    }

    #[test]
    fn parses_spot_candles() {
        // Bougie réelle du 11/10/2026 (BTC_USDT, DAY_1), relevée sur l'API.
        let body = json!([
            [
                "82787.01",
                "84129.27",
                "83017.8",
                "83725.82",
                "178777029.36",
                "2147.03535",
                "89221165.32",
                "1071.546916",
                113659,
                1791676800000i64,
                "83268.14",
                "DAY_1",
                1791676800000i64,
                1791763199999i64
            ],
            [
                "1",
                "0.5",
                "1",
                "1",
                "0",
                "0",
                "0",
                "0",
                0,
                0,
                "1",
                "DAY_1",
                1791763200000i64,
                1791849599999i64
            ]
        ]);
        let bars = parse_candles(Market::Spot, &body).unwrap();
        assert_eq!(bars.len(), 1, "bougie incohérente (haut < bas) écartée");
        let b = bars[0];
        assert_eq!(
            (b.open, b.high, b.low, b.close),
            (83017.8, 84129.27, 82787.01, 83725.82)
        );
        assert_eq!(b.time, 1791676800000);
        assert_eq!(b.volume, 178777029.36);
    }

    #[test]
    fn parses_perp_candles() {
        let body = json!({"code": 200, "msg": "Success", "data": [
            ["82503.34", "83132.87", "82589.24", "82945.68", "844914.27674", "10200", "3593",
             "1791590400000", "1791676799999"]
        ]});
        let bars = parse_candles(Market::Perp, &body).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, 82945.68);
        assert_eq!(bars[0].time, 1791590400000);
    }

    #[test]
    fn reports_api_errors() {
        let err = parse_candles(
            Market::Perp,
            &json!({"code": 400, "msg": "Param error", "data": []}),
        )
        .unwrap_err();
        assert!(err.contains("400") && err.contains("Param error"));
        let err = parse_candles(
            Market::Spot,
            &json!({"code": 21601, "message": "Invalid symbol"}),
        )
        .unwrap_err();
        assert!(err.contains("Invalid symbol"));
    }

    #[test]
    fn parses_funding_newest_first_into_ascending_order() {
        let body = json!({"code": 200, "msg": "Success", "data": [
            {"s": "BTC_USDT_PERP", "fR": "0.0001", "fT": "1791820800000"},
            {"s": "BTC_USDT_PERP", "fR": "0.000084", "fT": "1791763200000"}
        ]});
        let f = parse_funding(&body).unwrap();
        assert_eq!(
            f,
            vec![
                FundingRate {
                    time: 1791763200000,
                    rate: 0.000084
                },
                FundingRate {
                    time: 1791820800000,
                    rate: 0.0001
                }
            ]
        );
    }
}

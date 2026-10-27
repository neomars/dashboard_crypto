//! API publique Binance Futures (données de marché, sans clé) : ratio long/short
//! des « top traders » des contrats perpétuels USDⓈ-M.
//!
//! Binance appelle « top traders » les 20 % de comptes ayant le plus de marge.
//! Le ratio de comptes compte chaque compte une fois, quelle que soit la taille
//! de sa position. L'API ne publie que les 30 derniers jours.

use crate::table::value_as_f64;
use serde_json::Value;

pub const BASE_URL: &str = "https://fapi.binance.com/futures/data/topLongShortAccountRatio";
/// Nombre maximal de points par requête.
pub const PAGE_SIZE: usize = 500;
/// Garde-fou : 17 pages suffisent pour 30 jours en période de 5 minutes.
pub const MAX_PAGES: usize = 20;
/// Profondeur de l'historique publié par Binance.
pub const HISTORY_DAYS: i64 = 30;

/// Périodes acceptées par l'API : (code, libellé).
pub const PERIODS: [(&str, &str); 9] = [
    ("5m", "5 minutes"),
    ("15m", "15 minutes"),
    ("30m", "30 minutes"),
    ("1h", "1 heure"),
    ("2h", "2 heures"),
    ("4h", "4 heures"),
    ("6h", "6 heures"),
    ("12h", "12 heures"),
    ("1d", "1 jour"),
];

/// Un relevé du ratio long/short des top traders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopTraderPoint {
    /// Début de la période (millisecondes depuis 1970, UTC).
    pub ts: i64,
    /// Comptes nets long / comptes nets short.
    pub ratio: f64,
    /// Part des comptes nets long (0 à 1).
    pub long: f64,
    /// Part des comptes nets short (0 à 1).
    pub short: f64,
}

pub fn period_label(code: &str) -> Option<&'static str> {
    PERIODS.iter().find(|(c, _)| *c == code).map(|(_, l)| *l)
}

/// Contrat perpétuel USDⓈ-M d'une monnaie : `eth` → `ETHUSDT`. Le symbole
/// complet (`ETHUSDT`) est accepté tel quel.
pub fn symbol(coin: &str) -> Result<String, String> {
    let coin = coin.trim().to_uppercase();
    if coin.is_empty() || coin.len() > 20 || !coin.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(format!(
            "Monnaie invalide : « {coin} » (lettres et chiffres seulement, ex. BTC, ETH, 1000PEPE)."
        ));
    }
    Ok(if coin.ends_with("USDT") {
        coin
    } else {
        format!("{coin}USDT")
    })
}

/// Décode la réponse de `topLongShortAccountRatio` : un tableau de
/// `{"symbol", "longShortRatio", "longAccount", "shortAccount", "timestamp"}`
/// (nombres écrits entre guillemets), trié par date croissante. Une erreur de
/// l'API est un objet `{"code": -1121, "msg": "Invalid symbol."}`.
pub fn parse_points(
    status: u16,
    body: &Value,
    symbol: &str,
) -> Result<Vec<TopTraderPoint>, String> {
    if status == 451 {
        return Err(
            "Binance Futures refuse les connexions depuis cette zone géographique (HTTP 451)."
                .into(),
        );
    }
    let Some(entries) = body.as_array() else {
        return Err(match (body["code"].as_i64(), body["msg"].as_str()) {
            (Some(-1121), _) => {
                format!("Binance ne propose pas de contrat perpétuel {symbol} (erreur -1121).")
            }
            (Some(code), Some(msg)) => format!("Erreur API Binance ({code}) : {msg}"),
            _ => format!("Réponse Binance inattendue (HTTP {status})."),
        });
    };
    let mut points: Vec<TopTraderPoint> = entries
        .iter()
        .filter_map(|entry| {
            let ts = value_as_f64(&entry["timestamp"])? as i64;
            let ratio = value_as_f64(&entry["longShortRatio"])?;
            let long = value_as_f64(&entry["longAccount"]).unwrap_or(ratio / (1.0 + ratio));
            let short = value_as_f64(&entry["shortAccount"]).unwrap_or(1.0 - long);
            Some(TopTraderPoint {
                ts,
                ratio,
                long,
                short,
            })
        })
        .collect();
    points.sort_by_key(|p| p.ts);
    Ok(points)
}

/// Début de la fenêtre que l'on demande à Binance : un jour de marge sur les 30
/// jours publiés, pour ne jamais viser une date hors fenêtre (que l'API pourrait refuser).
pub fn window_floor(now_ms: i64) -> i64 {
    now_ms - (HISTORY_DAYS - 1) * 86_400_000
}

/// `endTime` de la page suivante, plus ancienne : `oldest` est le premier
/// horodatage de la page reçue, `count` son nombre de points. `None` quand
/// l'historique est épuisé (page incomplète, ou début de la fenêtre atteint).
pub fn next_end(oldest: i64, count: usize, floor: i64) -> Option<i64> {
    let end = oldest - 1;
    (count >= PAGE_SIZE && end >= floor).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_points_and_sorts_them_oldest_first() {
        // Nombres écrits en texte (réponse documentée) ou en nombres : les deux sont lus.
        let v = json!([
            {"symbol": "BTCUSDT", "longShortRatio": "1.7000", "longAccount": "0.6296", "shortAccount": "0.3704", "timestamp": 1583143200000_i64},
            {"symbol": "BTCUSDT", "longShortRatio": "1.8105", "longAccount": "0.6442", "shortAccount": "0.3558", "timestamp": "1583139600000"}
        ]);
        let p = parse_points(200, &v, "BTCUSDT").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].ts, 1_583_139_600_000);
        assert_eq!(
            (p[0].ratio, p[0].long, p[0].short),
            (1.8105, 0.6442, 0.3558)
        );
        assert_eq!(p[1].ts, 1_583_143_200_000);
    }

    #[test]
    fn shares_are_derived_from_the_ratio_when_missing() {
        let v = json!([{"longShortRatio": "3.0", "timestamp": "1583139600000"}]);
        let p = parse_points(200, &v, "BTCUSDT").unwrap();
        assert!((p[0].long - 0.75).abs() < 1e-12 && (p[0].short - 0.25).abs() < 1e-12);
    }

    #[test]
    fn unreadable_entries_are_skipped() {
        let v = json!([
            {"longShortRatio": "n/a", "timestamp": "1583139600000"},
            {"longShortRatio": "1.2"},
            {"longShortRatio": "1.2", "timestamp": "1583143200000"}
        ]);
        assert_eq!(parse_points(200, &v, "BTCUSDT").unwrap().len(), 1);
        assert!(parse_points(200, &json!([]), "BTCUSDT").unwrap().is_empty());
    }

    #[test]
    fn api_errors_are_reported() {
        let unknown = json!({"code": -1121, "msg": "Invalid symbol."});
        let e = parse_points(400, &unknown, "BTCCUSDT").unwrap_err();
        assert!(e.contains("BTCCUSDT") && e.contains("-1121"), "{e}");

        let limited = json!({"code": -1003, "msg": "Too many requests."});
        let e = parse_points(429, &limited, "BTCUSDT").unwrap_err();
        assert!(
            e.contains("-1003") && e.contains("Too many requests"),
            "{e}"
        );

        // Corps illisible (page d'erreur, proxy...) : le code HTTP est signalé.
        let e = parse_points(502, &Value::Null, "BTCUSDT").unwrap_err();
        assert!(e.contains("502"), "{e}");
    }

    #[test]
    fn restricted_location_is_explained_whatever_the_body() {
        for body in [
            Value::Null,
            json!({"code": 0, "msg": "Service unavailable"}),
        ] {
            let e = parse_points(451, &body, "BTCUSDT").unwrap_err();
            assert!(e.contains("451") && e.contains("zone géographique"), "{e}");
        }
    }

    #[test]
    fn symbols_from_coins() {
        assert_eq!(symbol("btc").unwrap(), "BTCUSDT");
        assert_eq!(symbol(" Eth ").unwrap(), "ETHUSDT");
        assert_eq!(symbol("1000pepe").unwrap(), "1000PEPEUSDT");
        assert_eq!(symbol("solusdt").unwrap(), "SOLUSDT");
        for bad in [
            "",
            "  ",
            "BTC/USDT",
            "BTC-USDT",
            "BTC USDT",
            "é",
            &"A".repeat(21),
        ] {
            assert!(symbol(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn period_labels() {
        assert_eq!(period_label("4h"), Some("4 heures"));
        assert_eq!(period_label("1d"), Some("1 jour"));
        assert_eq!(period_label("3h"), None);
        assert_eq!(PERIODS.len(), 9);
    }

    #[test]
    fn paging_goes_back_in_time_until_the_window_or_a_short_page() {
        let floor = window_floor(30 * 86_400_000);
        assert_eq!(floor, 86_400_000, "29 jours avant « maintenant »");
        // Page pleine, encore dans la fenêtre : la suivante finit juste avant son premier point.
        assert_eq!(
            next_end(10 * 86_400_000, PAGE_SIZE, floor),
            Some(863_999_999)
        );
        // Page incomplète : Binance n'a plus rien de plus ancien.
        assert_eq!(next_end(10 * 86_400_000, PAGE_SIZE - 1, floor), None);
        // Début de la fenêtre atteint : on ne demande pas plus ancien.
        assert_eq!(next_end(floor, PAGE_SIZE, floor), None);
        assert_eq!(next_end(floor + 1, PAGE_SIZE, floor), Some(floor));
    }
}

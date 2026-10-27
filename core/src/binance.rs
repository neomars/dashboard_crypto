//! API publique Binance Futures (données de marché, sans clé) : ratio long/short
//! des « top traders » des contrats perpétuels USDⓈ-M, et cours (bougies) du
//! même contrat.
//!
//! Binance appelle « top traders » les 20 % de comptes ayant le plus de marge.
//! Le ratio de comptes compte chaque compte une fois, quelle que soit la taille
//! de sa position. L'API ne publie que les 30 derniers jours de ce ratio ; les
//! bougies n'ont pas cette limite, mais on n'en demande que de quoi les comparer.

use crate::table::value_as_f64;
use serde_json::Value;

pub const BASE_URL: &str = "https://fapi.binance.com";
const RATIO_PATH: &str = "/futures/data/topLongShortAccountRatio";
const KLINES_PATH: &str = "/fapi/v1/klines";
/// Nombre maximal de points du ratio par requête.
pub const PAGE_SIZE: usize = 500;
/// Garde-fou : 17 pages suffisent pour 30 jours en période de 5 minutes.
pub const MAX_PAGES: usize = 20;
/// Profondeur de l'historique publié par Binance.
pub const HISTORY_DAYS: i64 = 30;
/// Nombre de bougies demandé par requête (le maximum de l'API) : la pagination ne s'y fie pas,
/// elle continue jusqu'à la bougie en cours, même si Binance renvoie des pages plus courtes.
pub const KLINES_PAGE_SIZE: usize = 1500;
/// Garde-fou : 7 pages de 1 500 bougies suffisent pour 33 jours en bougies de 5 minutes, et 20
/// pages couvrent encore des pages de 500.
pub const KLINES_MAX_PAGES: usize = 20;
/// Jours de bougies demandés avant le premier relevé du ratio : au moins une
/// bougie terminée précède ce relevé (une période dure au plus un jour), pour que
/// chaque relevé ait un cours connu.
pub const KLINES_MARGIN_DAYS: i64 = 3;
const DAY_MS: i64 = 86_400_000;

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

/// Une bougie du cours du contrat perpétuel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kline {
    /// Ouverture de la bougie (millisecondes depuis 1970, UTC).
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// URL de l'endpoint du ratio, sous l'adresse de base de l'API.
pub fn ratio_url(base: &str) -> String {
    format!("{base}{RATIO_PATH}")
}

/// URL de l'endpoint des bougies, sous l'adresse de base de l'API.
pub fn klines_url(base: &str) -> String {
    format!("{base}{KLINES_PATH}")
}

pub fn period_label(code: &str) -> Option<&'static str> {
    PERIODS.iter().find(|(c, _)| *c == code).map(|(_, l)| *l)
}

/// Durée d'une période de l'API en millisecondes (`4h` → 14 400 000).
pub fn period_ms(code: &str) -> Option<i64> {
    period_label(code)?;
    let (count, unit) = code.split_at(code.len() - 1);
    let unit_ms = match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => DAY_MS,
        _ => return None,
    };
    Some(count.parse::<i64>().ok()? * unit_ms)
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

/// Lignes d'une réponse réussie (un tableau), ou l'erreur lisible d'une réponse
/// refusée par Binance : un objet `{"code": -1121, "msg": "Invalid symbol."}`.
fn rows<'a>(status: u16, body: &'a Value, symbol: &str) -> Result<&'a Vec<Value>, String> {
    if status == 451 {
        return Err(
            "Binance Futures refuse les connexions depuis cette zone géographique (HTTP 451)."
                .into(),
        );
    }
    body.as_array()
        .ok_or_else(|| match (body["code"].as_i64(), body["msg"].as_str()) {
            (Some(-1121), _) => {
                format!("Binance ne propose pas de contrat perpétuel {symbol} (erreur -1121).")
            }
            (Some(code), Some(msg)) => format!("Erreur API Binance ({code}) : {msg}"),
            _ => format!("Réponse Binance inattendue (HTTP {status})."),
        })
}

/// Décode la réponse de `topLongShortAccountRatio` : un tableau de
/// `{"symbol", "longShortRatio", "longAccount", "shortAccount", "timestamp"}`
/// (nombres écrits entre guillemets), trié par date croissante.
pub fn parse_points(
    status: u16,
    body: &Value,
    symbol: &str,
) -> Result<Vec<TopTraderPoint>, String> {
    let entries = rows(status, body, symbol)?;
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

/// Décode la réponse de `/fapi/v1/klines` : un tableau de lignes
/// `[ouverture (ms), "open", "high", "low", "close", "volume", fermeture, ...]`,
/// trié par date croissante.
pub fn parse_klines(status: u16, body: &Value, symbol: &str) -> Result<Vec<Kline>, String> {
    let entries = rows(status, body, symbol)?;
    let mut klines: Vec<Kline> = entries
        .iter()
        .filter_map(|entry| {
            Some(Kline {
                ts: value_as_f64(&entry[0])? as i64,
                open: value_as_f64(&entry[1])?,
                high: value_as_f64(&entry[2])?,
                low: value_as_f64(&entry[3])?,
                close: value_as_f64(&entry[4])?,
            })
        })
        .collect();
    klines.sort_by_key(|k| k.ts);
    Ok(klines)
}

/// Début de la fenêtre que l'on demande à Binance : un jour de marge sur les 30
/// jours publiés, pour ne jamais viser une date hors fenêtre (que l'API pourrait refuser).
pub fn window_floor(now_ms: i64) -> i64 {
    now_ms - (HISTORY_DAYS - 1) * DAY_MS
}

/// Première bougie demandée : les jours du ratio, plus la marge de calcul du cours.
pub fn kline_window_start(now_ms: i64) -> i64 {
    now_ms - (HISTORY_DAYS + KLINES_MARGIN_DAYS) * DAY_MS
}

/// Vrai quand la dernière bougie reçue (ouverte à `last`) est la bougie en cours à l'instant
/// `now` : rien de plus récent n'existe.
pub fn klines_up_to_date(last: i64, period_ms: i64, now: i64) -> bool {
    last + period_ms > now
}

/// `startTime` de la page de bougies suivante, plus récente, ou `None` quand tout est reçu :
/// `last` est l'ouverture de la dernière bougie reçue. Le critère est la bougie en cours, pas la
/// taille de la page : une page plus courte que demandé n'est pas la dernière.
pub fn next_kline_start(last: i64, period_ms: i64, now: i64) -> Option<i64> {
    (!klines_up_to_date(last, period_ms, now)).then_some(last + 1)
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
    fn period_durations() {
        assert_eq!(period_ms("5m"), Some(300_000));
        assert_eq!(period_ms("30m"), Some(1_800_000));
        assert_eq!(period_ms("4h"), Some(14_400_000));
        assert_eq!(period_ms("1d"), Some(86_400_000));
        assert_eq!(period_ms("3h"), None, "seules les périodes de l'API");
        // Chaque période proposée a une durée, et la durée d'un jour est un multiple de chacune.
        for (code, _) in PERIODS {
            let ms = period_ms(code).unwrap();
            assert_eq!(DAY_MS % ms, 0, "{code}");
        }
    }

    #[test]
    fn endpoint_urls_hang_under_the_base_address() {
        assert_eq!(
            ratio_url(BASE_URL),
            "https://fapi.binance.com/futures/data/topLongShortAccountRatio"
        );
        assert_eq!(
            klines_url("http://127.0.0.1:1234"),
            "http://127.0.0.1:1234/fapi/v1/klines"
        );
    }

    #[test]
    fn parses_klines_and_sorts_them_oldest_first() {
        // Ouverture en nombre, prix en texte, puis volume, fermeture, etc. (réponse documentée).
        let v = json!([
            [
                1583143200000_i64,
                "8900.5",
                "9000.0",
                "8850.25",
                "8950.75",
                "148.9",
                1583157599999_i64,
                "1.0",
                308,
                "1.0",
                "1.0",
                "0"
            ],
            [
                1583128800000_i64,
                "8800.0",
                "8920.0",
                "8790.0",
                "8900.5",
                "120.1",
                1583143199999_i64,
                "1.0",
                250,
                "1.0",
                "1.0",
                "0"
            ]
        ]);
        let k = parse_klines(200, &v, "BTCUSDT").unwrap();
        assert_eq!(k.len(), 2);
        assert_eq!(k[0].ts, 1_583_128_800_000);
        assert_eq!(k[1].ts, 1_583_143_200_000);
        assert_eq!(
            (k[1].open, k[1].high, k[1].low, k[1].close),
            (8900.5, 9000.0, 8850.25, 8950.75)
        );
    }

    #[test]
    fn unreadable_klines_are_skipped_and_errors_reported() {
        let v = json!([
            [1583128800000_i64, "n/a", "1", "1", "1"],
            [1583132400000_i64, "1", "1", "1"],
            "pas une ligne",
            [1583136000000_i64, "1", "2", "0.5", "1.5"]
        ]);
        let k = parse_klines(200, &v, "BTCUSDT").unwrap();
        assert_eq!(
            k.iter().map(|k| k.ts).collect::<Vec<_>>(),
            [1_583_136_000_000]
        );

        let unknown = json!({"code": -1121, "msg": "Invalid symbol."});
        let e = parse_klines(400, &unknown, "XYZUSDT").unwrap_err();
        assert!(e.contains("XYZUSDT") && e.contains("-1121"), "{e}");
        let e = parse_klines(451, &Value::Null, "BTCUSDT").unwrap_err();
        assert!(e.contains("451"), "{e}");
    }

    #[test]
    fn klines_page_forward_from_the_window_start() {
        let now = 40 * DAY_MS;
        assert_eq!(
            kline_window_start(now),
            7 * DAY_MS,
            "33 jours avant « maintenant »"
        );
        // Dernière bougie déjà terminée : la suivante commence juste après, quelle que soit la
        // taille de la page reçue.
        let period = 300_000;
        assert_eq!(next_kline_start(5_000, period, 5_000 + period), Some(5_001));
        assert!(!klines_up_to_date(5_000, period, 5_000 + period));
        // Dernière bougie = bougie en cours : tout est reçu.
        assert_eq!(next_kline_start(5_000, period, 5_000 + period - 1), None);
        assert!(klines_up_to_date(5_000, period, 5_000 + period - 1));
        assert!(
            klines_up_to_date(5_000, period, 5_000),
            "ouverte à l'instant même"
        );
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

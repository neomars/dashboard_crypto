//! Accès à l'API Binance Futures contre un faux serveur local, sans réseau :
//! paramètres envoyés, pagination vers le passé, fenêtre de 30 jours, erreurs
//! HTTP et cache. Le faux serveur reproduit le comportement documenté de
//! `topLongShortAccountRatio` (500 points au plus, `endTime` inclus, ordre
//! croissant, 30 jours d'historique).

use dashboard_core::config::Config;
use dashboard_core::DataProvider;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

type Query = HashMap<String, String>;

struct FakeBinance {
    url: String,
    requests: Arc<Mutex<Vec<Query>>>,
}

impl FakeBinance {
    fn requests(&self) -> Vec<Query> {
        self.requests.lock().unwrap().clone()
    }
}

/// Paramètres (`?symbol=...&period=...`) d'une requête HTTP entrante.
fn read_query(stream: &mut TcpStream) -> Query {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(n) if n > 0 => head.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let head = String::from_utf8_lossy(&head);
    let target = head.split_whitespace().nth(1).unwrap_or("");
    target
        .split_once('?')
        .map_or("", |(_, query)| query)
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Faux serveur : `respond` reçoit les paramètres de chaque requête et renvoie
/// (code HTTP, corps).
fn fake_binance(respond: impl Fn(&Query) -> (u16, String) + Send + 'static) -> FakeBinance {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/futures/data/topLongShortAccountRatio",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let query = read_query(&mut stream);
            let (status, body) = respond(&query);
            log.lock().unwrap().push(query);
            let reply = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    FakeBinance { url, requests }
}

/// Historique que Binance publierait : un relevé par période sur 30 jours,
/// jusqu'à maintenant, par ordre croissant.
fn history(step_ms: i64) -> Vec<i64> {
    let last = chrono::Utc::now().timestamp_millis() / step_ms * step_ms;
    (0..30 * DAY_MS / step_ms)
        .rev()
        .map(|i| last - i * step_ms)
        .collect()
}

/// Réponse de l'API : les `limit` derniers relevés jusqu'à `endTime` (inclus).
fn page(history: &[i64], query: &Query) -> (u16, String) {
    let limit: usize = query["limit"].parse().unwrap();
    if limit > 500 {
        let error = json!({"code": -1130, "msg": "Invalid data sent for parameter 'limit'."});
        return (400, error.to_string());
    }
    let end = query
        .get("endTime")
        .map_or(i64::MAX, |e| e.parse().unwrap());
    let until: Vec<i64> = history.iter().copied().filter(|ts| *ts <= end).collect();
    let rows: Vec<_> = until[until.len().saturating_sub(limit)..]
        .iter()
        .map(|ts| {
            json!({"symbol": query["symbol"], "longShortRatio": "1.5000",
                "longAccount": "0.6000", "shortAccount": "0.4000", "timestamp": ts})
        })
        .collect();
    (200, json!(rows).to_string())
}

fn serving(history: Vec<i64>) -> FakeBinance {
    fake_binance(move |query| page(&history, query))
}

fn provider(fake: &FakeBinance) -> DataProvider {
    let dir = std::env::temp_dir().join(format!("dashboard-crypto-binance-{}", std::process::id()));
    DataProvider::new(Config::new(dir.join("config.ini")))
        .with_cache_dir(dir.join("cache"))
        .with_binance_url(&fake.url)
}

#[tokio::test]
async fn one_request_covers_30_days_in_4_hour_periods() {
    let series = history(4 * HOUR_MS);
    let fake = serving(series.clone());
    let points = provider(&fake)
        .binance_top_traders("eth", "4h")
        .await
        .unwrap();

    assert_eq!(points.len(), 180);
    assert_eq!(points.iter().map(|p| p.ts).collect::<Vec<_>>(), series);
    assert_eq!(
        (points[0].ratio, points[0].long, points[0].short),
        (1.5, 0.6, 0.4)
    );

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    let query = &requests[0];
    assert_eq!(query["symbol"], "ETHUSDT");
    assert_eq!(query["period"], "4h");
    assert_eq!(query["limit"], "500");
    assert!(!query.contains_key("endTime") && !query.contains_key("startTime"));
}

#[tokio::test]
async fn hourly_periods_take_a_second_page_going_back_in_time() {
    let series = history(HOUR_MS);
    assert_eq!(series.len(), 720);
    let fake = serving(series.clone());
    let points = provider(&fake)
        .binance_top_traders("BTC", "1h")
        .await
        .unwrap();

    assert_eq!(points.iter().map(|p| p.ts).collect::<Vec<_>>(), series);

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].contains_key("endTime"));
    // La seconde page finit juste avant le plus ancien point de la première (les 500 derniers).
    let oldest_of_first_page = series[series.len() - 500];
    assert_eq!(
        requests[1]["endTime"],
        (oldest_of_first_page - 1).to_string()
    );
}

#[tokio::test]
async fn five_minute_periods_stop_at_the_requested_window() {
    let series = history(5 * 60_000);
    assert_eq!(series.len(), 8_640);
    let fake = serving(series);
    let points = provider(&fake)
        .binance_top_traders("SOL", "5m")
        .await
        .unwrap();

    // 17 pages de 500 relevés remontent au-delà de 29 jours : il n'y a pas de 18e.
    let requests = fake.requests();
    assert_eq!(requests.len(), 17);
    assert_eq!(points.len(), 17 * 500);
    assert!(points.windows(2).all(|w| w[0].ts < w[1].ts));

    // Aucune requête ne vise une date hors des 30 jours publiés (que Binance pourrait refuser).
    let oldest_published = chrono::Utc::now().timestamp_millis() - 30 * DAY_MS;
    for end in requests.iter().filter_map(|q| q.get("endTime")) {
        assert!(end.parse::<i64>().unwrap() > oldest_published);
    }
}

#[tokio::test]
async fn a_recent_listing_ends_on_the_first_short_page() {
    // Contrat coté depuis 2 jours seulement : 576 relevés de 5 minutes.
    let step = 5 * 60_000;
    let series: Vec<i64> = history(step).split_off(8_640 - 576);
    let fake = serving(series.clone());
    let points = provider(&fake)
        .binance_top_traders("NEW", "5m")
        .await
        .unwrap();

    assert_eq!(points.iter().map(|p| p.ts).collect::<Vec<_>>(), series);
    // 500 relevés, puis 76 : la page incomplète signale la fin, sans requête de plus.
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn a_server_ignoring_end_time_does_not_make_the_app_loop() {
    // Toujours les 500 derniers relevés, quel que soit `endTime`.
    let series = history(HOUR_MS);
    let latest: Vec<i64> = series[series.len() - 500..].to_vec();
    let fake = fake_binance(move |query| {
        let mut query = query.clone();
        query.remove("endTime");
        page(&latest, &query)
    });
    let points = provider(&fake)
        .binance_top_traders("BTC", "1h")
        .await
        .unwrap();

    assert_eq!(points.len(), 500, "doublons écartés");
    assert_eq!(
        fake.requests().len(),
        2,
        "arrêt dès que la page n'est plus ancienne"
    );
}

#[tokio::test]
async fn http_errors_are_explained_and_not_cached() {
    // Zone géographique bloquée : le corps n'est pas du JSON.
    let blocked = fake_binance(|_| (451, "<html>blocked</html>".into()));
    let data = provider(&blocked);
    let err = data.binance_top_traders("BTC", "4h").await.unwrap_err();
    assert!(
        err.contains("451") && err.contains("zone géographique"),
        "{err}"
    );
    assert!(data.binance_top_traders("BTC", "4h").await.is_err());
    assert_eq!(
        blocked.requests().len(),
        2,
        "une erreur n'est pas mise en cache"
    );

    // Contrat inconnu.
    let unknown = fake_binance(|_| {
        (
            400,
            json!({"code": -1121, "msg": "Invalid symbol."}).to_string(),
        )
    });
    let err = provider(&unknown)
        .binance_top_traders("XYZ", "4h")
        .await
        .unwrap_err();
    assert!(err.contains("XYZUSDT") && err.contains("-1121"), "{err}");

    // Réponse vide : rien à afficher.
    let empty = fake_binance(|_| (200, "[]".into()));
    let err = provider(&empty)
        .binance_top_traders("BTC", "4h")
        .await
        .unwrap_err();
    assert!(err.contains("Aucune donnée Binance pour BTCUSDT"), "{err}");

    // Monnaie invalide : refusée sans appeler le serveur.
    let err = provider(&empty)
        .binance_top_traders("BTC/USDT", "4h")
        .await
        .unwrap_err();
    assert!(err.contains("Monnaie invalide"), "{err}");
    assert_eq!(empty.requests().len(), 1);
}

#[tokio::test]
async fn results_are_cached_per_symbol_and_period() {
    let fake = serving(history(4 * HOUR_MS));
    let data = provider(&fake);

    data.binance_top_traders("BTC", "4h").await.unwrap();
    data.binance_top_traders("btcusdt", "4h").await.unwrap();
    assert_eq!(fake.requests().len(), 1, "même contrat, même période");

    data.binance_top_traders("BTC", "1d").await.unwrap();
    data.binance_top_traders("ETH", "4h").await.unwrap();
    assert_eq!(fake.requests().len(), 3);
}

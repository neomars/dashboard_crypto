//! Accès à l'API Binance Futures contre un faux serveur local, sans réseau :
//! paramètres envoyés, pagination, fenêtre de 30 jours, erreurs HTTP et cache.
//! Le faux serveur reproduit le comportement documenté de
//! `topLongShortAccountRatio` (500 points au plus, `endTime` inclus, ordre
//! croissant, 30 jours d'historique) et de `klines` (1 500 bougies au plus à
//! partir de `startTime`, ordre croissant).

use dashboard_core::config::Config;
use dashboard_core::{indicators, DataProvider};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

type Query = HashMap<String, String>;

struct FakeBinance {
    base: String,
    requests: Arc<Mutex<Vec<(String, Query)>>>,
}

impl FakeBinance {
    /// Paramètres des requêtes reçues sur un endpoint (`topLongShortAccountRatio`, `klines`).
    fn requests_to(&self, endpoint: &str) -> Vec<Query> {
        let log = self.requests.lock().unwrap();
        log.iter()
            .filter(|(path, _)| path.ends_with(endpoint))
            .map(|(_, query)| query.clone())
            .collect()
    }

    fn ratio_requests(&self) -> Vec<Query> {
        self.requests_to("topLongShortAccountRatio")
    }

    fn kline_requests(&self) -> Vec<Query> {
        self.requests_to("klines")
    }
}

/// Chemin et paramètres (`/klines?symbol=...&limit=...`) d'une requête HTTP entrante.
fn read_request(stream: &mut TcpStream) -> (String, Query) {
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
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    (path.to_string(), query)
}

/// Faux serveur : `respond` reçoit le chemin et les paramètres de chaque requête
/// et renvoie (code HTTP, corps).
fn fake_binance(respond: impl Fn(&str, &Query) -> (u16, String) + Send + 'static) -> FakeBinance {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let (path, query) = read_request(&mut stream);
            let (status, body) = respond(&path, &query);
            log.lock().unwrap().push((path, query));
            let reply = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    FakeBinance { base, requests }
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

fn ratio_error(msg: &str) -> (u16, String) {
    (400, json!({"code": -1130, "msg": msg}).to_string())
}

/// Réponse de l'API : les `limit` derniers relevés jusqu'à `endTime` (inclus).
fn ratio_page(history: &[i64], query: &Query) -> (u16, String) {
    let limit: usize = query["limit"].parse().unwrap();
    if limit > 500 {
        return ratio_error("Invalid data sent for parameter 'limit'.");
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

/// Durée d'un intervalle de bougies (seuls ceux utilisés par les tests).
fn kline_step(interval: &str) -> i64 {
    match interval {
        "5m" => 5 * MINUTE_MS,
        "1h" => HOUR_MS,
        "4h" => 4 * HOUR_MS,
        "1d" => DAY_MS,
        other => panic!("intervalle non simulé : {other}"),
    }
}

/// Ouvertures des bougies que Binance publierait : une par période sur 40 jours,
/// jusqu'à maintenant (les bougies n'ont pas la limite de 30 jours du ratio).
fn kline_opens(step_ms: i64) -> Vec<i64> {
    let last = chrono::Utc::now().timestamp_millis() / step_ms * step_ms;
    (0..40 * DAY_MS / step_ms)
        .rev()
        .map(|i| last - i * step_ms)
        .collect()
}

/// Réponse de l'API : les `limit` premières bougies ouvertes à partir de `startTime` (inclus).
fn kline_page(opens: &[i64], step_ms: i64, query: &Query) -> (u16, String) {
    let limit: usize = query["limit"].parse().unwrap();
    if limit > 1500 {
        return ratio_error("Invalid data sent for parameter 'limit'.");
    }
    let start = query
        .get("startTime")
        .map_or(0, |s| s.parse::<i64>().unwrap());
    let rows: Vec<_> = opens
        .iter()
        .copied()
        .filter(|ts| *ts >= start)
        .take(limit)
        .map(|ts| {
            json!([
                ts,
                "100.0",
                "110.0",
                "90.0",
                "105.0",
                "1000.0",
                ts + step_ms - 1,
                "0",
                0,
                "0",
                "0",
                "0"
            ])
        })
        .collect();
    (200, json!(rows).to_string())
}

/// Faux Binance complet : ratio d'après `ratio_history`, bougies d'après l'intervalle demandé.
fn serving(ratio_history: Vec<i64>) -> FakeBinance {
    fake_binance(move |path, query| {
        if path.ends_with("/klines") {
            let step = kline_step(&query["interval"]);
            kline_page(&kline_opens(step), step, query)
        } else {
            ratio_page(&ratio_history, query)
        }
    })
}

fn provider(fake: &FakeBinance) -> DataProvider {
    let dir = std::env::temp_dir().join(format!("dashboard-crypto-binance-{}", std::process::id()));
    DataProvider::new(Config::new(dir.join("config.ini")))
        .with_cache_dir(dir.join("cache"))
        .with_binance_url(&fake.base)
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

    let requests = fake.ratio_requests();
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

    let requests = fake.ratio_requests();
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
    let series = history(5 * MINUTE_MS);
    assert_eq!(series.len(), 8_640);
    let fake = serving(series);
    let points = provider(&fake)
        .binance_top_traders("SOL", "5m")
        .await
        .unwrap();

    // 17 pages de 500 relevés remontent au-delà de 29 jours : il n'y a pas de 18e.
    let requests = fake.ratio_requests();
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
    let step = 5 * MINUTE_MS;
    let series: Vec<i64> = history(step).split_off(8_640 - 576);
    let fake = serving(series.clone());
    let points = provider(&fake)
        .binance_top_traders("NEW", "5m")
        .await
        .unwrap();

    assert_eq!(points.iter().map(|p| p.ts).collect::<Vec<_>>(), series);
    // 500 relevés, puis 76 : la page incomplète signale la fin, sans requête de plus.
    assert_eq!(fake.ratio_requests().len(), 2);
}

#[tokio::test]
async fn a_server_ignoring_end_time_does_not_make_the_app_loop() {
    // Toujours les 500 derniers relevés, quel que soit `endTime`.
    let series = history(HOUR_MS);
    let latest: Vec<i64> = series[series.len() - 500..].to_vec();
    let fake = fake_binance(move |_, query| {
        let mut query = query.clone();
        query.remove("endTime");
        ratio_page(&latest, &query)
    });
    let points = provider(&fake)
        .binance_top_traders("BTC", "1h")
        .await
        .unwrap();

    assert_eq!(points.len(), 500, "doublons écartés");
    assert_eq!(
        fake.ratio_requests().len(),
        2,
        "arrêt dès que la page n'est plus ancienne"
    );
}

#[tokio::test]
async fn http_errors_are_explained_and_not_cached() {
    // Zone géographique bloquée : le corps n'est pas du JSON.
    let blocked = fake_binance(|_, _| (451, "<html>blocked</html>".into()));
    let data = provider(&blocked);
    let err = data.binance_top_traders("BTC", "4h").await.unwrap_err();
    assert!(
        err.contains("451") && err.contains("zone géographique"),
        "{err}"
    );
    assert!(data.binance_top_traders("BTC", "4h").await.is_err());
    assert_eq!(
        blocked.ratio_requests().len(),
        2,
        "une erreur n'est pas mise en cache"
    );

    // Contrat inconnu.
    let unknown = fake_binance(|_, _| {
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
    let empty = fake_binance(|_, _| (200, "[]".into()));
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
    assert_eq!(empty.ratio_requests().len(), 1);
}

#[tokio::test]
async fn results_are_cached_per_symbol_and_period() {
    let fake = serving(history(4 * HOUR_MS));
    let data = provider(&fake);

    data.binance_top_traders("BTC", "4h").await.unwrap();
    data.binance_top_traders("btcusdt", "4h").await.unwrap();
    assert_eq!(fake.ratio_requests().len(), 1, "même contrat, même période");

    data.binance_top_traders("BTC", "1d").await.unwrap();
    data.binance_top_traders("ETH", "4h").await.unwrap();
    assert_eq!(fake.ratio_requests().len(), 3);
}

#[tokio::test]
async fn klines_cover_the_ratio_window_plus_the_margin_in_one_request() {
    let fake = serving(history(4 * HOUR_MS));
    let klines = provider(&fake).binance_klines("eth", "4h").await.unwrap();

    // 33 jours de bougies de 4 heures : 30 jours du ratio + 3 jours de marge.
    let now = chrono::Utc::now().timestamp_millis();
    assert!((196..=199).contains(&klines.len()), "{}", klines.len());
    assert!(klines.windows(2).all(|w| w[1].ts - w[0].ts == 4 * HOUR_MS));
    assert_eq!(
        (
            klines[0].open,
            klines[0].high,
            klines[0].low,
            klines[0].close
        ),
        (100.0, 110.0, 90.0, 105.0)
    );

    let requests = fake.kline_requests();
    assert_eq!(requests.len(), 1);
    let query = &requests[0];
    assert_eq!(query["symbol"], "ETHUSDT");
    assert_eq!(query["interval"], "4h");
    assert_eq!(query["limit"], "1500");
    let start: i64 = query["startTime"].parse().unwrap();
    assert!((start - (now - 33 * DAY_MS)).abs() < 60_000, "{start}");
    assert!(klines[0].ts >= start);
    assert!(fake.ratio_requests().is_empty(), "le ratio n'est pas lu");
}

#[tokio::test]
async fn five_minute_klines_page_forward() {
    let fake = serving(history(5 * MINUTE_MS));
    let klines = provider(&fake).binance_klines("SOL", "5m").await.unwrap();

    // ~9 504 bougies : 6 pages pleines de 1 500, puis une page de ~504.
    assert!((9_500..=9_510).contains(&klines.len()), "{}", klines.len());
    assert!(klines
        .windows(2)
        .all(|w| w[1].ts - w[0].ts == 5 * MINUTE_MS));
    let requests = fake.kline_requests();
    assert_eq!(requests.len(), 7);

    // Chaque page commence juste après la dernière bougie de la précédente.
    for (page, query) in requests.iter().enumerate().skip(1) {
        let last_of_previous = klines[page * 1500 - 1].ts;
        assert_eq!(query["startTime"], (last_of_previous + 1).to_string());
    }
}

#[tokio::test]
async fn a_server_ignoring_start_time_gives_an_error_not_stale_prices() {
    // Quel que soit `startTime`, le serveur renvoie la page qui commence à la première date demandée.
    let step = 5 * MINUTE_MS;
    let opens = kline_opens(step);
    let first_start = Arc::new(Mutex::new(None::<i64>));
    let fake = fake_binance(move |_, query| {
        let mut query = query.clone();
        let start = *first_start
            .lock()
            .unwrap()
            .get_or_insert_with(|| query["startTime"].parse().unwrap());
        query.insert("startTime".into(), start.to_string());
        kline_page(&opens, step, &query)
    });
    let err = provider(&fake)
        .binance_klines("BTC", "5m")
        .await
        .unwrap_err();

    // Les bougies s'arrêtent des jours avant maintenant : erreur, pas des cours périmés.
    assert!(
        err.contains("incomplètes") && err.contains("BTCUSDT"),
        "{err}"
    );
    assert_eq!(
        fake.kline_requests().len(),
        2,
        "arrêt dès que la page n'avance plus"
    );
}

#[tokio::test]
async fn smaller_pages_than_requested_still_reach_the_current_candle() {
    // Binance plafonne ici les pages à 1 000 bougies au lieu de 1 500 : on continue quand même.
    let step = 5 * MINUTE_MS;
    let fake = fake_binance(move |_, query| {
        let mut query = query.clone();
        query.insert("limit".into(), "1000".into());
        kline_page(&kline_opens(step), step, &query)
    });
    let klines = provider(&fake).binance_klines("SOL", "5m").await.unwrap();

    assert!((9_500..=9_510).contains(&klines.len()), "{}", klines.len());
    assert!(klines.windows(2).all(|w| w[1].ts - w[0].ts == step));
    assert_eq!(
        fake.kline_requests().len(),
        10,
        "9 pages de 1 000, puis ~504"
    );
}

#[tokio::test]
async fn too_many_small_pages_give_an_error_not_stale_prices() {
    // Pages de 100 bougies : 20 requêtes ne couvrent qu'une semaine sur les 33 jours demandés.
    let step = 5 * MINUTE_MS;
    let fake = fake_binance(move |_, query| {
        let mut query = query.clone();
        query.insert("limit".into(), "100".into());
        kline_page(&kline_opens(step), step, &query)
    });
    let err = provider(&fake)
        .binance_klines("BTC", "5m")
        .await
        .unwrap_err();

    assert!(err.contains("incomplètes"), "{err}");
    assert_eq!(fake.kline_requests().len(), 20);
}

#[tokio::test]
async fn kline_errors_are_explained_and_not_cached() {
    let blocked = fake_binance(|_, _| (451, "<html>blocked</html>".into()));
    let data = provider(&blocked);
    let err = data.binance_klines("BTC", "4h").await.unwrap_err();
    assert!(err.contains("451"), "{err}");
    assert!(data.binance_klines("BTC", "4h").await.is_err());
    assert_eq!(blocked.kline_requests().len(), 2);

    let unknown = fake_binance(|_, _| {
        (
            400,
            json!({"code": -1121, "msg": "Invalid symbol."}).to_string(),
        )
    });
    let err = provider(&unknown)
        .binance_klines("XYZ", "4h")
        .await
        .unwrap_err();
    assert!(err.contains("XYZUSDT") && err.contains("-1121"), "{err}");

    let empty = fake_binance(|_, _| (200, "[]".into()));
    let err = provider(&empty)
        .binance_klines("BTC", "4h")
        .await
        .unwrap_err();
    assert!(err.contains("Aucune bougie Binance pour BTCUSDT"), "{err}");
}

#[tokio::test]
async fn klines_are_cached_per_symbol_and_period() {
    let fake = serving(history(4 * HOUR_MS));
    let data = provider(&fake);

    data.binance_klines("BTC", "4h").await.unwrap();
    data.binance_klines("btcusdt", "4h").await.unwrap();
    assert_eq!(fake.kline_requests().len(), 1, "même contrat, même période");

    data.binance_klines("BTC", "1d").await.unwrap();
    data.binance_klines("ETH", "4h").await.unwrap();
    assert_eq!(fake.kline_requests().len(), 3);
}

fn labels(out: &indicators::IndicatorOutput) -> Vec<&str> {
    out.metrics.iter().map(|m| m.label.as_str()).collect()
}

fn warnings(out: &indicators::IndicatorOutput) -> Vec<String> {
    let notices = serde_json::to_value(&out.notices).unwrap();
    notices
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["level"] == "warning")
        .map(|n| n["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_indicator_shows_the_price_above_the_plain_ratio_end_to_end() {
    let fake = serving(history(4 * HOUR_MS));
    let params = json!({"coin": "eth", "period": "4h"});
    let out = indicators::render("top_traders", &params, &provider(&fake))
        .await
        .unwrap();

    // Le cours en haut, le ratio en dessous : rien d'autre, ni signal ni avertissement.
    assert_eq!(out.figure.data.len(), 2);
    assert_eq!(out.figure.data[0]["type"], "candlestick");
    assert_eq!(out.figure.data[1]["name"], "Ratio Long/Short (top traders)");
    assert_eq!(out.figure.data[1]["yaxis"], "y2");
    assert_eq!(
        labels(&out),
        ["Ratio Long/Short actuel", "Comptes long", "Comptes short"]
    );
    assert_eq!(fake.ratio_requests().len(), 1);
    assert_eq!(fake.kline_requests().len(), 1);
    assert!(warnings(&out).is_empty(), "{:?}", warnings(&out));
}

#[tokio::test]
async fn the_ratio_alone_is_shown_when_the_price_is_unavailable() {
    let ratio = history(4 * HOUR_MS);
    let fake = fake_binance(move |path, query| {
        if path.ends_with("/klines") {
            (451, "<html>blocked</html>".into())
        } else {
            ratio_page(&ratio, query)
        }
    });
    let params = json!({"coin": "BTC", "period": "4h"});
    let out = indicators::render("top_traders", &params, &provider(&fake))
        .await
        .unwrap();

    // Le ratio reste affiché, seul et au même format, avec la raison de l'absence du cours.
    assert_eq!(out.figure.data.len(), 1);
    assert_eq!(out.figure.data[0]["name"], "Ratio Long/Short (top traders)");
    assert_eq!(out.figure.data[0]["yaxis"], "y");
    assert_eq!(
        labels(&out),
        ["Ratio Long/Short actuel", "Comptes long", "Comptes short"]
    );
    let warnings = warnings(&out);
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("Cours indisponible") && warnings[0].contains("451"),
        "{}",
        warnings[0]
    );
}

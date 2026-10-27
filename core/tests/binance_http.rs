//! Accès à l'API Binance Futures contre un faux serveur local, sans réseau :
//! paramètres envoyés, pagination, fenêtre de 30 jours, erreurs HTTP et cache.
//! Le faux serveur reproduit le comportement documenté de
//! `topLongShortAccountRatio` (500 points au plus, `endTime` inclus, ordre
//! croissant, 30 jours d'historique) et de `klines` (1 500 bougies au plus à
//! partir de `startTime`, ordre croissant). Il sert aussi l'archive publique
//! (`data.binance.vision`) : un zip par jour et par contrat, un relevé toutes les
//! 5 minutes, introuvable (404) avant la cotation du contrat.

use chrono::{DateTime, NaiveDate, Utc};
use dashboard_core::binance::{ArchiveReport, HISTORY_DAYS};
use dashboard_core::config::Config;
use dashboard_core::{indicators, DataProvider};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Profondeur de l'historique des tests qui n'utilisent pas l'archive : les 30 jours de l'API.
const API_DAYS: i64 = HISTORY_DAYS;

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

    /// Jours des fichiers d'archive demandés, dans l'ordre des requêtes.
    fn archive_days(&self) -> Vec<NaiveDate> {
        let log = self.requests.lock().unwrap();
        log.iter()
            .filter_map(|(path, _)| archive_target(path))
            .map(|(_, day)| day)
            .collect()
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
/// et renvoie (code HTTP, corps en octets).
fn fake_server(respond: impl Fn(&str, &Query) -> (u16, Vec<u8>) + Send + 'static) -> FakeBinance {
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
            let head = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    FakeBinance { base, requests }
}

/// Faux serveur de l'API : `respond` renvoie (code HTTP, corps JSON ou texte).
fn fake_binance(respond: impl Fn(&str, &Query) -> (u16, String) + Send + 'static) -> FakeBinance {
    fake_server(move |path, query| {
        let (status, body) = respond(path, query);
        (status, body.into_bytes())
    })
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
    kline_opens_over(step_ms, 40)
}

/// Les mêmes bougies sur `days` jours.
fn kline_opens_over(step_ms: i64, days: i64) -> Vec<i64> {
    let last = chrono::Utc::now().timestamp_millis() / step_ms * step_ms;
    (0..days * DAY_MS / step_ms)
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

/// Dossier de cache neuf : les jours d'archive gardés sur le disque ne doivent pas passer d'un
/// test à l'autre.
fn fresh_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dashboard-crypto-binance-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn provider_in(fake: &FakeBinance, dir: &Path) -> DataProvider {
    DataProvider::new(Config::new(dir.join("config.ini")))
        .with_cache_dir(dir.join("cache"))
        .with_binance_url(&fake.base)
        .with_binance_archive_url(&fake.base)
}

fn provider(fake: &FakeBinance) -> DataProvider {
    provider_in(fake, &fresh_dir())
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
    let klines = provider(&fake)
        .binance_klines("eth", "4h", API_DAYS)
        .await
        .unwrap();

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
    let klines = provider(&fake)
        .binance_klines("SOL", "5m", API_DAYS)
        .await
        .unwrap();

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
        .binance_klines("BTC", "5m", API_DAYS)
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
    let klines = provider(&fake)
        .binance_klines("SOL", "5m", API_DAYS)
        .await
        .unwrap();

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
        .binance_klines("BTC", "5m", API_DAYS)
        .await
        .unwrap_err();

    assert!(err.contains("incomplètes"), "{err}");
    assert_eq!(fake.kline_requests().len(), 20);
}

#[tokio::test]
async fn kline_errors_are_explained_and_not_cached() {
    let blocked = fake_binance(|_, _| (451, "<html>blocked</html>".into()));
    let data = provider(&blocked);
    let err = data
        .binance_klines("BTC", "4h", API_DAYS)
        .await
        .unwrap_err();
    assert!(err.contains("451"), "{err}");
    assert!(data.binance_klines("BTC", "4h", API_DAYS).await.is_err());
    assert_eq!(blocked.kline_requests().len(), 2);

    let unknown = fake_binance(|_, _| {
        (
            400,
            json!({"code": -1121, "msg": "Invalid symbol."}).to_string(),
        )
    });
    let err = provider(&unknown)
        .binance_klines("XYZ", "4h", API_DAYS)
        .await
        .unwrap_err();
    assert!(err.contains("XYZUSDT") && err.contains("-1121"), "{err}");

    let empty = fake_binance(|_, _| (200, "[]".into()));
    let err = provider(&empty)
        .binance_klines("BTC", "4h", API_DAYS)
        .await
        .unwrap_err();
    assert!(err.contains("Aucune bougie Binance pour BTCUSDT"), "{err}");
}

#[tokio::test]
async fn klines_are_cached_per_symbol_and_period() {
    let fake = serving(history(4 * HOUR_MS));
    let data = provider(&fake);

    data.binance_klines("BTC", "4h", API_DAYS).await.unwrap();
    data.binance_klines("btcusdt", "4h", API_DAYS)
        .await
        .unwrap();
    assert_eq!(fake.kline_requests().len(), 1, "même contrat, même période");

    data.binance_klines("BTC", "1d", API_DAYS).await.unwrap();
    data.binance_klines("ETH", "4h", API_DAYS).await.unwrap();
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

// ---------- Archive publique : historique de plus de 30 jours ----------

const ARCHIVE_HEADER: &str = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio";

/// Contenu CSV d'un jour de l'archive : un relevé toutes les 5 minutes, les deux ratios valant
/// `count` et `sum`. Étiquettes de 00:00 à 23:55 (`new_labels`, depuis le 25/06/2026), sinon de
/// 00:05 à 00:00 le lendemain.
fn archive_csv(day: NaiveDate, new_labels: bool, count: f64, sum: f64) -> String {
    let midnight = day
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let first = if new_labels { 0 } else { 1 };
    let mut csv = format!("{ARCHIVE_HEADER}\n");
    for i in first..first + 288 {
        let label = DateTime::from_timestamp_millis(midnight + i * 5 * MINUTE_MS).unwrap();
        csv.push_str(&format!(
            "{},BTCUSDT,100.0,6000000.0,{count},{sum},1.0,1.0\n",
            label.format("%Y-%m-%d %H:%M:%S")
        ));
    }
    csv
}

fn zip_of(name: &str, content: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(name, options).unwrap();
    writer.write_all(content.as_bytes()).unwrap();
    writer.finish().unwrap().into_inner()
}

/// Contrat et jour d'une requête de fichier d'archive :
/// `/data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-2026-05-14.zip`.
fn archive_target(path: &str) -> Option<(String, NaiveDate)> {
    let file = path.strip_prefix("/data/futures/um/daily/metrics/")?;
    let (symbol, name) = file.split_once('/')?;
    let date = name
        .strip_prefix(symbol)?
        .strip_prefix("-metrics-")?
        .strip_suffix(".zip")?;
    Some((
        symbol.to_string(),
        NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?,
    ))
}

/// Ce que contient l'archive du faux serveur.
#[derive(Clone)]
struct ArchiveSpec {
    /// Premier jour dont le fichier existe (404 avant : contrat pas encore coté).
    first_day: NaiveDate,
    /// À partir de ce jour, les étiquettes vont de 00:00 à 23:55 (nouvelle convention).
    new_labels_from: NaiveDate,
    /// Valeur des colonnes `count_…` et `sum_…`.
    count: f64,
    sum: f64,
    /// Réponse d'erreur imposée à ces jours.
    broken: Vec<(NaiveDate, u16)>,
}

impl ArchiveSpec {
    /// Archive cohérente avec l'API du faux serveur (ratio de 1,5), sans trou, ancienne de
    /// plusieurs années.
    fn consistent() -> Self {
        let today = Utc::now().date_naive();
        Self {
            first_day: today - chrono::Duration::days(1_000),
            new_labels_from: today - chrono::Duration::days(100),
            count: 1.5,
            sum: 0.8,
            broken: Vec::new(),
        }
    }

    fn reply(&self, day: NaiveDate) -> (u16, Vec<u8>) {
        if let Some((_, status)) = self.broken.iter().find(|(d, _)| *d == day) {
            return (*status, b"<html>erreur</html>".to_vec());
        }
        if day < self.first_day {
            let missing = b"<Error><Code>NoSuchKey</Code></Error>";
            return (404, missing.to_vec());
        }
        let csv = archive_csv(day, day >= self.new_labels_from, self.count, self.sum);
        (200, zip_of(&format!("BTCUSDT-metrics-{day}.csv"), &csv))
    }
}

/// Faux Binance complet avec son archive : ratio de 1,5 sur 30 jours à la période `period_ms`,
/// 200 jours de bougies, fichiers d'archive d'après `spec`.
fn serving_with_archive(period_ms: i64, spec: ArchiveSpec) -> FakeBinance {
    let series = history(period_ms);
    fake_server(move |path, query| {
        if let Some((_, day)) = archive_target(path) {
            return spec.reply(day);
        }
        let (status, body) = if path.ends_with("/klines") {
            let step = kline_step(&query["interval"]);
            kline_page(&kline_opens_over(step, 200), step, query)
        } else {
            ratio_page(&series, query)
        };
        (status, body.into_bytes())
    })
}

fn days_ago(n: i64) -> NaiveDate {
    Utc::now().date_naive() - chrono::Duration::days(n)
}

fn day_start_ms(day: NaiveDate) -> i64 {
    day.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis()
}

#[tokio::test]
async fn the_archive_extends_the_ratio_beyond_the_30_days_of_the_api() {
    let period = 4 * HOUR_MS;
    let fake = serving_with_archive(period, ArchiveSpec::consistent());
    let started = Utc::now().timestamp_millis();
    let history = provider(&fake)
        .binance_top_traders_history("eth", "4h", 180)
        .await
        .unwrap();

    // Les 180 relevés de l'API, précédés de ceux de l'archive : une date par période, sans trou.
    let points = &history.points;
    assert!(
        (1_075..=1_085).contains(&points.len()),
        "180 jours en 4 h : {}",
        points.len()
    );
    for pair in points.windows(2) {
        assert_eq!(pair[1].ts - pair[0].ts, period, "{pair:?}");
    }
    let since = started - 180 * DAY_MS;
    assert!(
        (since..since + period + 60_000).contains(&points[0].ts),
        "début de la profondeur demandée : {}",
        points[0].ts
    );
    assert!(points
        .iter()
        .all(|p| p.ratio == 1.5 && p.long == 0.6 && p.short == 0.4));

    let ArchiveReport::Added {
        added,
        gap,
        common,
        missing_days,
        cause,
    } = &history.archive
    else {
        panic!("{:?}", history.archive);
    };
    assert_eq!(*added, points.len() - 180);
    assert!(
        *gap < 1e-9 && *common >= 12,
        "écart {gap}, {common} relevés communs"
    );
    assert_eq!((*missing_days, cause), (0, &None));

    // Un fichier par jour, une seule fois, de la veille du début jusqu'aux jours de comparaison.
    let mut days = fake.archive_days();
    let requested = days.len();
    days.sort();
    days.dedup();
    assert_eq!(days.len(), requested, "aucun fichier demandé deux fois");
    assert_eq!(days[0], days_ago(181));
    assert!(*days.last().unwrap() >= days_ago(30) && *days.last().unwrap() <= days_ago(26));
    assert_eq!(
        fake.ratio_requests().len(),
        1,
        "l'API n'est interrogée qu'une fois"
    );
}

#[tokio::test]
async fn archive_days_are_kept_on_disk_and_never_downloaded_twice() {
    let fake = serving_with_archive(4 * HOUR_MS, ArchiveSpec::consistent());
    let dir = fresh_dir();
    let first = provider_in(&fake, &dir)
        .binance_top_traders_history("BTC", "4h", 90)
        .await
        .unwrap();
    let downloaded = fake.archive_days().len();
    assert!(downloaded > 60, "{downloaded} fichiers");

    // Autre session (le cache mémoire est vide) : tout vient du disque.
    let second = provider_in(&fake, &dir)
        .binance_top_traders_history("BTC", "4h", 90)
        .await
        .unwrap();
    assert_eq!(fake.archive_days().len(), downloaded);
    assert_eq!(second.points.len(), first.points.len());
    assert_eq!(second.archive, first.archive);

    // Un fichier du cache abîmé est téléchargé de nouveau, lui seul.
    let victim = days_ago(60);
    let path = dir
        .join("cache/binance-archive/BTCUSDT")
        .join(format!("{victim}.csv"));
    assert!(path.exists(), "{path:?}");
    std::fs::write(&path, "n'importe quoi").unwrap();
    let third = provider_in(&fake, &dir)
        .binance_top_traders_history("BTC", "4h", 90)
        .await
        .unwrap();
    assert_eq!(fake.archive_days()[downloaded..], [victim]);
    assert_eq!(third.points.len(), first.points.len());

    // Une profondeur plus grande ne télécharge que les jours en plus.
    let before = fake.archive_days().len();
    provider_in(&fake, &dir)
        .binance_top_traders_history("BTC", "4h", 100)
        .await
        .unwrap();
    let new_days = &fake.archive_days()[before..];
    assert_eq!(new_days.len(), 10, "{new_days:?}");
    assert!(new_days.iter().all(|d| *d < days_ago(90)));
}

#[tokio::test]
async fn an_archive_that_disagrees_with_the_api_is_refused() {
    let spec = ArchiveSpec {
        count: 0.9,
        sum: 0.9,
        ..ArchiveSpec::consistent()
    };
    let fake = serving_with_archive(4 * HOUR_MS, spec);
    let history = provider(&fake)
        .binance_top_traders_history("BTC", "4h", 180)
        .await
        .unwrap();
    // Rien d'autre que l'API : mieux vaut 30 jours justes que 180 jours d'une autre grandeur.
    assert_eq!(history.points.len(), 180);
    let ArchiveReport::Failed(why) = &history.archive else {
        panic!("{:?}", history.archive);
    };
    assert!(
        why.contains("ne concorde pas") && why.contains("40,0 %"),
        "{why}"
    );
}

#[tokio::test]
async fn the_column_that_matches_the_api_is_used_whatever_its_name_suggests() {
    let spec = ArchiveSpec {
        count: 0.9,
        sum: 1.5,
        ..ArchiveSpec::consistent()
    };
    let fake = serving_with_archive(4 * HOUR_MS, spec);
    let history = provider(&fake)
        .binance_top_traders_history("BTC", "4h", 90)
        .await
        .unwrap();
    assert!(
        matches!(history.archive, ArchiveReport::Added { .. }),
        "{:?}",
        history.archive
    );
    assert!(history.points.iter().all(|p| p.ratio == 1.5));
}

#[tokio::test]
async fn a_contract_listed_later_starts_when_its_files_start() {
    let spec = ArchiveSpec {
        first_day: days_ago(100),
        ..ArchiveSpec::consistent()
    };
    let fake = serving_with_archive(4 * HOUR_MS, spec);
    let dir = fresh_dir();
    let history = provider_in(&fake, &dir)
        .binance_top_traders_history("ARB", "4h", 180)
        .await
        .unwrap();

    let ArchiveReport::Added {
        added,
        missing_days,
        cause,
        ..
    } = &history.archive
    else {
        panic!("{:?}", history.archive);
    };
    // Les jours d'avant la cotation (404) ne sont pas des pannes : l'historique est plus court.
    assert_eq!((*missing_days, cause), (0, &None));
    // De minuit le jour de la cotation (il y a 100 jours) jusqu'à l'API (29 jours et demi) : 6 par jour.
    assert!((420..=427).contains(added), "{added} relevés d'archive");
    assert!(history.points[0].ts >= day_start_ms(days_ago(100)));
    assert!(history.points[0].ts < day_start_ms(days_ago(100)) + 2 * DAY_MS);

    // Les jours sans fichier sont mémorisés aussi : on ne les redemande pas.
    let requested = fake.archive_days().len();
    provider_in(&fake, &dir)
        .binance_top_traders_history("ARB", "4h", 180)
        .await
        .unwrap();
    assert_eq!(fake.archive_days().len(), requested);
}

#[tokio::test]
async fn a_contract_younger_than_the_api_window_has_nothing_older() {
    let spec = ArchiveSpec {
        first_day: days_ago(20),
        ..ArchiveSpec::consistent()
    };
    let fake = serving_with_archive(4 * HOUR_MS, spec);
    let history = provider(&fake)
        .binance_top_traders_history("NEW", "4h", 180)
        .await
        .unwrap();
    assert_eq!(history.archive, ArchiveReport::Nothing);
    assert_eq!(history.points.len(), 180);
}

#[tokio::test]
async fn an_unreachable_archive_leaves_the_api_and_stops_after_the_first_failures() {
    // L'archive refuse tout (HTTP 451) : on n'insiste pas sur les 150 autres jours.
    let series = history(4 * HOUR_MS);
    let fake = fake_server(move |path, query| {
        if archive_target(path).is_some() {
            (451, b"<html>blocked</html>".to_vec())
        } else {
            (200, ratio_page(&series, query).1.into_bytes())
        }
    });
    let history = provider(&fake)
        .binance_top_traders_history("BTC", "4h", 180)
        .await
        .unwrap();
    assert_eq!(history.points.len(), 180);
    let ArchiveReport::Failed(why) = &history.archive else {
        panic!("{:?}", history.archive);
    };
    assert!(
        why.contains("451") && why.contains("zone géographique"),
        "{why}"
    );
    let requested = fake.archive_days().len();
    assert!((8..=16).contains(&requested), "{requested} requêtes");
}

#[tokio::test]
async fn a_busy_archive_is_retried_and_a_missing_day_only_leaves_a_gap() {
    let spec = ArchiveSpec {
        // Deux jours refusés pour de bon (403) au milieu de la période.
        broken: vec![(days_ago(100), 403), (days_ago(101), 403)],
        ..ArchiveSpec::consistent()
    };
    let busy = days_ago(120);
    let busy_once = Arc::new(Mutex::new(false));
    let series = history(4 * HOUR_MS);
    let fake = fake_server(move |path, query| {
        if let Some((_, day)) = archive_target(path) {
            // Le fichier du 120e jour répond « occupé » une première fois, puis normalement.
            let mut seen = busy_once.lock().unwrap();
            if day == busy && !*seen {
                *seen = true;
                return (503, b"<html>occupe</html>".to_vec());
            }
            return spec.reply(day);
        }
        (200, ratio_page(&series, query).1.into_bytes())
    });
    let dir = fresh_dir();
    let history = provider_in(&fake, &dir)
        .binance_top_traders_history("BTC", "4h", 150)
        .await
        .unwrap();

    assert_eq!(
        fake.archive_days().iter().filter(|d| **d == busy).count(),
        2,
        "un essai de plus"
    );
    let ArchiveReport::Added {
        missing_days,
        cause,
        ..
    } = &history.archive
    else {
        panic!("{:?}", history.archive);
    };
    assert_eq!(*missing_days, 2);
    assert!(
        cause.as_deref().is_some_and(|c| c.contains("403")),
        "{cause:?}"
    );
    // Deux jours sans relevé : 12 dates de moins que sur 150 jours complets.
    assert!(
        (150 * 6 - 14..=150 * 6 - 10).contains(&history.points.len()),
        "{} relevés",
        history.points.len()
    );
    // Les jours en échec ne sont pas mémorisés (une autre session les redemanderait), les autres si.
    let cached = |day: NaiveDate| {
        dir.join("cache/binance-archive/BTCUSDT")
            .join(format!("{day}.csv"))
            .exists()
    };
    assert!(!cached(days_ago(100)) && !cached(days_ago(101)));
    assert!(cached(days_ago(99)) && cached(busy));
}

#[tokio::test]
async fn klines_cover_the_whole_requested_history() {
    let fake = serving_with_archive(HOUR_MS, ArchiveSpec::consistent());
    let started = Utc::now().timestamp_millis();
    let klines = provider(&fake)
        .binance_klines("BTC", "1h", 180)
        .await
        .unwrap();

    // 180 jours et la marge de 3 jours, en bougies d'une heure : 3 pages de 1 500 bougies.
    assert!(
        (183 * 24 - 2..=183 * 24 + 2).contains(&klines.len()),
        "{} bougies",
        klines.len()
    );
    let requests = fake.kline_requests();
    assert_eq!(requests.len(), 3);
    let first_start: i64 = requests[0]["startTime"].parse().unwrap();
    assert!(
        (first_start - (started - 183 * DAY_MS)).abs() < 60_000,
        "{first_start}"
    );
    // Les pages se suivent sans trou ni doublon.
    for pair in klines.windows(2) {
        assert_eq!(pair[1].ts - pair[0].ts, HOUR_MS);
    }
}

fn notices_of(out: &indicators::IndicatorOutput, level: &str) -> Vec<String> {
    let notices = serde_json::to_value(&out.notices).unwrap();
    notices
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["level"] == level)
        .map(|n| n["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_indicator_draws_the_whole_history_end_to_end() {
    let fake = serving_with_archive(4 * HOUR_MS, ArchiveSpec::consistent());
    let params = json!({"coin": "eth", "period": "4h", "days": 180});
    let out = indicators::render("top_traders", &params, &provider(&fake))
        .await
        .unwrap();

    // Cours et ratio sur les mêmes dates, 180 jours en 4 h.
    assert_eq!(out.figure.data.len(), 2);
    let candles = out.figure.data[0]["x"].as_array().unwrap().len();
    let ratio = out.figure.data[1]["x"].as_array().unwrap().len();
    assert!((1_075..=1_085).contains(&ratio), "{ratio} relevés");
    assert_eq!(candles, ratio, "une bougie par relevé");
    assert_eq!(
        labels(&out),
        ["Ratio Long/Short actuel", "Comptes long", "Comptes short"]
    );

    // Une seule note, qui dit d'où viennent les relevés ; aucun avertissement.
    assert!(warnings(&out).is_empty(), "{:?}", warnings(&out));
    let info = notices_of(&out, "info");
    assert_eq!(info.len(), 1, "{info:?}");
    assert!(
        info[0].contains("data.binance.vision")
            && info[0].contains("concordance")
            && info[0].contains("0,0 %")
            && info[0].contains("les 180 derniers viennent de l'API"),
        "{}",
        info[0]
    );
}

#[tokio::test]
async fn without_a_history_setting_only_the_api_is_read() {
    let fake = serving_with_archive(4 * HOUR_MS, ArchiveSpec::consistent());
    let out = indicators::render("top_traders", &json!({"coin": "eth"}), &provider(&fake))
        .await
        .unwrap();
    assert_eq!(out.figure.data[1]["x"].as_array().unwrap().len(), 180);
    assert!(
        fake.archive_days().is_empty(),
        "l'archive n'est lue que sur demande"
    );
    let info = notices_of(&out, "info");
    assert!(
        info[0].starts_with("Binance ne publie que les 30 derniers jours"),
        "{}",
        info[0]
    );
    assert!(warnings(&out).is_empty());
}

#[tokio::test]
async fn a_failing_archive_is_reported_and_the_30_days_remain() {
    let series = history(4 * HOUR_MS);
    let fake = fake_server(move |path, query| {
        if archive_target(path).is_some() {
            return (451, b"<html>blocked</html>".to_vec());
        }
        let (status, body) = if path.ends_with("/klines") {
            let step = kline_step(&query["interval"]);
            kline_page(&kline_opens_over(step, 200), step, query)
        } else {
            ratio_page(&series, query)
        };
        (status, body.into_bytes())
    });
    let params = json!({"coin": "BTC", "period": "4h", "days": 180});
    let out = indicators::render("top_traders", &params, &provider(&fake))
        .await
        .unwrap();

    assert_eq!(out.figure.data[1]["x"].as_array().unwrap().len(), 180);
    let warnings = warnings(&out);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("Historique de plus de 30 jours indisponible")
            && warnings[0].contains("seuls les 30 derniers jours sont affichés")
            && warnings[0].contains("451"),
        "{}",
        warnings[0]
    );
    // La note de couverture reste celle de l'API seule.
    assert!(notices_of(&out, "info")[0].starts_with("Binance ne publie que les 30 derniers jours"));
}

#[tokio::test]
async fn a_history_too_long_for_the_period_is_cut_and_said_so() {
    let fake = serving_with_archive(HOUR_MS, ArchiveSpec::consistent());
    let params = json!({"coin": "BTC", "period": "1h", "days": 365});
    let out = indicators::render("top_traders", &params, &provider(&fake))
        .await
        .unwrap();

    let ratio = out.figure.data[1]["x"].as_array().unwrap().len();
    assert!((5_990..=6_010).contains(&ratio), "{ratio} relevés");
    let info = notices_of(&out, "info");
    assert!(
        info.iter().any(|n| n.contains("limité à 250 jours")
            && n.contains("1 heure")
            && n.contains("6000 relevés")),
        "{info:?}"
    );
}

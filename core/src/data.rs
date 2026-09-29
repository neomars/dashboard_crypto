//! Accès aux données externes avec cache mémoire (équivalent des
//! `st.cache_data(ttl=...)` de la version Python).

use crate::config::Config;
use crate::dune::{self, Row};
use crate::series::{Candle, PricePoint};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, Utc};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const OLD_BTC_DATASET_URL: &str =
    "https://raw.githubusercontent.com/Yrzxiong/Bitcoin-Dataset/master/bitcoin_dataset.csv";
const YAHOO_CHART_URL: &str = "https://query2.finance.yahoo.com/v8/finance/chart/";
const FEAR_GREED_URL: &str = "https://api.alternative.me/fng/?limit=0";
const BLOCK_HEIGHT_URL: &str = "https://mempool.space/api/blocks/tip/height";
const DUNE_API_URL: &str = "https://api.dune.com/api/v1";
// Yahoo refuse les requêtes sans User-Agent de navigateur (HTTP 429).
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

const HOUR: Duration = Duration::from_secs(3600);

pub struct Halving {
    pub date: &'static str,
    pub block: u64,
    pub reward: &'static str,
}

pub const KNOWN_HALVINGS: [Halving; 4] = [
    Halving {
        date: "2012-11-28",
        block: 210_000,
        reward: "50 → 25",
    },
    Halving {
        date: "2016-07-09",
        block: 420_000,
        reward: "25 → 12.5",
    },
    Halving {
        date: "2020-05-11",
        block: 630_000,
        reward: "12.5 → 6.25",
    },
    Halving {
        date: "2024-04-20",
        block: 840_000,
        reward: "6.25 → 3.125",
    },
];

pub fn halving_dates() -> Vec<NaiveDate> {
    KNOWN_HALVINGS.iter().map(|h| parse_ymd(h.date)).collect()
}

pub fn parse_ymd(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date littérale valide")
}

pub fn today() -> NaiveDate {
    Utc::now().date_naive()
}

type CacheEntry = (Instant, Arc<dyn Any + Send + Sync>);

pub struct DataProvider {
    http: reqwest::Client,
    config: Config,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl DataProvider {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("client HTTP");
        Self {
            http,
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Renvoie la valeur en cache si elle a moins de `ttl`, sinon la recalcule.
    /// Les erreurs ne sont pas mises en cache.
    async fn cached<T, F, Fut>(&self, key: &str, ttl: Duration, fetch: F) -> Result<Arc<T>, String>
    where
        T: Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        if let Some((at, value)) = self.cache.lock().unwrap().get(key) {
            if at.elapsed() < ttl {
                if let Ok(v) = value.clone().downcast::<T>() {
                    return Ok(v);
                }
            }
        }
        let value = Arc::new(fetch().await?);
        self.cache.lock().unwrap().insert(
            key.to_string(),
            (Instant::now(), value.clone() as Arc<dyn Any + Send + Sync>),
        );
        Ok(value)
    }

    fn seed<T: Send + Sync + 'static>(&self, key: String, value: T) {
        self.cache
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), Arc::new(value)));
    }

    /// Pré-remplit le cache (tests hors ligne) : historique d'un ticker.
    #[doc(hidden)]
    pub fn seed_ticker_history(&self, ticker: &str, candles: Vec<Candle>) {
        self.seed(format!("yahoo:{}", ticker.to_uppercase()), candles);
    }

    #[doc(hidden)]
    pub fn seed_combined_btc(&self, prices: Vec<PricePoint>) {
        self.seed("combined_btc".into(), prices);
    }

    #[doc(hidden)]
    pub fn seed_fear_greed(&self, values: Vec<(NaiveDate, f64)>) {
        self.seed("fear_greed".into(), values);
    }

    #[doc(hidden)]
    pub fn seed_dune_results(&self, query_id: &str, rows: Vec<Row>) {
        self.seed(format!("dune:{query_id}"), rows);
    }

    async fn get_text(&self, url: &str, timeout: Duration) -> Result<String, String> {
        let resp = self
            .http
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| format!("Erreur réseau ({url}) : {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Erreur HTTP {} ({url})", resp.status().as_u16()));
        }
        resp.text()
            .await
            .map_err(|e| format!("Réponse illisible ({url}) : {e}"))
    }

    /// Historique journalier complet (depuis 2010) d'un ticker Yahoo Finance.
    pub async fn ticker_history(&self, ticker: &str) -> Result<Arc<Vec<Candle>>, String> {
        let ticker = ticker.trim().to_uppercase();
        if ticker.is_empty() {
            return Err("Ticker vide.".into());
        }
        self.cached(&format!("yahoo:{ticker}"), HOUR, || async {
            let url = format!("{YAHOO_CHART_URL}{}", encode_path_segment(&ticker));
            let period2 = (Utc::now().timestamp() + 86_400).to_string();
            let resp = self
                .http
                .get(&url)
                .query(&[
                    ("period1", "1262304000"),
                    ("period2", &period2),
                    ("interval", "1d"),
                    ("events", "history"),
                ])
                .send()
                .await
                .map_err(|e| format!("Erreur réseau Yahoo Finance : {e}"))?;
            let status = resp.status();
            let body: Value = resp
                .json()
                .await
                .map_err(|e| format!("Réponse Yahoo Finance illisible (HTTP {status}) : {e}"))?;
            let candles = parse_yahoo_chart(&body)?;
            if candles.is_empty() {
                return Err(format!("Aucune donnée Yahoo Finance pour {ticker}."));
            }
            Ok(candles)
        })
        .await
    }

    /// Historique journalier limité à `[start, end]` (bornes incluses si présentes).
    pub async fn ticker_range(
        &self,
        ticker: &str,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
    ) -> Result<Vec<Candle>, String> {
        let all = self.ticker_history(ticker).await?;
        Ok(all
            .iter()
            .filter(|c| start.is_none_or(|s| c.date >= s) && end.is_none_or(|e| c.date < e))
            .copied()
            .collect())
    }

    /// Historique BTC complet : dataset GitHub (2010-2018) complété par Yahoo Finance (2018+).
    pub async fn combined_btc_history(&self) -> Result<Arc<Vec<PricePoint>>, String> {
        self.cached("combined_btc", Duration::from_secs(86_400), || async {
            let old = match self
                .get_text(OLD_BTC_DATASET_URL, Duration::from_secs(15))
                .await
            {
                Ok(csv) => parse_old_btc_dataset(&csv),
                Err(_) => Vec::new(),
            };
            let start = parse_ymd("2018-01-01");
            let new: Vec<PricePoint> = match self.ticker_history("BTC-USD").await {
                Ok(c) => c
                    .iter()
                    .filter(|c| c.date >= start)
                    .map(|c| PricePoint {
                        date: c.date,
                        close: c.close,
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            let merged = merge_histories(old, new);
            if merged.is_empty() {
                return Err("Impossible de récupérer l'historique du Bitcoin.".into());
            }
            Ok(merged)
        })
        .await
    }

    /// Indice Fear & Greed journalier (alternative.me), trié par date.
    pub async fn fear_greed(&self) -> Result<Arc<Vec<(NaiveDate, f64)>>, String> {
        self.cached("fear_greed", HOUR, || async {
            let text = self
                .get_text(FEAR_GREED_URL, Duration::from_secs(15))
                .await?;
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| format!("Réponse Fear & Greed illisible : {e}"))?;
            parse_fear_greed(&v)
        })
        .await
    }

    /// Estimation de la date du prochain halving (~10 min/bloc).
    pub async fn estimate_next_halving(&self) -> NaiveDate {
        let height = match self
            .get_text(BLOCK_HEIGHT_URL, Duration::from_secs(10))
            .await
        {
            Ok(t) => t.trim().parse::<u64>().ok(),
            Err(_) => None,
        };
        next_halving_estimate(height, Utc::now().naive_utc())
    }

    /// Lignes du dernier résultat d'une requête Dune.
    pub async fn dune_results(&self, query_id: &str) -> Result<Arc<Vec<Row>>, String> {
        let api_key = self.config.dune_api_key();
        if api_key.is_empty() {
            return Err(dune::MISSING_KEY.into());
        }
        let query_id = query_id.to_string();
        self.cached(&format!("dune:{query_id}"), HOUR, || async {
            let url = format!("{DUNE_API_URL}/query/{query_id}/results");
            let resp = self
                .http
                .get(&url)
                .header("X-Dune-API-Key", &api_key)
                .send()
                .await
                .map_err(|e| format!("Erreur de connexion à l'API Dune : {e}"))?;
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            dune::parse_results_response(status, &body, &query_id)
        })
        .await
    }

    /// Lance l'exécution d'une requête Dune puis attend son résultat (~80 s max).
    pub async fn dune_execute(&self, query_id: &str) -> Result<Arc<Vec<Row>>, String> {
        let api_key = self.config.dune_api_key();
        if api_key.is_empty() {
            return Err(dune::MISSING_KEY.into());
        }
        let query_id = query_id.to_string();
        self.cached(&format!("dune-exec:{query_id}"), HOUR * 2, || async {
            let resp = self
                .http
                .post(format!("{DUNE_API_URL}/query/{query_id}/execute"))
                .header("X-Dune-API-Key", &api_key)
                .send()
                .await
                .map_err(|e| format!("Erreur exécution Dune : {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "Erreur exécution Dune : {}",
                    resp.status().as_u16()
                ));
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| format!("Erreur exécution Dune : {e}"))?;
            let execution_id = v["execution_id"]
                .as_str()
                .ok_or("Réponse d'exécution Dune sans execution_id.")?
                .to_string();

            for _ in 0..40 {
                let resp = self
                    .http
                    .get(format!("{DUNE_API_URL}/execution/{execution_id}/results"))
                    .header("X-Dune-API-Key", &api_key)
                    .send()
                    .await;
                if let Ok(resp) = resp {
                    if resp.status().is_success() {
                        let v: Value = resp.json().await.unwrap_or(Value::Null);
                        match v["state"].as_str() {
                            Some("QUERY_STATE_COMPLETED") => return dune::rows_from_json(&v),
                            Some("QUERY_STATE_FAILED") | Some("QUERY_STATE_CANCELLED") => {
                                return Err("La requête Dune a échoué.".into())
                            }
                            _ => {}
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err("Délai dépassé lors de l'exécution de la requête Dune.".into())
        })
        .await
    }
}

/// Encode un segment d'URL (ex. `GC=F` → `GC%3DF`).
pub fn encode_path_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Décode la réponse `v8/finance/chart` de Yahoo Finance.
pub fn parse_yahoo_chart(body: &Value) -> Result<Vec<Candle>, String> {
    let chart = &body["chart"];
    if let Some(desc) = chart["error"]["description"].as_str() {
        return Err(format!("Yahoo Finance : {desc}"));
    }
    let result = &chart["result"][0];
    let Some(timestamps) = result["timestamp"].as_array() else {
        return Ok(Vec::new());
    };
    let offset = result["meta"]["gmtoffset"].as_i64().unwrap_or(0);
    let quote = &result["indicators"]["quote"][0];
    let field = |name: &str, i: usize| quote[name][i].as_f64();

    let mut out: Vec<Candle> = Vec::with_capacity(timestamps.len());
    for (i, ts) in timestamps.iter().enumerate() {
        let (Some(ts), Some(close)) = (ts.as_i64(), field("close", i)) else {
            continue;
        };
        let Some(dt) = DateTime::from_timestamp(ts + offset, 0) else {
            continue;
        };
        let candle = Candle {
            date: dt.date_naive(),
            open: field("open", i).unwrap_or(close),
            high: field("high", i).unwrap_or(close),
            low: field("low", i).unwrap_or(close),
            close,
            volume: field("volume", i).unwrap_or(0.0),
        };
        // Yahoo renvoie parfois le jour courant deux fois : on garde le dernier point.
        match out.last_mut() {
            Some(last) if last.date == candle.date => *last = candle,
            _ => out.push(candle),
        }
    }
    Ok(out)
}

/// Décode le CSV historique (colonnes `Date` au format M/J/AAAA et `btc_market_price`).
pub fn parse_old_btc_dataset(csv: &str) -> Vec<PricePoint> {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let (Some(di), Some(pi)) = (
        cols.iter().position(|c| *c == "Date"),
        cols.iter().position(|c| *c == "btc_market_price"),
    ) else {
        return Vec::new();
    };
    let mut out: Vec<PricePoint> = lines
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            let date = parse_flexible_date(fields.get(di)?)?;
            let close = fields.get(pi)?.trim().parse::<f64>().ok()?;
            Some(PricePoint { date, close })
        })
        .collect();
    out.sort_by_key(|p| p.date);
    out
}

/// Fusionne deux historiques : en cas de doublon de date, le premier l'emporte ;
/// les prix nuls ou négatifs sont écartés (échelle logarithmique).
pub fn merge_histories(first: Vec<PricePoint>, second: Vec<PricePoint>) -> Vec<PricePoint> {
    let mut by_date = std::collections::BTreeMap::new();
    for p in first.into_iter().chain(second) {
        by_date.entry(p.date).or_insert(p.close);
    }
    by_date
        .into_iter()
        .filter(|(_, c)| *c > 0.0 && c.is_finite())
        .map(|(date, close)| PricePoint { date, close })
        .collect()
}

pub fn parse_fear_greed(v: &Value) -> Result<Vec<(NaiveDate, f64)>, String> {
    let data = v["data"]
        .as_array()
        .ok_or("Réponse Fear & Greed sans données.")?;
    let mut out: Vec<(NaiveDate, f64)> = data
        .iter()
        .filter_map(|d| {
            let ts = dune::value_as_f64(&d["timestamp"])? as i64;
            let value = dune::value_as_f64(&d["value"])?;
            Some((DateTime::from_timestamp(ts, 0)?.date_naive(), value.trunc()))
        })
        .collect();
    out.sort_by_key(|(d, _)| *d);
    out.dedup_by_key(|(d, _)| *d);
    Ok(out)
}

/// Prochain halving à partir de la hauteur de bloc actuelle. Sans hauteur
/// connue, l'estime depuis le halving de 2024 (144 blocs/jour).
pub fn next_halving_estimate(current_height: Option<u64>, now: NaiveDateTime) -> NaiveDate {
    let height = current_height.unwrap_or_else(|| {
        let last = parse_ymd("2024-04-20");
        let days = (now.date() - last).num_days().max(0) as u64;
        840_000 + days * 144
    });
    let interval = 210_000;
    let next_block = (height / interval + 1) * interval;
    let minutes = (next_block - height) as i64 * 10;
    (now + ChronoDuration::minutes(minutes)).date()
}

/// Date au format ISO (`2024-01-01`, `2024-01-01 00:00:00.000 UTC`,
/// `2024-01-01T00:00:00Z`) ou américain (`1/31/2018 0:00`).
pub fn parse_flexible_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc().date());
    }
    let first = s.split([' ', 'T']).next()?;
    if let Ok(d) = NaiveDate::parse_from_str(first, "%Y-%m-%d") {
        return Some(d);
    }
    NaiveDate::parse_from_str(first, "%m/%d/%Y").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_yahoo_chart_and_skips_null_rows() {
        let body = json!({"chart": {"error": null, "result": [{
            "meta": {"gmtoffset": 0},
            "timestamp": [1704067200, 1704153600, 1704240000, 1704240500],
            "indicators": {"quote": [{
                "open": [1.0, null, 3.0, 3.5], "high": [2.0, null, 4.0, 4.5],
                "low": [0.5, null, 2.5, 3.0], "close": [1.5, null, 3.5, 4.0],
                "volume": [10.0, null, 30.0, 40.0]
            }]}
        }]}});
        let c = parse_yahoo_chart(&body).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].date, parse_ymd("2024-01-01"));
        assert_eq!(c[1].date, parse_ymd("2024-01-03"));
        assert_eq!(c[1].close, 4.0, "le dernier point du jour l'emporte");
    }

    #[test]
    fn yahoo_error_is_reported() {
        let body = json!({"chart": {"result": null, "error": {"code": "Not Found", "description": "No data found, symbol may be delisted"}}});
        assert!(parse_yahoo_chart(&body).unwrap_err().contains("delisted"));
    }

    #[test]
    fn parses_old_dataset_fixture() {
        let csv = include_str!("../tests/fixtures/bitcoin_dataset_excerpt.csv");
        let p = parse_old_btc_dataset(csv);
        assert_eq!(p.first().unwrap().date, parse_ymd("2010-02-17"));
        assert!(p
            .iter()
            .any(|x| x.date == parse_ymd("2012-11-16") && x.close == 11.8));
    }

    #[test]
    fn merge_prefers_first_and_drops_zero_prices() {
        let d = parse_ymd;
        let a = vec![
            PricePoint {
                date: d("2018-01-01"),
                close: 0.0,
            },
            PricePoint {
                date: d("2018-01-02"),
                close: 10.0,
            },
        ];
        let b = vec![
            PricePoint {
                date: d("2018-01-02"),
                close: 99.0,
            },
            PricePoint {
                date: d("2018-01-03"),
                close: 11.0,
            },
        ];
        let m = merge_histories(a, b);
        assert_eq!(
            m.iter().map(|p| p.close).collect::<Vec<_>>(),
            vec![10.0, 11.0]
        );
    }

    #[test]
    fn parses_fear_greed_payload() {
        let v = json!({"data": [
            {"value": "40", "timestamp": "1704153600"},
            {"value": "25", "timestamp": "1704067200"}
        ]});
        let fg = parse_fear_greed(&v).unwrap();
        assert_eq!(
            fg,
            vec![
                (parse_ymd("2024-01-01"), 25.0),
                (parse_ymd("2024-01-02"), 40.0)
            ]
        );
    }

    #[test]
    fn next_halving_from_height() {
        let now = parse_ymd("2026-01-01").and_hms_opt(0, 0, 0).unwrap();
        // 1 000 blocs restants = 10 000 minutes ≈ 6,9 jours
        assert_eq!(
            next_halving_estimate(Some(1_049_000), now),
            parse_ymd("2026-01-07")
        );
        let fallback = next_halving_estimate(None, now);
        assert!(fallback > parse_ymd("2027-06-01") && fallback < parse_ymd("2029-01-01"));
    }

    #[test]
    fn flexible_dates() {
        let d = parse_ymd("2024-03-05");
        for s in [
            "2024-03-05",
            "2024-03-05 00:00:00.000 UTC",
            "2024-03-05T10:00:00Z",
            "3/5/2024 0:00",
        ] {
            assert_eq!(parse_flexible_date(s), Some(d), "{s}");
        }
        assert_eq!(parse_flexible_date("n/a"), None);
    }

    #[test]
    fn encodes_special_tickers() {
        assert_eq!(encode_path_segment("GC=F"), "GC%3DF");
        assert_eq!(encode_path_segment("BTC-USD"), "BTC-USD");
    }
}

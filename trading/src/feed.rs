//! Récupération des données de marché Poloniex pour le moteur de trading,
//! avec un cache mémoire. Indépendant du `DataProvider` du tableau de bord :
//! le moteur décide sur les cours de la plateforme où il exécute.

use crate::market::{date_ms, Bar, FundingRate, Instrument, Interval, Market, DAY_MS};
use crate::poloniex::{self, Request, CANDLE_LIMIT, FUNDING_HISTORY_DAYS, FUNDING_LIMIT};
use chrono::{Datelike, NaiveDate, Utc};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Les bougies journalières en cours changent toute la journée : 15 min de cache.
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
/// Pause entre deux pages, pour rester loin des limites de requêtes.
const PAGE_DELAY: Duration = Duration::from_millis(150);
/// Garde-fou : nombre maximal de pages par série.
const MAX_PAGES: usize = 200;

type CacheEntry = (Instant, Arc<dyn Any + Send + Sync>);

pub struct MarketFeed {
    pub(crate) http: reqwest::Client,
    cache: Mutex<HashMap<String, CacheEntry>>,
    /// Requêtes récentes à l'API BGeometrics (limites du plan gratuit).
    pub(crate) bgeometrics_calls: Mutex<crate::mvrv::RateGuard>,
}

impl Default for MarketFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketFeed {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "dashboard-crypto-trading/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("client HTTP");
        Self {
            http,
            cache: Mutex::new(HashMap::new()),
            bgeometrics_calls: Mutex::new(crate::mvrv::RateGuard::bgeometrics()),
        }
    }

    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    async fn cached<T, F, Fut>(&self, key: &str, fetch: F) -> Result<Arc<T>, String>
    where
        T: Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        if let Some((at, value)) = self.cache_entry::<T>(key) {
            if at.elapsed() < CACHE_TTL {
                return Ok(value);
            }
        }
        let value = Arc::new(fetch().await?);
        self.cache_put(key, value.clone());
        Ok(value)
    }

    /// Entrée du cache et son âge, quelle que soit la durée de validité.
    pub(crate) fn cache_entry<T: Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Option<(Instant, Arc<T>)> {
        let guard = self.cache.lock().unwrap();
        let (at, value) = guard.get(key)?;
        Some((*at, value.clone().downcast::<T>().ok()?))
    }

    pub(crate) fn cache_put<T: Send + Sync + 'static>(&self, key: &str, value: Arc<T>) {
        self.cache.lock().unwrap().insert(
            key.to_string(),
            (Instant::now(), value as Arc<dyn Any + Send + Sync>),
        );
    }

    fn bars_key(inst: &Instrument, interval: Interval, from: NaiveDate) -> String {
        format!(
            "bars:{}:{}:{}",
            inst.symbol(),
            interval.api_name(),
            year_start(from)
        )
    }

    /// Pré-remplit le cache (tests hors ligne).
    #[doc(hidden)]
    pub fn seed_bars(
        &self,
        inst: &Instrument,
        interval: Interval,
        from: NaiveDate,
        bars: Vec<Bar>,
    ) {
        self.cache.lock().unwrap().insert(
            Self::bars_key(inst, interval, from),
            (Instant::now(), Arc::new(bars)),
        );
    }

    /// Pré-remplit le cache (tests hors ligne).
    #[doc(hidden)]
    pub fn seed_funding(&self, inst: &Instrument, funding: Vec<FundingRate>) {
        self.cache.lock().unwrap().insert(
            format!("funding:{}", inst.symbol()),
            (Instant::now(), Arc::new(funding)),
        );
    }

    async fn get_json(&self, (url, query): &Request) -> Result<Value, String> {
        let resp = self
            .http
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|e| format!("Erreur réseau Poloniex ({url}) : {e}"))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("Réponse Poloniex illisible ({url}) : {e}"))?;
        let body: Value = serde_json::from_str(&text).map_err(|_| {
            format!(
                "Réponse Poloniex illisible (HTTP {}) : {}",
                status.as_u16(),
                text.chars().take(200).collect::<String>()
            )
        })?;
        if !status.is_success() {
            return Err(poloniex::api_error(&body)
                .unwrap_or_else(|| format!("Poloniex : HTTP {} ({url})", status.as_u16())));
        }
        Ok(body)
    }

    /// Bougies closes de `inst` depuis le 1er janvier de l'année de `from`
    /// jusqu'à maintenant (le début est arrondi à l'année pour partager le cache).
    pub async fn bars(
        &self,
        inst: &Instrument,
        interval: Interval,
        from: NaiveDate,
    ) -> Result<Arc<Vec<Bar>>, String> {
        let key = Self::bars_key(inst, interval, from);
        self.cached(&key, || async {
            let now = Utc::now().timestamp_millis();
            let mut all = Vec::new();
            for (k, (start, end)) in candle_pages(date_ms(year_start(from)), now, interval)
                .into_iter()
                .enumerate()
            {
                if k > 0 {
                    tokio::time::sleep(PAGE_DELAY).await;
                }
                let body = self
                    .get_json(&poloniex::candles_request(inst, interval, start, end))
                    .await?;
                let page = poloniex::parse_candles(inst.market, &body)?;
                if page.is_empty() {
                    break; // avant la mise en cotation
                }
                all.extend(page);
            }
            let bars = finalize_bars(all, interval, now);
            if bars.is_empty() {
                return Err(format!(
                    "Aucune bougie Poloniex pour {} (actif non coté sur ce marché ?).",
                    inst.symbol()
                ));
            }
            Ok(bars)
        })
        .await
    }

    /// Historique du funding du perpétuel sur les 180 derniers jours (seule
    /// profondeur servie par Poloniex), trié par date croissante. Vide en spot.
    pub async fn funding(&self, inst: &Instrument) -> Result<Arc<Vec<FundingRate>>, String> {
        if inst.market != Market::Perp {
            return Ok(Arc::new(Vec::new()));
        }
        self.cached(&format!("funding:{}", inst.symbol()), || async {
            let now = Utc::now().timestamp_millis();
            // Une heure de marge : l'API refuse un début à plus de 180 jours.
            let start = now - FUNDING_HISTORY_DAYS * DAY_MS + 3_600_000;
            let mut end = now;
            let mut all: Vec<FundingRate> = Vec::new();
            for page in 0..MAX_PAGES {
                if page > 0 {
                    tokio::time::sleep(PAGE_DELAY).await;
                }
                let body = self
                    .get_json(&poloniex::funding_request(inst, start, end))
                    .await?;
                let rates = poloniex::parse_funding(&body)?;
                let Some(oldest) = rates.first().map(|f| f.time) else {
                    break;
                };
                let full = rates.len() >= FUNDING_LIMIT;
                all.extend(rates);
                if !full || oldest <= start {
                    break;
                }
                end = oldest - 1;
            }
            all.sort_by_key(|f| f.time);
            all.dedup_by_key(|f| f.time);
            Ok(all)
        })
        .await
    }
}

/// 1er janvier de l'année de `date`.
pub fn year_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap_or(date)
}

/// Fenêtres de `CANDLE_LIMIT` bougies couvrant `[from, now]` (ms), de la plus
/// récente à la plus ancienne. Chaque heure d'ouverture tombe dans une seule
/// fenêtre, et une fenêtre ne peut pas contenir plus de `CANDLE_LIMIT` bougies.
pub fn candle_pages(from: i64, now: i64, interval: Interval) -> Vec<(i64, i64)> {
    let span = CANDLE_LIMIT as i64 * interval.millis();
    let mut pages = Vec::new();
    let mut end = now;
    while end >= from && pages.len() < MAX_PAGES {
        let start = (end - span + 1).max(from);
        pages.push((start, end));
        end = start - 1;
    }
    pages
}

/// Trie, dédoublonne et retire la bougie en cours (pas encore close à `now`).
pub fn finalize_bars(mut bars: Vec<Bar>, interval: Interval, now: i64) -> Vec<Bar> {
    bars.sort_by_key(|b| b.time);
    bars.dedup_by_key(|b| b.time);
    bars.retain(|b| b.time + interval.millis() <= now);
    bars
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(time: i64) -> Bar {
        Bar {
            time,
            open: 1.0,
            high: 1.0,
            low: 1.0,
            close: 1.0,
            volume: 0.0,
        }
    }

    #[test]
    fn pages_cover_the_range_without_overlap() {
        let day = Interval::D1.millis();
        let from = 0;
        let now = 1_234 * day + 5;
        let pages = candle_pages(from, now, Interval::D1);
        assert_eq!(pages.first().unwrap().1, now);
        assert_eq!(pages.last().unwrap().0, from);
        for w in pages.windows(2) {
            assert_eq!(w[1].1, w[0].0 - 1, "fenêtres contiguës");
        }
        for (s, e) in &pages {
            assert!(
                e - s < CANDLE_LIMIT as i64 * day,
                "au plus 500 bougies par fenêtre"
            );
        }
        assert_eq!(pages.len(), 3);
    }

    #[test]
    fn unfinished_bar_is_dropped_and_duplicates_removed() {
        let day = Interval::D1.millis();
        let bars = vec![bar(2 * day), bar(0), bar(day), bar(day)];
        let out = finalize_bars(bars, Interval::D1, 2 * day + 10);
        assert_eq!(out.iter().map(|b| b.time).collect::<Vec<_>>(), vec![0, day]);
    }

    #[test]
    fn year_start_rounds_down() {
        let d = NaiveDate::from_ymd_opt(2024, 7, 14).unwrap();
        assert_eq!(year_start(d), NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
    }

    #[tokio::test]
    async fn seeded_cache_avoids_the_network() {
        let feed = MarketFeed::new();
        let inst = Instrument::new("BTC", Market::Perp).unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        feed.seed_bars(&inst, Interval::D1, from, vec![bar(0)]);
        feed.seed_funding(
            &inst,
            vec![FundingRate {
                time: 0,
                rate: 0.0001,
            }],
        );
        // Même année : même entrée de cache.
        let later = NaiveDate::from_ymd_opt(2024, 9, 1).unwrap();
        assert_eq!(
            feed.bars(&inst, Interval::D1, later).await.unwrap().len(),
            1
        );
        assert_eq!(feed.funding(&inst).await.unwrap().len(), 1);
        let spot = Instrument::new("BTC", Market::Spot).unwrap();
        assert!(feed.funding(&spot).await.unwrap().is_empty());
    }
}

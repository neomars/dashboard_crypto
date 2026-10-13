//! Données de la stratégie Fibonacci : téléchargement des bougies 1 minute de
//! Poloniex vers le cache disque ([`crate::candle_store`]), paramètres du
//! contrat et assemblage de la trajectoire de prix.
//!
//! **Limites de requêtes.** Les bougies sont servies 500 par requête (environ
//! 1 050 requêtes par an en 1 minute). Le téléchargement avance de la plus
//! ancienne vers la plus récente, avec une pause entre les pages ; sur une
//! réponse de limitation (HTTP 429) ou une erreur réseau il ralentit
//! (attente doublée, 5 essais), puis s'arrête en gardant tout ce qui est déjà
//! sur disque : le relancer reprend où il s'est arrêté. Les limites exactes de
//! Poloniex pour ces endpoints ne sont pas publiées : la pause est prudente
//! (5 requêtes par seconde au plus).
//!
//! **Profondeur.** La profondeur réelle de l'historique 1 minute n'est pas
//! documentée : elle est détectée (recherche dichotomique de la première
//! bougie disponible) puis mémorisée. Si le perpétuel ne remonte pas assez loin,
//! les bougies 1 minute du comptant `BTC_USDT` servent de trajectoire avant cette
//! date, avec un avertissement ; le funding et les frais restent ceux du perpétuel.

use crate::candle_store::{CandleStore, SeriesKey};
use crate::contract::{self, ContractSpec};
use crate::feed::MarketFeed;
use crate::market::{Bar, Instrument, Interval, Market, DAY_MS};
use crate::poloniex::{self, CANDLE_LIMIT};
use chrono::Utc;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Pause entre deux pages de bougies.
const PAGE_DELAY: Duration = Duration::from_millis(200);
/// Pages gardées en mémoire avant d'être écrites sur disque (environ 8 jours de 1 min).
const FLUSH_PAGES: usize = 24;
/// Essais par page avant d'abandonner (l'attente double à chaque essai).
const MAX_ATTEMPTS: usize = 5;

/// Une source de pages de bougies (Poloniex, ou une fausse source en test).
pub trait CandleSource: Sync {
    /// Bougies dont l'ouverture est dans `[start, end)`, `end - start` ≤ 500 intervalles.
    fn page<'a>(
        &'a self,
        start: i64,
        end: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Bar>, String>> + Send + 'a>>;
}

/// Poloniex : bougies du perpétuel, du comptant, ou du prix de marque.
pub struct PoloniexSource<'a> {
    pub feed: &'a MarketFeed,
    pub inst: Instrument,
    pub interval: Interval,
    pub mark: bool,
}

impl CandleSource for PoloniexSource<'_> {
    fn page<'a>(
        &'a self,
        start: i64,
        end: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Bar>, String>> + Send + 'a>> {
        Box::pin(async move {
            let req = if self.mark {
                poloniex::mark_candles_request(&self.inst, self.interval, start, end - 1)
            } else {
                poloniex::candles_request(&self.inst, self.interval, start, end - 1)
            };
            let body = self.feed.get_json(&req).await?;
            poloniex::parse_candles(self.inst.market, &body)
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    /// Étape en cours (« Recherche du début », « Téléchargement »…).
    pub phase: String,
    /// Pages téléchargées sur le total prévu.
    pub done: usize,
    pub total: usize,
    /// Bougies reçues depuis le début de cette synchronisation.
    pub bars: usize,
    /// Date atteinte.
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub requests: usize,
    pub bars_added: usize,
    pub cancelled: bool,
    /// Première bougie disponible détectée (ms), si la recherche a eu lieu.
    pub first_available: Option<i64>,
}

fn is_retryable(e: &str) -> bool {
    let e = e.to_lowercase();
    e.contains("429")
        || e.contains("réseau")
        || e.contains("illisible")
        || e.contains("rate")
        || e.contains("timeout")
}

async fn page_with_retry(
    source: &dyn CandleSource,
    start: i64,
    end: i64,
    cancel: &AtomicBool,
    base_delay: Duration,
) -> Result<Vec<Bar>, String> {
    let mut delay = base_delay;
    let mut last = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        if cancel.load(Ordering::Relaxed) {
            return Err("annulé".into());
        }
        match source.page(start, end).await {
            Ok(bars) => return Ok(bars),
            Err(e) if is_retryable(&e) && attempt + 1 < MAX_ATTEMPTS => {
                last = e;
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Première date (début de jour, ms) à partir de laquelle la source sert des
/// bougies, entre `from` et `to` : recherche dichotomique, en supposant que les
/// données existent sans interruption à partir de leur début. `None` si même la
/// fin de la plage est vide.
pub async fn first_available(
    source: &dyn CandleSource,
    interval: Interval,
    from: i64,
    to: i64,
    cancel: &AtomicBool,
) -> Result<Option<i64>, String> {
    let span = CANDLE_LIMIT as i64 * interval.millis();
    let has = |t: i64| async move {
        let page = page_with_retry(source, t, t + span, cancel, PAGE_DELAY).await?;
        Ok::<bool, String>(!page.is_empty())
    };
    if has(from).await? {
        return Ok(Some(from));
    }
    let last = (to - span).max(from);
    if !has(last).await? {
        return Ok(None);
    }
    let (mut lo, mut hi) = (from, last);
    while hi - lo > DAY_MS {
        let mid = lo + ((hi - lo) / 2).div_euclid(DAY_MS).max(1) * DAY_MS;
        tokio::time::sleep(PAGE_DELAY).await;
        if has(mid).await? {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Ok(Some(hi))
}

fn fmt_day(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string()
}

/// Plage à télécharger : `[from, to)` en ms ; `now_ms` est l'instant courant
/// (paramètre pour les tests).
#[derive(Debug, Clone, Copy)]
pub struct SyncRange {
    pub from: i64,
    pub to: i64,
    pub now_ms: i64,
}

/// Rythme, progression et annulation d'un téléchargement.
pub struct SyncControl<'a> {
    /// Pause entre deux pages.
    pub pause: Duration,
    pub on_progress: &'a (dyn Fn(SyncProgress) + Sync),
    pub cancel: &'a AtomicBool,
}

/// Télécharge dans le cache les bougies de la plage pas encore présentes.
/// La fin est ramenée au début de la bougie en cours, qui n'est jamais stockée.
pub async fn sync_series(
    source: &dyn CandleSource,
    store: &CandleStore,
    key: &SeriesKey,
    range: SyncRange,
    control: &SyncControl<'_>,
) -> Result<SyncReport, String> {
    let SyncControl {
        pause,
        on_progress,
        cancel,
    } = *control;
    let interval = key.interval;
    let step = interval.millis();
    let span = CANDLE_LIMIT as i64 * step;
    let closed_now = range.now_ms.div_euclid(step) * step;
    let to = range.to.min(closed_now);
    let from = range.from.div_euclid(step) * step;
    let mut report = SyncReport::default();
    let progress = |phase: &str, done: usize, total: usize, bars: usize, at: i64| {
        on_progress(SyncProgress {
            phase: phase.to_string(),
            done,
            total,
            bars,
            at: fmt_day(at),
        })
    };
    if from >= to {
        return Ok(report);
    }
    let mut gaps = store.missing(key, from, to);
    if gaps.is_empty() {
        return Ok(report);
    }

    // Série vide : chercher le début réel de l'historique pour ne pas interroger
    // en vain des années qui n'existent pas.
    if store.bounds(key)?.is_none() {
        progress("Recherche du début de l'historique", 0, 0, 0, from);
        let first = first_available(source, interval, gaps[0].0, to, cancel).await;
        let first = match first {
            Ok(f) => f,
            Err(e) if cancel.load(Ordering::Relaxed) || e == "annulé" => {
                report.cancelled = true;
                return Ok(report);
            }
            Err(e) => return Err(e),
        };
        report.first_available = first;
        match first {
            None => {
                return Err(format!(
                    "Aucune bougie {} disponible chez Poloniex entre {} et {}.",
                    key.interval.label(),
                    fmt_day(gaps[0].0),
                    fmt_day(to)
                ));
            }
            Some(t) if t > gaps[0].0 => {
                // Avant la première bougie : rien à télécharger, à ne plus redemander.
                store.append(key, &[], &[(gaps[0].0, t)])?;
                gaps = store.missing(key, from, to);
            }
            Some(_) => {}
        }
    }

    let total: usize = gaps
        .iter()
        .map(|(s, e)| ((e - s + span - 1) / span) as usize)
        .sum();
    let mut done = 0usize;
    let mut buffer: Vec<Bar> = Vec::new();
    let mut covered: Vec<(i64, i64)> = Vec::new();
    let mut pending_pages = 0usize;
    let flush = |buffer: &mut Vec<Bar>, covered: &mut Vec<(i64, i64)>| -> Result<(), String> {
        if buffer.is_empty() && covered.is_empty() {
            return Ok(());
        }
        store.append(key, buffer, covered)?;
        buffer.clear();
        covered.clear();
        Ok(())
    };

    'gaps: for (gap_start, gap_end) in gaps {
        let mut s = gap_start;
        while s < gap_end {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                break 'gaps;
            }
            let e = (s + span).min(gap_end);
            let page = match page_with_retry(source, s, e, cancel, pause).await {
                Ok(p) => p,
                Err(err) if err == "annulé" => {
                    report.cancelled = true;
                    break 'gaps;
                }
                Err(err) => {
                    flush(&mut buffer, &mut covered)?;
                    return Err(format!(
                        "Téléchargement interrompu au {} : {err}. Les {} bougies déjà reçues sont conservées ; relancez pour reprendre.",
                        fmt_day(s),
                        report.bars_added
                    ));
                }
            };
            report.requests += 1;
            // Seules les bougies de la fenêtre, closes, sans doublon.
            let before = buffer.len();
            buffer.extend(
                page.into_iter()
                    .filter(|b| b.time >= s && b.time < e && b.time + step <= closed_now),
            );
            report.bars_added += buffer.len() - before;
            covered.push((s, e));
            done += 1;
            pending_pages += 1;
            progress("Téléchargement", done, total, report.bars_added, s);
            if pending_pages >= FLUSH_PAGES {
                flush(&mut buffer, &mut covered)?;
                pending_pages = 0;
            }
            s = e;
            tokio::time::sleep(pause).await;
        }
    }
    flush(&mut buffer, &mut covered)?;
    Ok(report)
}

fn contract_file(inst: &Instrument) -> String {
    format!("contract_{}.json", inst.symbol())
}

/// Fiche gardée sur disque avec sa date de lecture.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct CachedContract {
    fetched_at: i64,
    spec: ContractSpec,
}

/// Durée pendant laquelle la fiche gardée sur disque est réutilisée sans interroger l'API.
const CONTRACT_TTL_MS: i64 = DAY_MS;

impl MarketFeed {
    /// Fiche du contrat : lue sur l'API au plus une fois par jour, gardée sur
    /// disque ; si l'API échoue, la dernière fiche gardée est utilisée avec un
    /// avertissement, et sans fiche gardée l'erreur est renvoyée.
    pub async fn contract(&self, base: &str, store: &CandleStore) -> Result<ContractSpec, String> {
        let inst = contract::perp_instrument(base)?;
        let now = Utc::now().timestamp_millis();
        let cached = store
            .read_json::<CachedContract>(&contract_file(&inst))
            .filter(|c| c.spec.symbol == inst.symbol());
        if let Some(c) = cached
            .as_ref()
            .filter(|c| now - c.fetched_at < CONTRACT_TTL_MS)
        {
            return Ok(c.spec.clone());
        }
        match self.fetch_contract(&inst).await {
            Ok(spec) => {
                store.write_json(
                    &contract_file(&inst),
                    &CachedContract {
                        fetched_at: now,
                        spec: spec.clone(),
                    },
                )?;
                Ok(spec)
            }
            Err(e) => match cached {
                Some(mut c) => {
                    c.spec.warnings.push(format!(
                        "Paramètres du contrat lus dans le cache disque du {} (API indisponible : {e}).",
                        fmt_day(c.fetched_at)
                    ));
                    Ok(c.spec)
                }
                None => Err(format!("Paramètres du contrat indisponibles : {e}")),
            },
        }
    }

    async fn fetch_contract(&self, inst: &Instrument) -> Result<ContractSpec, String> {
        let body = self.get_json(&poloniex::instrument_request(inst)).await?;
        let mut spec = contract::parse_instrument(&body, &inst.symbol())?;
        let tiers = match self.get_json(&poloniex::risk_limit_request(inst)).await {
            Ok(b) => contract::parse_risk_limit(&b),
            Err(e) => Err(e),
        };
        match tiers {
            Ok(t) => {
                spec.tiers = t;
                spec.tiers_from_risk_limit = true;
            }
            Err(e) => spec.warnings.push(format!(
                "Paliers de marge de maintenance non lus ({e}) : le taux unique de la fiche du contrat est utilisé."
            )),
        }
        Ok(spec)
    }
}

/// Trajectoire de prix 1 minute assemblée depuis le cache.
#[derive(Debug, Clone)]
pub struct Trajectory {
    pub bars: Vec<Bar>,
    pub warnings: Vec<String>,
    /// Première bougie du perpétuel, s'il y en a.
    pub perp_first: Option<i64>,
    /// Le comptant sert de trajectoire avant cette date.
    pub spot_until: Option<i64>,
}

/// Assemble perpétuel et comptant : le perpétuel là où il existe, le comptant
/// avant sa première bougie. Signale le raccord, les minutes manquantes et une
/// couverture incomplète de `[from, to)`.
pub fn stitch(perp: Vec<Bar>, spot: Vec<Bar>, from: i64, to: i64) -> Result<Trajectory, String> {
    let mut warnings = Vec::new();
    let perp_first = perp.first().map(|b| b.time);
    let mut spot_until = None;
    let bars = match (perp.is_empty(), spot.is_empty()) {
        (true, true) => {
            return Err("Aucune bougie 1 minute en cache : lancez le téléchargement.".into())
        }
        (true, false) => {
            warnings.push("Pas de bougies 1 minute du perpétuel : la trajectoire est celle du comptant BTC_USDT (le funding et les frais restent ceux du perpétuel).".into());
            spot_until = spot.last().map(|b| b.time + 60_000);
            spot
        }
        (false, _) => {
            let first = perp[0].time;
            if first > from + DAY_MS && !spot.is_empty() {
                let head: Vec<Bar> = spot.into_iter().filter(|b| b.time < first).collect();
                if let Some(last) = head.last() {
                    let basis = (perp[0].open / last.close - 1.0) * 100.0;
                    warnings.push(format!(
                        "Le 1 minute du perpétuel ne remonte qu'au {} : avant cette date, la trajectoire est celle du comptant BTC_USDT (écart perpétuel/comptant au raccord : {basis:+.3} %). Le funding et les frais restent ceux du perpétuel.",
                        fmt_day(first)
                    ));
                    spot_until = Some(first);
                }
                head.into_iter().chain(perp).collect()
            } else {
                if first > from + DAY_MS {
                    warnings.push(format!(
                        "L'historique 1 minute disponible commence au {} : la période demandée commence plus tôt.",
                        fmt_day(first)
                    ));
                }
                perp
            }
        }
    };
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else {
        return Err("Aucune bougie 1 minute dans la période.".into());
    };
    let expected = (last.time - first.time) / 60_000 + 1;
    let missing = expected - bars.len() as i64;
    if missing > 0 {
        warnings.push(format!(
            "{missing} minutes manquantes sur {expected} ({:.3} %) : trous de l'historique Poloniex.",
            missing as f64 / expected as f64 * 100.0
        ));
    }
    if last.time + 60_000 < to - 2 * DAY_MS {
        warnings.push(format!(
            "Les bougies en cache s'arrêtent au {} : relancez le téléchargement pour couvrir la période.",
            fmt_day(last.time)
        ));
    }
    Ok(Trajectory {
        bars,
        warnings,
        perp_first,
        spot_until,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesStatus {
    pub label: String,
    pub first: Option<String>,
    pub last: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataStatus {
    pub series: Vec<SeriesStatus>,
    pub cache_bytes: u64,
    pub cache_dir: String,
}

/// État du cache disque (pour l'interface).
pub fn data_status(store: &CandleStore, base: &str) -> Result<DataStatus, String> {
    let perp = contract::perp_instrument(base)?;
    let spot = Instrument::new(base, Market::Spot)?;
    let series = [
        (
            "Perpétuel, dernier prix",
            SeriesKey::new(&perp.symbol(), Interval::M1, false),
        ),
        (
            "Comptant, dernier prix",
            SeriesKey::new(&spot.symbol(), Interval::M1, false),
        ),
        (
            "Perpétuel, prix de marque",
            SeriesKey::new(&perp.symbol(), Interval::M1, true),
        ),
    ];
    let mut out = Vec::new();
    for (label, key) in series {
        let b = store.bounds(&key)?;
        out.push(SeriesStatus {
            label: label.to_string(),
            first: b.map(|b| fmt_day(b.0)),
            last: b.map(|b| {
                chrono::DateTime::from_timestamp_millis(b.1)
                    .unwrap_or_default()
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            }),
        });
    }
    Ok(DataStatus {
        series: out,
        cache_bytes: store.size_bytes(),
        cache_dir: store.root().display().to_string(),
    })
}

/// Ce qu'il faut avoir en cache : bougies de `[from, to)` pour `base`.
#[derive(Debug, Clone, Copy)]
pub struct DataRange<'a> {
    pub base: &'a str,
    pub from: i64,
    pub to: i64,
    /// Télécharger aussi le prix de marque (facultatif).
    pub use_mark: bool,
}

/// Télécharge ce qu'il faut pour la plage : perpétuel, puis comptant avant la
/// première bougie du perpétuel, puis (si demandé) prix de marque. Le prix de
/// marque est facultatif : son absence n'est qu'un avertissement.
pub async fn ensure_data(
    feed: &MarketFeed,
    store: &CandleStore,
    want: DataRange<'_>,
    on_progress: &(dyn Fn(SyncProgress) + Sync),
    cancel: &AtomicBool,
) -> Result<Vec<String>, String> {
    let DataRange {
        base,
        from,
        to,
        use_mark,
    } = want;
    let now_ms = Utc::now().timestamp_millis();
    let control = SyncControl {
        pause: PAGE_DELAY,
        on_progress,
        cancel,
    };
    let perp = contract::perp_instrument(base)?;
    let spot = Instrument::new(base, Market::Spot)?;
    let mut warnings = Vec::new();
    let mk = |inst: &Instrument, mark: bool| PoloniexSource {
        feed,
        inst: inst.clone(),
        interval: Interval::M1,
        mark,
    };
    let perp_key = SeriesKey::new(&perp.symbol(), Interval::M1, false);
    let range = SyncRange { from, to, now_ms };
    let r = sync_series(&mk(&perp, false), store, &perp_key, range, &control).await?;
    if r.cancelled {
        return Ok(warnings);
    }
    // Le comptant couvre la période que le perpétuel n'a pas.
    let perp_first = store.bounds(&perp_key)?.map(|b| b.0);
    let spot_to = perp_first.map_or(to, |f| f.min(to));
    if perp_first.is_none_or(|f| f > from + DAY_MS) {
        let spot_key = SeriesKey::new(&spot.symbol(), Interval::M1, false);
        let range = SyncRange {
            to: spot_to,
            ..range
        };
        let r = sync_series(&mk(&spot, false), store, &spot_key, range, &control).await?;
        if r.cancelled {
            return Ok(warnings);
        }
    }
    if use_mark {
        let mark_key = SeriesKey::new(&perp.symbol(), Interval::M1, true);
        if let Err(e) = sync_series(&mk(&perp, true), store, &mark_key, range, &control).await {
            warnings.push(format!("Prix de marque indisponible ({e}) : les liquidations se lisent sur le dernier prix, avec une marge de sécurité."));
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    /// Appel à plat, pour garder les tests lisibles.
    #[allow(clippy::too_many_arguments)]
    async fn sync_series(
        source: &dyn CandleSource,
        store: &CandleStore,
        key: &SeriesKey,
        from: i64,
        to: i64,
        now_ms: i64,
        pause: Duration,
        on_progress: &(dyn Fn(SyncProgress) + Sync),
        cancel: &AtomicBool,
    ) -> Result<SyncReport, String> {
        super::sync_series(
            source,
            store,
            key,
            SyncRange { from, to, now_ms },
            &SyncControl {
                pause,
                on_progress,
                cancel,
            },
        )
        .await
    }

    /// Fausse source : une bougie par minute à partir de `listed`.
    struct Fake {
        listed: i64,
        end: i64,
        calls: AtomicUsize,
        fail_after: Mutex<Option<usize>>,
        cancel_after: Option<(usize, std::sync::Arc<AtomicBool>)>,
    }

    impl Fake {
        fn new(listed: i64, end: i64) -> Self {
            Self {
                listed,
                end,
                calls: AtomicUsize::new(0),
                fail_after: Mutex::new(None),
                cancel_after: None,
            }
        }
    }

    impl CandleSource for Fake {
        fn page<'a>(
            &'a self,
            start: i64,
            end: i64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Bar>, String>> + Send + 'a>> {
            Box::pin(async move {
                let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some((k, flag)) = &self.cancel_after {
                    if n == *k {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
                if self.fail_after.lock().unwrap().is_some_and(|k| n > k) {
                    return Err("Erreur réseau Poloniex : coupure".into());
                }
                assert!(
                    end - start <= 500 * 60_000,
                    "au plus 500 bougies par requête"
                );
                let mut out = Vec::new();
                let mut t = start.max(self.listed).div_euclid(60_000) * 60_000;
                while t < end && t < self.end {
                    out.push(Bar {
                        time: t,
                        open: 100.0,
                        high: 101.0,
                        low: 99.0,
                        close: 100.5,
                        volume: 0.0,
                    });
                    t += 60_000;
                }
                Ok(out)
            })
        }
    }

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            Self(
                std::env::temp_dir()
                    .join(format!("fibo-data-{tag}-{}-{nanos}", std::process::id())),
            )
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn key() -> SeriesKey {
        SeriesKey::new("BTC_USDT_PERP", Interval::M1, false)
    }

    const T0: i64 = 20_000 * DAY_MS;

    #[tokio::test]
    async fn the_download_is_incremental_and_excludes_the_current_candle() {
        let dir = Scratch::new("inc");
        let store = CandleStore::new(&dir.0);
        let source = Fake::new(T0, T0 + 3 * DAY_MS);
        let cancel = AtomicBool::new(false);
        let now = T0 + 2 * DAY_MS + 90_000; // la bougie de la minute 2 880 est en cours
        let seen = Mutex::new(Vec::new());
        let on = |p: SyncProgress| seen.lock().unwrap().push(p);
        let r = sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 5 * DAY_MS,
            now,
            Duration::ZERO,
            &on,
            &cancel,
        )
        .await
        .unwrap();
        // De T0 à la dernière minute close : 2 jours + 1 minute (la minute 2 880 est close, la 2 881 en cours).
        let bars = store.read(&key(), T0, T0 + 10 * DAY_MS).unwrap();
        assert_eq!(
            bars.last().unwrap().time,
            T0 + 2 * DAY_MS,
            "la bougie en cours est exclue"
        );
        assert_eq!(bars.len() as i64, 2 * 1440 + 1);
        assert_eq!(r.bars_added, bars.len());
        assert!(!r.cancelled);
        // 2 jours = 5,76 pages de 500 minutes.
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.phase == "Téléchargement" && p.done == p.total));
        let calls_first = source.calls.load(Ordering::SeqCst);

        // Deuxième passage plus tard : seul le complément est interrogé.
        let later = T0 + 3 * DAY_MS;
        let before = source.calls.load(Ordering::SeqCst);
        let r2 = sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 5 * DAY_MS,
            later,
            Duration::ZERO,
            &|_| {},
            &cancel,
        )
        .await
        .unwrap();
        assert!(
            source.calls.load(Ordering::SeqCst) - before <= 3,
            "seulement le reste ({calls_first} appels avant)"
        );
        assert_eq!(
            r2.bars_added as i64, 1439,
            "le reste du jour 3 de la source"
        );
        let all = store.read(&key(), T0, T0 + 10 * DAY_MS).unwrap();
        assert_eq!(all.len() as i64, 3 * 1440, "aucun doublon, aucun trou");
        // Un troisième passage ne demande plus rien.
        let before = source.calls.load(Ordering::SeqCst);
        sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 5 * DAY_MS,
            later,
            Duration::ZERO,
            &|_| {},
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(source.calls.load(Ordering::SeqCst), before);
    }

    #[tokio::test]
    async fn the_first_available_candle_is_found_and_the_empty_past_is_not_requested_again() {
        let dir = Scratch::new("first");
        let store = CandleStore::new(&dir.0);
        let listed = T0 + 40 * DAY_MS + 5 * 3_600_000;
        let source = Fake::new(listed, T0 + 60 * DAY_MS);
        let cancel = AtomicBool::new(false);
        let r = sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 50 * DAY_MS,
            T0 + 70 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &cancel,
        )
        .await
        .unwrap();
        let first = r.first_available.unwrap();
        assert!(
            first <= listed && listed - first < DAY_MS + 1,
            "début détecté au jour près"
        );
        let bars = store.read(&key(), T0, T0 + 60 * DAY_MS).unwrap();
        assert_eq!(bars.first().unwrap().time, listed);
        // 50 jours à 500 bougies par page = 144 requêtes ; la recherche du début les économise.
        assert!(r.requests < 40, "{} requêtes", r.requests);
        // Le passé vide est mémorisé.
        let before = source.calls.load(Ordering::SeqCst);
        sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 50 * DAY_MS,
            T0 + 70 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(source.calls.load(Ordering::SeqCst), before);
        // Rien du tout : erreur explicite.
        let empty = Fake::new(T0 + 900 * DAY_MS, T0 + 901 * DAY_MS);
        let other = SeriesKey::new("X", Interval::M1, false);
        let err = sync_series(
            &empty,
            &store,
            &other,
            T0,
            T0 + 50 * DAY_MS,
            T0 + 70 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &cancel,
        )
        .await
        .unwrap_err();
        assert!(err.contains("Aucune bougie"), "{err}");
    }

    #[tokio::test]
    async fn cancelling_keeps_the_progress_and_resuming_continues() {
        let dir = Scratch::new("cancel");
        let store = CandleStore::new(&dir.0);
        let flag = std::sync::Arc::new(AtomicBool::new(false));
        let mut source = Fake::new(T0, T0 + 10 * DAY_MS);
        source.cancel_after = Some((5, flag.clone()));
        let r = sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 4 * DAY_MS,
            T0 + 20 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &flag,
        )
        .await
        .unwrap();
        assert!(r.cancelled);
        let partial = store.read(&key(), T0, T0 + 10 * DAY_MS).unwrap().len();
        assert!(partial > 0 && partial < 4 * 1440, "{partial}");
        // Reprise : le reste seulement.
        let resume = Fake::new(T0, T0 + 10 * DAY_MS);
        let r = sync_series(
            &resume,
            &store,
            &key(),
            T0,
            T0 + 4 * DAY_MS,
            T0 + 20 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert!(!r.cancelled);
        assert_eq!(
            store.read(&key(), T0, T0 + 10 * DAY_MS).unwrap().len(),
            4 * 1440
        );
        assert_eq!(r.bars_added + partial, 4 * 1440);
    }

    #[tokio::test]
    async fn a_network_failure_keeps_what_was_received() {
        let dir = Scratch::new("fail");
        let store = CandleStore::new(&dir.0);
        let source = Fake::new(T0, T0 + 10 * DAY_MS);
        *source.fail_after.lock().unwrap() = Some(6);
        let err = sync_series(
            &source,
            &store,
            &key(),
            T0,
            T0 + 4 * DAY_MS,
            T0 + 20 * DAY_MS,
            Duration::ZERO,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap_err();
        assert!(err.contains("relancez pour reprendre"), "{err}");
        assert!(!store.read(&key(), T0, T0 + 10 * DAY_MS).unwrap().is_empty());
        assert!(!store.coverage(&key()).is_empty());
    }

    fn bar(minute: i64, price: f64) -> Bar {
        Bar {
            time: T0 + minute * 60_000,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: 0.0,
        }
    }

    #[test]
    fn spot_fills_the_period_before_the_perpetual_with_a_warning() {
        let spot: Vec<Bar> = (0..3 * 1440).map(|m| bar(m, 100.0)).collect();
        let perp: Vec<Bar> = (2 * 1440..4 * 1440).map(|m| bar(m, 100.5)).collect();
        let t = stitch(perp, spot, T0, T0 + 4 * DAY_MS).unwrap();
        assert_eq!(t.bars.len(), 4 * 1440, "pas de doublon, pas de trou");
        assert!(t.bars.windows(2).all(|w| w[0].time < w[1].time));
        assert_eq!(t.perp_first, Some(T0 + 2 * 1440 * 60_000));
        assert_eq!(t.spot_until, t.perp_first);
        assert_eq!(t.bars[0].close, 100.0, "comptant avant");
        assert_eq!(t.bars.last().unwrap().close, 100.5);
        let w = &t.warnings[0];
        assert!(w.contains("comptant") && w.contains("+0.500 %"), "{w}");
        // Perpétuel seul, complet : pas d'avertissement.
        let perp: Vec<Bar> = (0..1440).map(|m| bar(m, 100.0)).collect();
        let t = stitch(perp, vec![], T0, T0 + DAY_MS).unwrap();
        assert!(t.warnings.is_empty(), "{:?}", t.warnings);
        // Comptant seul.
        let spot: Vec<Bar> = (0..1440).map(|m| bar(m, 100.0)).collect();
        let t = stitch(vec![], spot, T0, T0 + DAY_MS).unwrap();
        assert!(t.warnings[0].contains("comptant"));
        assert!(stitch(vec![], vec![], T0, T0 + DAY_MS).is_err());
    }

    #[test]
    fn missing_minutes_and_stale_cache_are_reported() {
        let mut perp: Vec<Bar> = (0..1440).map(|m| bar(m, 100.0)).collect();
        perp.remove(700);
        perp.remove(700);
        let t = stitch(perp, vec![], T0, T0 + 10 * DAY_MS).unwrap();
        assert!(
            t.warnings
                .iter()
                .any(|w| w.contains("2 minutes manquantes")),
            "{:?}",
            t.warnings
        );
        assert!(
            t.warnings.iter().any(|w| w.contains("s'arrêtent")),
            "cache en retard sur la période"
        );
    }

    #[test]
    fn data_status_lists_the_cached_series() {
        let dir = Scratch::new("status");
        let store = CandleStore::new(&dir.0);
        store
            .append(
                &key(),
                &[bar(0, 100.0), bar(10, 101.0)],
                &[(T0, T0 + 600_000)],
            )
            .unwrap();
        let s = data_status(&store, "BTC").unwrap();
        assert_eq!(s.series.len(), 3);
        assert!(s.series[0].first.is_some() && s.series[1].first.is_none());
        assert!(s.cache_bytes > 0);
    }

    #[test]
    fn retryable_errors_are_recognised() {
        assert!(is_retryable("Poloniex : HTTP 429 (https://…)"));
        assert!(is_retryable("Erreur réseau Poloniex (…) : timed out"));
        assert!(!is_retryable("Poloniex (code 400) : Param error"));
    }
}

//! Historique du MVRV du bitcoin, pour le module d'accumulation.
//!
//! Deux sources publiques, sans clé, raccordées l'une à l'autre :
//! - **Coin Metrics** (CSV du dépôt public `coinmetrics/data`, colonne
//!   `CapMVRVCur`, environ 2,5 Mo, un point par jour depuis 2010) : l'historique.
//!   Le fichier n'est pas mis à jour en continu ; lors de la vérification du
//!   cahier des charges, il s'arrêtait au 24 mai 2026 ;
//! - **BGeometrics** (`https://bitcoin-data.com/v1/mvrv`) : les données récentes.
//!   Plan gratuit : 8 requêtes par heure, 15 par jour, 4 dernières années. Depuis
//!   septembre 2026, ses 7 derniers jours sont réservés aux abonnés
//!   (changelog BGeometrics v1.7) : sans abonnement, les données les plus
//!   récentes ont donc environ une semaine de retard.
//!
//! Sur les dates communes, la valeur de BGeometrics (la plus récente) l'emporte
//! et l'écart entre les deux sources est mesuré. Ce module est indépendant du
//! client BGeometrics du tableau de bord (`dashboard-core`) : il n'en partage
//! ni le code ni le cache, mais l'adresse IP, donc le quota de requêtes, est le
//! même ; le moteur reste donc bien en dessous des limites.
//!
//! L'endpoint BGeometrics et le format de sa réponse n'ont pas pu être vérifiés
//! sur le service lui-même pour ce développement (accès réseau refusé depuis
//! l'environnement de développement) : l'analyse de la réponse est donc
//! tolérante, voir [`parse_bgeometrics`].

use crate::feed::MarketFeed;
use chrono::{DateTime, Duration as Days, NaiveDate};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const COINMETRICS_URL: &str =
    "https://raw.githubusercontent.com/coinmetrics/data/master/csv/btc.csv";
pub const BGEOMETRICS_URL: &str = "https://bitcoin-data.com/v1/mvrv";
/// Au-delà de ce nombre de jours sans donnée, le MVRV est jugé périmé.
pub const STALE_AFTER_DAYS: i64 = 3;
/// Écarts moyen et maximal (%) entre les deux sources au-delà desquels on avertit.
const WARN_MEAN_GAP_PCT: f64 = 2.0;
const WARN_MAX_GAP_PCT: f64 = 5.0;

/// Les données du jour ne changent qu'une fois par jour : 6 h de cache. Si une
/// source manque, nouvel essai plus tôt (sans dépasser les quotas de l'API).
const CACHE_TTL_COMPLETE: Duration = Duration::from_secs(6 * 3600);
const CACHE_TTL_DEGRADED: Duration = Duration::from_secs(15 * 60);
const MVRV_CACHE_KEY: &str = "mvrv";

pub type Series = Vec<(NaiveDate, f64)>;

/// Contrôle de l'écart entre les deux sources sur leurs dates communes.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MvrvInfo {
    pub coinmetrics_last: Option<NaiveDate>,
    pub bgeometrics_last: Option<NaiveDate>,
    /// Dernière date disponible, toutes sources confondues.
    pub latest: Option<NaiveDate>,
    pub overlap_days: usize,
    pub mean_gap_pct: Option<f64>,
    pub max_gap_pct: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MvrvData {
    /// MVRV par date, croissant.
    pub points: Series,
    pub info: MvrvInfo,
    pub warnings: Vec<String>,
    /// Les deux sources ont répondu.
    pub complete: bool,
}

/// Limiteur de requêtes (fenêtres glissantes d'une heure et d'un jour).
#[derive(Debug)]
pub struct RateGuard {
    calls: VecDeque<Instant>,
    per_hour: usize,
    per_day: usize,
}

impl RateGuard {
    pub fn new(per_hour: usize, per_day: usize) -> Self {
        Self {
            calls: VecDeque::new(),
            per_hour,
            per_day,
        }
    }

    /// Marge sous les limites du plan gratuit (8 par heure, 15 par jour), que
    /// le tableau de bord consomme aussi pour ses propres métriques.
    pub fn bgeometrics() -> Self {
        Self::new(4, 8)
    }

    /// Enregistre une requête à l'instant `now` si les quotas le permettent.
    pub fn try_acquire(&mut self, now: Instant) -> bool {
        let age = |t: &Instant| now.saturating_duration_since(*t);
        while self
            .calls
            .front()
            .is_some_and(|t| age(t) >= Duration::from_secs(24 * 3600))
        {
            self.calls.pop_front();
        }
        let last_hour = self
            .calls
            .iter()
            .filter(|t| age(t) < Duration::from_secs(3600))
            .count();
        if last_hour >= self.per_hour || self.calls.len() >= self.per_day {
            return false;
        }
        self.calls.push_back(now);
        true
    }
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim().get(..10)?, "%Y-%m-%d").ok()
}

/// Lit le CSV Coin Metrics : colonnes `time` et `CapMVRVCur` repérées par leur
/// nom d'en-tête (l'ordre et le nombre de colonnes peuvent changer). Les
/// lignes sans MVRV (avant 2010) sont ignorées.
pub fn parse_coinmetrics(csv: &str) -> Result<Series, String> {
    let mut lines = csv.lines();
    let header = lines.next().ok_or("Fichier Coin Metrics vide.")?;
    let names: Vec<&str> = header
        .trim_start_matches('\u{feff}')
        .split(',')
        .map(|c| c.trim().trim_matches('"'))
        .collect();
    let column = |name: &str| {
        names
            .iter()
            .position(|c| *c == name)
            .ok_or_else(|| format!("Colonne « {name} » absente du fichier Coin Metrics."))
    };
    let (time_col, mvrv_col) = (column("time")?, column("CapMVRVCur")?);
    let mut points: Series = lines
        .filter_map(|line| {
            let cells: Vec<&str> = line.split(',').collect();
            let date = parse_date(cells.get(time_col)?.trim_matches('"'))?;
            let value: f64 = cells.get(mvrv_col)?.trim().trim_matches('"').parse().ok()?;
            (value.is_finite() && value > 0.0).then_some((date, value))
        })
        .collect();
    points.sort_by_key(|p| p.0);
    points.dedup_by_key(|p| p.0);
    if points.is_empty() {
        return Err("Aucune valeur de MVRV dans le fichier Coin Metrics.".into());
    }
    Ok(points)
}

fn json_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

/// Lit la réponse de BGeometrics : un tableau de lignes, ou un objet qui le
/// contient sous `data`. Chaque ligne porte la date (`d`, « AAAA-MM-JJ », ou
/// `unixTs`, en secondes) et la valeur (`mvrv`, ou à défaut `value`, ou le
/// seul autre champ numérique). Nombres et chaînes sont acceptés.
pub fn parse_bgeometrics(body: &Value) -> Result<Series, String> {
    let rows = body
        .as_array()
        .or_else(|| body["data"].as_array())
        .ok_or_else(|| {
            let msg = body["message"]
                .as_str()
                .or_else(|| body["error"].as_str())
                .unwrap_or("format inattendu");
            format!("Réponse BGeometrics inattendue : {msg}")
        })?;
    const META: [&str; 2] = ["d", "unixTs"];
    let mut points: Series = rows
        .iter()
        .filter_map(|row| {
            let obj = row.as_object()?;
            let date = obj
                .get("d")
                .and_then(Value::as_str)
                .and_then(parse_date)
                .or_else(|| {
                    let ts = json_number(obj.get("unixTs")?)?;
                    let secs = if ts > 1e11 { ts / 1000.0 } else { ts };
                    Some(DateTime::from_timestamp(secs as i64, 0)?.date_naive())
                })?;
            let value = obj
                .get("mvrv")
                .or_else(|| obj.get("value"))
                .and_then(json_number)
                .or_else(|| {
                    obj.iter()
                        .filter(|(k, _)| !META.contains(&k.as_str()))
                        .find_map(|(_, v)| json_number(v))
                })?;
            (value > 0.0).then_some((date, value))
        })
        .collect();
    points.sort_by_key(|p| p.0);
    points.dedup_by_key(|p| p.0);
    if points.is_empty() {
        return Err("Réponse BGeometrics sans valeur de MVRV exploitable.".into());
    }
    Ok(points)
}

/// Raccorde les deux séries. Sur une date commune, la valeur de `recent` l'emporte.
/// Renvoie la série, le contrôle d'écart et les avertissements.
pub fn merge(history: &[(NaiveDate, f64)], recent: &[(NaiveDate, f64)]) -> MvrvData {
    let mut by_date: BTreeMap<NaiveDate, f64> = history.iter().copied().collect();
    let history_map = by_date.clone();
    let mut gaps = Vec::new();
    for &(date, value) in recent {
        if let Some(old) = history_map.get(&date) {
            gaps.push(((value / old - 1.0).abs() * 100.0, date));
        }
        by_date.insert(date, value);
    }
    let mut warnings = Vec::new();
    let mean_gap =
        (!gaps.is_empty()).then(|| gaps.iter().map(|g| g.0).sum::<f64>() / gaps.len() as f64);
    let max_gap = gaps
        .iter()
        .copied()
        .fold(None, |best: Option<(f64, NaiveDate)>, g| {
            Some(match best {
                Some(b) if b.0 >= g.0 => b,
                _ => g,
            })
        });
    if let (Some(mean), Some((max, at))) = (mean_gap, max_gap) {
        if mean > WARN_MEAN_GAP_PCT || max > WARN_MAX_GAP_PCT {
            warnings.push(format!(
                "Écart entre Coin Metrics et BGeometrics sur {} jours communs : {mean:.1} % en moyenne, jusqu'à {max:.1} % le {at}. Les valeurs de BGeometrics sont retenues sur cette période.",
                gaps.len()
            ));
        }
    }
    // Trou entre la fin de l'historique et le début des données récentes.
    if let (Some(&(h_last, _)), Some(&(r_first, _))) = (history.last(), recent.first()) {
        let hole = (r_first - h_last).num_days() - 1;
        if hole > STALE_AFTER_DAYS {
            warnings.push(format!(
                "Aucune donnée MVRV entre le {h_last} (Coin Metrics) et le {r_first} (BGeometrics) : {hole} jours."
            ));
        }
    }
    let points: Series = by_date.into_iter().collect();
    MvrvData {
        info: MvrvInfo {
            coinmetrics_last: history.last().map(|p| p.0),
            bgeometrics_last: recent.last().map(|p| p.0),
            latest: points.last().map(|p| p.0),
            overlap_days: gaps.len(),
            mean_gap_pct: mean_gap,
            max_gap_pct: max_gap.map(|g| g.0),
        },
        points,
        warnings,
        complete: !history.is_empty() && !recent.is_empty(),
    }
}

/// Dernier MVRV daté de `date` ou avant, s'il a au plus `max_age_days` jours de
/// retard sur `date`.
pub fn value_on_or_before(
    points: &[(NaiveDate, f64)],
    date: NaiveDate,
    max_age_days: i64,
) -> Option<(NaiveDate, f64)> {
    let k = points.partition_point(|p| p.0 <= date).checked_sub(1)?;
    let found = points[k];
    ((date - found.0).num_days() <= max_age_days).then_some(found)
}

impl MarketFeed {
    /// Historique du MVRV : Coin Metrics, complété par BGeometrics pour les
    /// données récentes. Échoue seulement si aucune des deux sources ne répond ;
    /// si l'une manque, l'autre est utilisée et un avertissement l'indique.
    pub async fn mvrv(&self, today: NaiveDate) -> Result<Arc<MvrvData>, String> {
        if let Some((at, data)) = self.cache_entry::<MvrvData>(MVRV_CACHE_KEY) {
            let ttl = if data.complete {
                CACHE_TTL_COMPLETE
            } else {
                CACHE_TTL_DEGRADED
            };
            if at.elapsed() < ttl {
                return Ok(data);
            }
        }
        let mut notes = Vec::new();
        let history = match self.fetch_coinmetrics().await {
            Ok(p) => p,
            Err(e) => {
                notes.push(format!("Coin Metrics indisponible : {e}"));
                Vec::new()
            }
        };
        // Sans historique, on demande tout ce que le plan gratuit sert (4 ans).
        let from = history
            .last()
            .map_or(today - Days::days(4 * 365), |p| p.0 - Days::days(30));
        let recent = match self.fetch_bgeometrics(from, today).await {
            Ok(p) => p,
            Err(e) => {
                notes.push(format!("BGeometrics indisponible : {e}"));
                Vec::new()
            }
        };
        if history.is_empty() && recent.is_empty() {
            return Err(notes.join(" ; "));
        }
        let mut data = merge(&history, &recent);
        data.warnings.splice(0..0, notes);
        let data = Arc::new(data);
        self.cache_put(MVRV_CACHE_KEY, data.clone());
        Ok(data)
    }

    /// Pré-remplit le cache (tests hors ligne).
    #[doc(hidden)]
    pub fn seed_mvrv(&self, history: Series, recent: Series) {
        self.cache_put(MVRV_CACHE_KEY, Arc::new(merge(&history, &recent)));
    }

    async fn fetch_coinmetrics(&self) -> Result<Series, String> {
        let resp = self
            .http
            .get(COINMETRICS_URL)
            .send()
            .await
            .map_err(|e| format!("erreur réseau : {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| format!("réponse illisible : {e}"))?;
        parse_coinmetrics(&text)
    }

    async fn fetch_bgeometrics(&self, from: NaiveDate, to: NaiveDate) -> Result<Series, String> {
        if !self
            .bgeometrics_calls
            .lock()
            .unwrap()
            .try_acquire(Instant::now())
        {
            return Err(
                "quota de requêtes du plan gratuit presque atteint (8 par heure, 15 par jour, partagés avec le tableau de bord) ; nouvel essai plus tard"
                    .into(),
            );
        }
        let resp = self
            .http
            .get(BGEOMETRICS_URL)
            .query(&[
                ("startday", from.format("%Y-%m-%d").to_string()),
                ("endday", to.format("%Y-%m-%d").to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("erreur réseau : {e}"))?;
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err("limite de requêtes atteinte (HTTP 429)".into());
        }
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| format!("réponse illisible : {e}"))?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|_| "réponse illisible (JSON attendu)".to_string())?;
        parse_bgeometrics(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn coinmetrics_columns_are_read_by_header_name() {
        let csv = "time,AdrActCnt,CapMVRVCur,PriceUSD\n\
                   2009-01-03,0,,\n\
                   2017-01-01,1,2.30193946,970\n\
                   2017-01-02,1,2.4,1000\n\
                   2017-01-03,1,,1000\n";
        assert_eq!(
            parse_coinmetrics(csv).unwrap(),
            vec![(d("2017-01-01"), 2.30193946), (d("2017-01-02"), 2.4)]
        );
        // Colonnes dans un autre ordre, fins de ligne Windows, BOM.
        let csv = "\u{feff}CapMVRVCur,time\r\n1.5,2020-05-05\r\n";
        assert_eq!(
            parse_coinmetrics(csv).unwrap(),
            vec![(d("2020-05-05"), 1.5)]
        );
    }

    #[test]
    fn coinmetrics_errors_are_explicit() {
        let err = parse_coinmetrics("time,PriceUSD\n2020-01-01,1\n").unwrap_err();
        assert!(err.contains("CapMVRVCur"), "{err}");
        assert!(parse_coinmetrics("").is_err());
        assert!(parse_coinmetrics("time,CapMVRVCur\n2020-01-01,\n").is_err());
    }

    #[test]
    fn bgeometrics_accepts_the_usual_shapes() {
        let rows = json!([
            {"d": "2026-05-25", "unixTs": "1779667200", "mvrv": "1.41"},
            {"d": "2026-05-26", "unixTs": 1779753600, "mvrv": 1.43},
        ]);
        let expected = vec![(d("2026-05-25"), 1.41), (d("2026-05-26"), 1.43)];
        assert_eq!(parse_bgeometrics(&rows).unwrap(), expected);
        // Enveloppe `data`, métrique sous un autre nom, date en unixTs seulement.
        let wrapped = json!({"metric": "mvrv", "data": [
            {"unixTs": "1779667200", "value": "1.41"},
            {"d": "2026-05-26", "ratio": 1.43},
        ]});
        assert_eq!(parse_bgeometrics(&wrapped).unwrap(), expected);
        assert!(parse_bgeometrics(&json!({"message": "Too Many Requests"}))
            .unwrap_err()
            .contains("Too Many Requests"));
        assert!(parse_bgeometrics(&json!([])).is_err());
        assert!(parse_bgeometrics(&json!([{"d": "2026-05-25", "mvrv": "-1"}])).is_err());
    }

    #[test]
    fn merge_prefers_the_recent_source_and_measures_the_gap() {
        let history = vec![
            (d("2026-05-20"), 1.40),
            (d("2026-05-21"), 1.41),
            (d("2026-05-22"), 1.42),
        ];
        let recent = vec![
            (d("2026-05-21"), 1.4241), // +1 %
            (d("2026-05-22"), 1.4342), // +1 %
            (d("2026-05-23"), 1.44),
        ];
        let m = merge(&history, &recent);
        assert_eq!(m.points.len(), 4);
        assert_eq!(
            m.points[1],
            (d("2026-05-21"), 1.4241),
            "BGeometrics l'emporte"
        );
        assert_eq!(m.points[0].1, 1.40);
        assert_eq!(m.info.overlap_days, 2);
        assert!((m.info.mean_gap_pct.unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(m.info.latest, Some(d("2026-05-23")));
        assert_eq!(m.info.coinmetrics_last, Some(d("2026-05-22")));
        assert!(m.warnings.is_empty(), "écart de 1 % : pas d'alerte");
        assert!(m.complete);
    }

    #[test]
    fn merge_warns_on_a_large_gap_and_on_a_hole() {
        let history = vec![(d("2026-05-01"), 1.0), (d("2026-05-02"), 1.0)];
        let recent = vec![(d("2026-05-02"), 1.2), (d("2026-05-20"), 1.0)];
        let m = merge(&history, &recent);
        assert!(
            m.warnings.iter().any(|w| w.contains("Écart")),
            "{:?}",
            m.warnings
        );
        assert_eq!(m.info.max_gap_pct.map(|g| g.round()), Some(20.0));
        // Pas de recouvrement et un trou de 17 jours.
        let m = merge(&history, &[(d("2026-05-20"), 1.0)]);
        assert!(
            m.warnings.iter().any(|w| w.contains("17 jours")),
            "{:?}",
            m.warnings
        );
        assert_eq!(m.info.overlap_days, 0);
        assert_eq!(m.info.mean_gap_pct, None);
        // Une seule source : jeu incomplet.
        assert!(!merge(&history, &[]).complete);
    }

    #[test]
    fn lookup_takes_the_latest_value_not_after_the_date() {
        let pts = vec![(d("2026-05-01"), 1.0), (d("2026-05-03"), 2.0)];
        assert_eq!(value_on_or_before(&pts, d("2026-05-03"), 3), Some(pts[1]));
        assert_eq!(value_on_or_before(&pts, d("2026-05-02"), 3), Some(pts[0]));
        assert_eq!(value_on_or_before(&pts, d("2026-05-06"), 3), Some(pts[1]));
        assert_eq!(
            value_on_or_before(&pts, d("2026-05-07"), 3),
            None,
            "4 jours"
        );
        assert_eq!(
            value_on_or_before(&pts, d("2026-04-30"), 3),
            None,
            "avant le début"
        );
    }

    #[test]
    fn rate_guard_respects_hourly_and_daily_quotas() {
        let t0 = Instant::now();
        let mut g = RateGuard::new(2, 3);
        assert!(g.try_acquire(t0));
        assert!(g.try_acquire(t0 + Duration::from_secs(60)));
        assert!(!g.try_acquire(t0 + Duration::from_secs(120)), "2 par heure");
        let later = t0 + Duration::from_secs(3700);
        assert!(g.try_acquire(later), "la première heure est écoulée");
        assert!(
            !g.try_acquire(later + Duration::from_secs(3700)),
            "3 sur 24 h"
        );
        assert!(
            g.try_acquire(t0 + Duration::from_secs(25 * 3600)),
            "jour écoulé"
        );
    }

    #[tokio::test]
    async fn seeded_mvrv_avoids_the_network() {
        let feed = MarketFeed::new();
        feed.seed_mvrv(vec![(d("2026-05-01"), 1.0)], vec![(d("2026-05-02"), 1.1)]);
        let data = feed.mvrv(d("2026-05-03")).await.unwrap();
        assert_eq!(data.points.len(), 2);
    }

    /// Contrôle sur le vrai fichier Coin Metrics :
    /// `COINMETRICS_CSV=btc.csv cargo test -p trading-engine real_coinmetrics -- --ignored`
    #[test]
    #[ignore]
    fn real_coinmetrics_file() {
        let path = std::env::var("COINMETRICS_CSV").expect("COINMETRICS_CSV");
        let pts = parse_coinmetrics(&std::fs::read_to_string(path).unwrap()).unwrap();
        let at = |s: &str| pts.iter().find(|p| p.0 == d(s)).map(|p| p.1);
        assert!(at("2017-01-01").is_some_and(|v| (v - 2.30193946).abs() < 1e-6));
        assert!(pts.len() > 5000);
        assert!(pts.last().unwrap().0 >= d("2026-05-01"));
    }
}

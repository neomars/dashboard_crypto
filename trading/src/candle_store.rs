//! Cache disque des bougies fines (1 minute) du moteur de trading.
//!
//! Plusieurs années de bougies 1 minute, ce sont des millions de lignes et
//! environ 1 000 requêtes par an : le cache est obligatoire.
//!
//! **Stockage compact** : un dossier par série (symbole, intervalle, dernier prix
//! ou prix de marque), un fichier binaire par mois (`AAAAMM.bin`), 20 octets par
//! bougie (minute en `u32`, quatre prix en entiers au millième) : environ
//! 40 Mo pour quatre ans de 1 minute. Un fichier `index.json` garde les plages
//! de temps **déjà interrogées** (même vides) : le téléchargement est incrémental
//! et reprenable, et une plage vide (avant la mise en cotation) n'est jamais
//! redemandée. La bougie en cours n'est jamais stockée.
//!
//! Les écritures passent par un fichier temporaire renommé : une interruption ne
//! laisse jamais un fichier tronqué.

use crate::market::{Bar, Interval};
use chrono::{DateTime, Datelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Prix stockés en entiers : millièmes (BTC à 100 000 → 1e8, bien sous `u32::MAX`).
const PRICE_SCALE: f64 = 1000.0;
const RECORD_BYTES: usize = 20;

/// Une série de bougies : un symbole, un intervalle, dernier prix ou prix de marque.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeriesKey {
    pub symbol: String,
    pub interval: Interval,
    pub mark: bool,
}

impl SeriesKey {
    pub fn new(symbol: &str, interval: Interval, mark: bool) -> Self {
        Self {
            symbol: symbol.to_string(),
            interval,
            mark,
        }
    }

    fn dir_name(&self) -> String {
        format!(
            "{}_{}{}",
            self.symbol,
            self.interval.api_name(),
            if self.mark { "_mark" } else { "" }
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    /// Plages `[début, fin)` en ms déjà interrogées, triées et fusionnées.
    covered: Vec<(i64, i64)>,
}

/// Fusionne des plages `[début, fin)` qui se touchent ou se recouvrent.
pub fn merge_ranges(mut ranges: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    ranges.retain(|r| r.0 < r.1);
    ranges.sort();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => out.push(r),
        }
    }
    out
}

/// Parties de `[from, to)` que `covered` ne recouvre pas.
pub fn gaps(covered: &[(i64, i64)], from: i64, to: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut cursor = from;
    for &(s, e) in covered {
        if e <= cursor {
            continue;
        }
        if s >= to {
            break;
        }
        if s > cursor {
            out.push((cursor, s.min(to)));
        }
        cursor = cursor.max(e);
        if cursor >= to {
            break;
        }
    }
    if cursor < to {
        out.push((cursor, to));
    }
    out
}

fn month_key(time_ms: i64) -> u32 {
    let d = DateTime::from_timestamp_millis(time_ms).unwrap_or_default();
    d.year() as u32 * 100 + d.month()
}

fn encode(bars: &BTreeMap<u32, [u32; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bars.len() * RECORD_BYTES);
    for (minute, p) in bars {
        out.extend_from_slice(&minute.to_le_bytes());
        for v in p {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn decode(bytes: &[u8]) -> Result<BTreeMap<u32, [u32; 4]>, String> {
    if !bytes.len().is_multiple_of(RECORD_BYTES) {
        return Err("fichier de bougies tronqué ou corrompu".into());
    }
    let word = |c: &[u8], k: usize| {
        u32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]])
    };
    Ok(bytes
        .chunks_exact(RECORD_BYTES)
        .map(|c| (word(c, 0), [word(c, 1), word(c, 2), word(c, 3), word(c, 4)]))
        .collect())
}

fn to_scaled(price: f64) -> Result<u32, String> {
    let v = (price * PRICE_SCALE).round();
    if !(0.0..=u32::MAX as f64).contains(&v) {
        return Err(format!("prix hors limites du stockage compact : {price}"));
    }
    Ok(v as u32)
}

/// Cache disque des bougies, dans un dossier de données de l'application.
#[derive(Debug, Clone)]
pub struct CandleStore {
    root: PathBuf,
}

impl CandleStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, key: &SeriesKey) -> PathBuf {
        self.root.join(key.dir_name())
    }

    fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Dossier {} : {e}", parent.display()))?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes).map_err(|e| format!("Écriture de {} : {e}", tmp.display()))?;
        fs::rename(&tmp, path).map_err(|e| format!("Remplacement de {} : {e}", path.display()))
    }

    fn index(&self, key: &SeriesKey) -> Index {
        fs::read_to_string(self.dir(key).join("index.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Plages déjà interrogées (triées, fusionnées).
    pub fn coverage(&self, key: &SeriesKey) -> Vec<(i64, i64)> {
        self.index(key).covered
    }

    /// Plages de `[from, to)` pas encore interrogées.
    pub fn missing(&self, key: &SeriesKey, from: i64, to: i64) -> Vec<(i64, i64)> {
        gaps(&self.coverage(key), from, to)
    }

    fn month_path(&self, key: &SeriesKey, month: u32) -> PathBuf {
        self.dir(key).join(format!("{month}.bin"))
    }

    /// Ajoute des bougies et les plages interrogées. Les bougies déjà présentes
    /// sont remplacées. Une seule écriture par mois touché et par appel.
    pub fn append(
        &self,
        key: &SeriesKey,
        bars: &[Bar],
        covered: &[(i64, i64)],
    ) -> Result<(), String> {
        let minute_ms = 60_000i64;
        let mut by_month: BTreeMap<u32, Vec<&Bar>> = BTreeMap::new();
        for b in bars {
            by_month.entry(month_key(b.time)).or_default().push(b);
        }
        for (month, list) in by_month {
            let path = self.month_path(key, month);
            let mut records = match fs::read(&path) {
                Ok(bytes) => decode(&bytes).map_err(|e| format!("{} : {e}", path.display()))?,
                Err(_) => BTreeMap::new(),
            };
            for b in list {
                let minute = u32::try_from(b.time / minute_ms)
                    .map_err(|_| format!("date hors limites : {}", b.time))?;
                records.insert(
                    minute,
                    [
                        to_scaled(b.open)?,
                        to_scaled(b.high)?,
                        to_scaled(b.low)?,
                        to_scaled(b.close)?,
                    ],
                );
            }
            Self::write_atomic(&path, &encode(&records))?;
        }
        let mut index = self.index(key);
        index.version = 1;
        index.covered.extend_from_slice(covered);
        index.covered = merge_ranges(std::mem::take(&mut index.covered));
        let text = serde_json::to_vec(&index).map_err(|e| e.to_string())?;
        Self::write_atomic(&self.dir(key).join("index.json"), &text)
    }

    /// Bougies dont l'ouverture est dans `[from, to)`, triées.
    pub fn read(&self, key: &SeriesKey, from: i64, to: i64) -> Result<Vec<Bar>, String> {
        let mut out = Vec::new();
        if from >= to {
            return Ok(out);
        }
        let (first, last) = (month_key(from), month_key(to - 1));
        let mut months: Vec<u32> = fs::read_dir(self.dir(key))
            .map(|d| {
                d.filter_map(|e| {
                    let name = e.ok()?.file_name().into_string().ok()?;
                    name.strip_suffix(".bin")?.parse::<u32>().ok()
                })
                .filter(|m| (first..=last).contains(m))
                .collect()
            })
            .unwrap_or_default();
        months.sort_unstable();
        for month in months {
            let path = self.month_path(key, month);
            let bytes =
                fs::read(&path).map_err(|e| format!("Lecture de {} : {e}", path.display()))?;
            for (minute, p) in decode(&bytes).map_err(|e| format!("{} : {e}", path.display()))? {
                let time = minute as i64 * 60_000;
                if time >= from && time < to {
                    let f = |v: u32| v as f64 / PRICE_SCALE;
                    out.push(Bar {
                        time,
                        open: f(p[0]),
                        high: f(p[1]),
                        low: f(p[2]),
                        close: f(p[3]),
                        volume: 0.0,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Première et dernière bougie stockées, si la série n'est pas vide.
    pub fn bounds(&self, key: &SeriesKey) -> Result<Option<(i64, i64)>, String> {
        let mut months: Vec<u32> = fs::read_dir(self.dir(key))
            .map(|d| {
                d.filter_map(|e| {
                    let name = e.ok()?.file_name().into_string().ok()?;
                    name.strip_suffix(".bin")?.parse::<u32>().ok()
                })
                .collect()
            })
            .unwrap_or_default();
        months.sort_unstable();
        let edge = |month: u32, first: bool| -> Result<Option<i64>, String> {
            let path = self.month_path(key, month);
            let bytes =
                fs::read(&path).map_err(|e| format!("Lecture de {} : {e}", path.display()))?;
            let rec = decode(&bytes)?;
            Ok(if first {
                rec.keys().next()
            } else {
                rec.keys().next_back()
            }
            .map(|m| *m as i64 * 60_000))
        };
        let (Some(&a), Some(&b)) = (months.first(), months.last()) else {
            return Ok(None);
        };
        Ok(edge(a, true)?.zip(edge(b, false)?))
    }

    /// Taille du cache sur disque, en octets.
    pub fn size_bytes(&self) -> u64 {
        fn walk(dir: &Path) -> u64 {
            fs::read_dir(dir)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .map(|e| match e.metadata() {
                            Ok(m) if m.is_dir() => walk(&e.path()),
                            Ok(m) => m.len(),
                            Err(_) => 0,
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
        walk(&self.root)
    }

    /// Lit un petit fichier JSON du dossier (fiche du contrat, par exemple).
    pub fn read_json<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<T> {
        serde_json::from_str(&fs::read_to_string(self.root.join(name)).ok()?).ok()
    }

    pub fn write_json<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        Self::write_atomic(&self.root.join(name), &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::DAY_MS;

    /// Dossier temporaire unique, supprimé à la fin du test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir()
                .join(format!("fibo-store-{tag}-{}-{nanos}", std::process::id()));
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn bar(minute: i64, base: f64) -> Bar {
        Bar {
            time: minute * 60_000,
            open: base,
            high: base + 1.25,
            low: base - 0.5,
            close: base + 0.1,
            volume: 0.0,
        }
    }

    fn key() -> SeriesKey {
        SeriesKey::new("BTC_USDT_PERP", Interval::M1, false)
    }

    #[test]
    fn ranges_merge_and_gaps_are_found() {
        assert_eq!(
            merge_ranges(vec![(5, 8), (0, 3), (3, 4), (7, 10), (20, 20)]),
            vec![(0, 4), (5, 10)]
        );
        let covered = vec![(10, 20), (30, 40)];
        assert_eq!(gaps(&covered, 0, 50), vec![(0, 10), (20, 30), (40, 50)]);
        assert_eq!(gaps(&covered, 12, 18), vec![]);
        assert_eq!(gaps(&covered, 15, 35), vec![(20, 30)]);
        assert_eq!(gaps(&[], 5, 9), vec![(5, 9)]);
        assert_eq!(gaps(&covered, 40, 45), vec![(40, 45)]);
    }

    #[test]
    fn bars_round_trip_exactly_across_months() {
        let dir = Scratch::new("rt");
        let store = CandleStore::new(&dir.0);
        let t_jan_end = 31 * 1440; // 1er février 1970, minute
        let bars = vec![
            bar(t_jan_end - 2, 100.0),
            bar(t_jan_end - 1, 101.5),
            bar(t_jan_end, 99999.999),
            bar(t_jan_end + 1, 7.001),
        ];
        store
            .append(&key(), &bars, &[(bars[0].time, bars[3].time + 60_000)])
            .unwrap();
        let back = store.read(&key(), 0, 100_000 * 60_000).unwrap();
        assert_eq!(back.len(), bars.len());
        for (a, b) in back.iter().zip(&bars) {
            assert_eq!(a.time, b.time);
            for (x, y) in [
                (a.open, b.open),
                (a.high, b.high),
                (a.low, b.low),
                (a.close, b.close),
            ] {
                assert!((x - y).abs() < 5e-4, "prix au millième : {x} / {y}");
            }
        }
        // Lecture d'une plage : bornes [from, to).
        let part = store.read(&key(), bars[1].time, bars[3].time).unwrap();
        assert_eq!(
            part.iter().map(|b| b.time).collect::<Vec<_>>(),
            vec![bars[1].time, bars[2].time]
        );
        assert_eq!(
            store.bounds(&key()).unwrap(),
            Some((bars[0].time, bars[3].time))
        );
        assert!(
            fs::read_dir(dir.0.join("BTC_USDT_PERP_MINUTE_1"))
                .unwrap()
                .count()
                >= 3,
            "2 mois + index"
        );
        // 20 octets par bougie.
        let jan = fs::metadata(dir.0.join("BTC_USDT_PERP_MINUTE_1/197001.bin"))
            .unwrap()
            .len();
        assert_eq!(jan, 2 * 20);
    }

    #[test]
    fn appending_merges_with_existing_bars_and_coverage() {
        let dir = Scratch::new("merge");
        let store = CandleStore::new(&dir.0);
        store
            .append(&key(), &[bar(10, 1.0), bar(11, 2.0)], &[(600_000, 720_000)])
            .unwrap();
        store
            .append(&key(), &[bar(11, 3.0), bar(12, 4.0)], &[(720_000, 780_000)])
            .unwrap();
        let all = store.read(&key(), 0, 100 * 60_000).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].open, 3.0, "la bougie réécrite remplace l'ancienne");
        assert_eq!(store.coverage(&key()), vec![(600_000, 780_000)]);
        assert_eq!(
            store.missing(&key(), 0, 1_000_000),
            vec![(0, 600_000), (780_000, 1_000_000)]
        );
        // Une plage interrogée sans bougie (avant la cotation) est conservée.
        store.append(&key(), &[], &[(0, 600_000)]).unwrap();
        assert_eq!(store.coverage(&key()), vec![(0, 780_000)]);
        assert!(store.size_bytes() > 0);
    }

    #[test]
    fn series_are_independent_and_unknown_ones_are_empty() {
        let dir = Scratch::new("keys");
        let store = CandleStore::new(&dir.0);
        let mark = SeriesKey::new("BTC_USDT_PERP", Interval::M1, true);
        let spot = SeriesKey::new("BTC_USDT", Interval::M1, false);
        store
            .append(&key(), &[bar(1, 5.0)], &[(0, 120_000)])
            .unwrap();
        assert!(store.read(&mark, 0, DAY_MS).unwrap().is_empty());
        assert!(store.read(&spot, 0, DAY_MS).unwrap().is_empty());
        assert_eq!(store.bounds(&spot).unwrap(), None);
        assert_eq!(store.coverage(&mark), vec![]);
        assert_eq!(store.missing(&mark, 0, 60_000), vec![(0, 60_000)]);
        assert_eq!(mark.dir_name(), "BTC_USDT_PERP_MINUTE_1_mark");
    }

    #[test]
    fn corrupt_files_are_reported_not_silently_ignored() {
        let dir = Scratch::new("bad");
        let store = CandleStore::new(&dir.0);
        store
            .append(&key(), &[bar(5, 1.0)], &[(0, 600_000)])
            .unwrap();
        let path = dir.0.join("BTC_USDT_PERP_MINUTE_1/197001.bin");
        fs::write(&path, [1u8, 2, 3]).unwrap();
        assert!(store
            .read(&key(), 0, DAY_MS)
            .unwrap_err()
            .contains("corrompu"));
        // Prix hors limites du stockage compact.
        let huge = Bar {
            open: 1e12,
            high: 1e12,
            low: 1e12,
            close: 1e12,
            ..bar(1, 1.0)
        };
        assert!(store.append(&key(), &[huge], &[]).is_err());
    }

    #[test]
    fn small_json_files_round_trip() {
        let dir = Scratch::new("json");
        let store = CandleStore::new(&dir.0);
        assert_eq!(store.read_json::<Vec<u32>>("x.json"), None);
        store.write_json("x.json", &vec![1u32, 2, 3]).unwrap();
        assert_eq!(store.read_json::<Vec<u32>>("x.json"), Some(vec![1, 2, 3]));
    }
}

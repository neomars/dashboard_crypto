//! API publique Binance Futures (données de marché, sans clé) : ratio long/short
//! des « top traders » des contrats perpétuels USDⓈ-M, et cours (bougies) du
//! même contrat.
//!
//! Binance appelle « top traders » les 20 % de comptes ayant le plus de marge.
//! Le ratio de comptes compte chaque compte une fois, quelle que soit la taille
//! de sa position. L'API ne publie que les 30 derniers jours de ce ratio ; au-delà, on lit
//! l'archive publique de Binance (un fichier zip par jour et par contrat, un relevé toutes les
//! 5 minutes), dont les colonnes ne sont pas documentées : le raccord avec l'API vérifie donc
//! que les deux séries concordent sur leurs jours communs avant de s'en servir. Les bougies
//! n'ont pas la limite de 30 jours de l'API.

use crate::table::value_as_f64;
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;
use std::io::Read;

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
/// Jours de bougies demandés en plus de l'historique du ratio : le premier relevé
/// peut remonter un peu au-delà (jusqu'à une période, soit un jour au plus), et
/// le cours doit commencer avec lui.
pub const KLINES_MARGIN_DAYS: i64 = 3;
/// Adresse de l'archive publique de Binance.
pub const ARCHIVE_URL: &str = "https://data.binance.vision";
const ARCHIVE_PATH: &str = "/data/futures/um/daily/metrics";
/// Profondeur maximale de l'historique, en jours (chaque jour est un fichier à télécharger).
pub const MAX_HISTORY_DAYS: i64 = 730;
/// Nombre maximal de relevés tracés : au-delà, la figure devient lente. Les 30 jours de l'API en
/// période de 5 minutes (8 640 relevés) restent permis.
pub const MAX_POINTS: i64 = 6_000;
/// Les relevés de l'archive sont toutes les 5 minutes : un relevé est apparié à une date de la
/// période s'il en est à 5 minutes au plus (la convention des dates du fichier a changé).
const ARCHIVE_STEP_MS: i64 = 300_000;
/// Écart relatif médian au-delà duquel l'archive et l'API ne mesurent pas la même chose.
pub const MAX_DISAGREEMENT: f64 = 0.10;
/// Nombre minimal de relevés communs pour juger de la concordance.
pub const MIN_COMMON_POINTS: usize = 5;
/// Taille maximale d'un fichier CSV décompressé (un jour pèse environ 30 Ko) : garde-fou.
const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;
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

/// Première bougie demandée : les `days` jours de l'historique, plus une marge (l'indicateur
/// n'affiche les bougies qu'à partir du premier relevé).
pub fn kline_window_start(now_ms: i64, days: i64) -> i64 {
    now_ms - (days + KLINES_MARGIN_DAYS) * DAY_MS
}

/// Profondeur de l'historique réellement servie : celle demandée, limitée à [`MAX_POINTS`]
/// relevés pour la période (mais jamais en dessous des 30 jours de l'API) et à
/// [`MAX_HISTORY_DAYS`].
pub fn history_days(requested: i64, period_ms: i64) -> i64 {
    let cap = (MAX_POINTS * period_ms / DAY_MS).clamp(HISTORY_DAYS, MAX_HISTORY_DAYS);
    requested.clamp(1, cap)
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

/// Fichier d'archive d'un jour :
/// `{base}/data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-2026-05-14.zip`.
pub fn archive_url(base: &str, symbol: &str, day: NaiveDate) -> String {
    format!("{base}{ARCHIVE_PATH}/{symbol}/{symbol}-metrics-{day}.zip")
}

/// Accès à la valeur d'une colonne ratio d'une ligne de l'archive.
type Pick = fn(&ArchiveRow) -> Option<f64>;

/// Les deux colonnes candidates : leur nom d'en-tête et l'accès à leur valeur.
const COLUMNS: [(&str, Pick); 2] = [(COUNT_COLUMN, |r| r.count), (SUM_COLUMN, |r| r.sum)];

/// Colonne de l'archive la plus proche de l'API.
struct Match {
    column: &'static str,
    pick: Pick,
    gap: f64,
    common: usize,
}

/// Un relevé de l'archive : les deux ratios « top traders » du fichier. Leurs noms (`count_…` et
/// `sum_…`) ne disent pas lequel est le ratio de comptes : [`stitch`] le tranche en les comparant
/// à l'API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArchiveRow {
    /// Date du relevé (millisecondes depuis 1970, UTC).
    pub ts: i64,
    /// Colonne `count_toptrader_long_short_ratio`.
    pub count: Option<f64>,
    /// Colonne `sum_toptrader_long_short_ratio`.
    pub sum: Option<f64>,
}

const TIME_COLUMN: &str = "create_time";
const COUNT_COLUMN: &str = "count_toptrader_long_short_ratio";
const SUM_COLUMN: &str = "sum_toptrader_long_short_ratio";

/// Le CSV du jour, extrait du zip téléchargé (un fichier par archive).
pub fn unzip_csv(bytes: &[u8]) -> Result<String, String> {
    let unreadable = |e: &dyn std::fmt::Display| format!("Archive Binance illisible (zip) : {e}");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| unreadable(&e))?;
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| unreadable(&e))?;
        let is_csv = file
            .name()
            .is_ok_and(|name| name.to_ascii_lowercase().ends_with(".csv"));
        if !file.is_file() || !is_csv {
            continue;
        }
        if file.size() > MAX_CSV_BYTES {
            return Err(format!(
                "Archive Binance trop volumineuse : {} octets décompressés.",
                file.size()
            ));
        }
        let mut text = String::new();
        file.take(MAX_CSV_BYTES)
            .read_to_string(&mut text)
            .map_err(|e| unreadable(&e))?;
        return Ok(text);
    }
    Err("Archive Binance sans fichier CSV.".into())
}

/// Date d'un relevé : `2026-05-14 08:05:00` (UTC), une date seule (`2023-01-21`, minuit), ou un
/// horodatage en millisecondes.
fn parse_archive_time(s: &str) -> Option<i64> {
    let s = s.trim().trim_matches('"');
    if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(t.and_utc().timestamp_millis());
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    s.parse::<i64>().ok().filter(|ms| *ms > 100_000_000_000)
}

/// Décode le CSV d'un jour : les colonnes se repèrent par leur nom dans l'en-tête, les lignes
/// illisibles ou sans date sont ignorées, un ratio vide (dénominateur nul) ou non positif vaut
/// `None`. Trié par date.
pub fn parse_archive_csv(text: &str) -> Result<Vec<ArchiveRow>, String> {
    let mut lines = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let header = lines.next().ok_or("Fichier d'archive Binance vide.")?;
    let columns: Vec<&str> = header
        .split(',')
        .map(|c| c.trim().trim_matches('"'))
        .collect();
    let index = |name: &str| columns.iter().position(|c| *c == name);
    let (time, count, sum) = (index(TIME_COLUMN), index(COUNT_COLUMN), index(SUM_COLUMN));
    let Some(time) = time.filter(|_| count.is_some() || sum.is_some()) else {
        return Err(format!(
            "Archive Binance : colonnes {TIME_COLUMN} et {COUNT_COLUMN} ou {SUM_COLUMN} introuvables (en-tête : {header})."
        ));
    };
    let ratio = |fields: &[&str], column: Option<usize>| {
        let value = fields
            .get(column?)?
            .trim()
            .trim_matches('"')
            .parse::<f64>()
            .ok()?;
        (value.is_finite() && value > 0.0).then_some(value)
    };
    let mut rows: Vec<ArchiveRow> = lines
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            Some(ArchiveRow {
                ts: parse_archive_time(fields.get(time)?)?,
                count: ratio(&fields, count),
                sum: ratio(&fields, sum),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.ts);
    Ok(rows)
}

/// Valeur (colonne choisie par `pick`) du relevé de l'archive le plus proche de `at`, à 5 minutes
/// près ; les relevés où la colonne est vide sont ignorés. `rows` est trié par date.
fn nearest(rows: &[ArchiveRow], pick: Pick, at: i64) -> Option<f64> {
    let first = rows.partition_point(|r| r.ts < at - ARCHIVE_STEP_MS);
    rows[first..]
        .iter()
        .take_while(|r| r.ts <= at + ARCHIVE_STEP_MS)
        .filter_map(|r| Some(((r.ts - at).abs(), pick(r)?)))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, value)| value)
}

fn point(ts: i64, ratio: f64) -> TopTraderPoint {
    TopTraderPoint {
        ts,
        ratio,
        long: ratio / (1.0 + ratio),
        short: 1.0 / (1.0 + ratio),
    }
}

/// Relevés de l'archive aux dates de la période (multiples de `period_ms` depuis 1970, comme
/// l'API), de `from` (inclus) à `until` (exclu). Une date sans relevé voisin est sautée.
fn resample(
    rows: &[ArchiveRow],
    pick: Pick,
    period_ms: i64,
    from: i64,
    until: i64,
) -> Vec<TopTraderPoint> {
    let mut points = Vec::new();
    let mut at = (from + period_ms - 1).div_euclid(period_ms) * period_ms;
    while at < until {
        if let Some(ratio) = nearest(rows, pick, at) {
            points.push(point(at, ratio));
        }
        at += period_ms;
    }
    points
}

/// Écarts relatifs, triés, entre les relevés de l'API et la colonne `pick` de l'archive aux
/// mêmes dates.
fn gaps(api: &[TopTraderPoint], rows: &[ArchiveRow], pick: Pick) -> Vec<f64> {
    let mut gaps: Vec<f64> = api
        .iter()
        .filter_map(|p| Some((nearest(rows, pick, p.ts)? - p.ratio).abs() / p.ratio))
        .collect();
    gaps.sort_by(f64::total_cmp);
    gaps
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// Jours de l'API, les plus anciens, que l'on compare à l'archive : de quoi réunir une douzaine de
/// relevés (deux jours en période de 4 heures, douze en période d'un jour).
pub fn overlap_days(period_ms: i64) -> i64 {
    ((12 * period_ms + DAY_MS - 1) / DAY_MS).clamp(2, 14)
}

/// L'historique complet : les relevés de l'archive plus anciens que l'API, puis ceux de l'API.
#[derive(Debug, Clone, PartialEq)]
pub struct Stitched {
    pub points: Vec<TopTraderPoint>,
    /// Nombre de relevés venus de l'archive (les premiers de `points`) ; 0 si elle n'a rien de
    /// plus ancien que l'API, auquel cas les trois champs suivants sont vides.
    pub from_archive: usize,
    /// Colonne de l'archive qui concorde avec le ratio de comptes de l'API.
    pub column: &'static str,
    /// Écart relatif médian entre l'archive et l'API sur les relevés communs (0,01 = 1 %).
    pub gap: f64,
    /// Nombre de relevés communs comparés.
    pub common: usize,
}

/// Ce que l'archive a apporté à l'historique du ratio.
#[derive(Debug, Clone, PartialEq)]
pub enum ArchiveReport {
    /// Non demandée : la profondeur voulue tient dans les 30 jours de l'API.
    NotNeeded,
    /// L'archive n'a rien de plus ancien que l'API (contrat récent).
    Nothing,
    /// `added` relevés plus anciens que l'API ont été ajoutés, après vérification de leur
    /// concordance avec elle ; `missing_days` jours de l'archive n'ont pas pu être lus (`cause`).
    Added {
        added: usize,
        gap: f64,
        common: usize,
        missing_days: usize,
        cause: Option<String>,
    },
    /// L'archive n'a pas pu servir : la raison, à montrer à l'utilisateur.
    Failed(String),
}

/// Ratio des top traders sur la profondeur demandée, avec la provenance de ses relevés.
#[derive(Debug, Clone, PartialEq)]
pub struct TopTraderHistory {
    pub points: Vec<TopTraderPoint>,
    pub archive: ArchiveReport,
}

/// Raccorde l'archive à l'API. `api` est trié et non vide, `rows` (trié, sans doublon) couvre la
/// profondeur voulue et les [`overlap_days`] premiers jours de l'API, `since` est le début voulu
/// de l'historique.
///
/// Les colonnes de l'archive ne sont pas documentées par Binance : on compare chacune à l'API sur
/// les jours communs et on ne garde que celle qui concorde (écart médian d'au plus
/// [`MAX_DISAGREEMENT`]). Sans concordance, ou sans assez de relevés communs pour en juger,
/// l'archive est refusée plutôt que de montrer une autre grandeur à la suite de celle de l'API.
pub fn stitch(
    api: &[TopTraderPoint],
    rows: &[ArchiveRow],
    period_ms: i64,
    since: i64,
) -> Result<Stitched, String> {
    let first_api = api.first().ok_or("Aucun relevé de l'API.")?.ts;
    let kept = api.partition_point(|p| p.ts < since);
    let nothing = || Stitched {
        points: api[kept..].to_vec(),
        from_archive: 0,
        column: "",
        gap: 0.0,
        common: 0,
    };
    if !rows.iter().any(|r| r.ts < first_api) {
        return Ok(nothing());
    }

    let overlap_end = first_api + overlap_days(period_ms) * DAY_MS;
    let overlap = &api[..api.partition_point(|p| p.ts < overlap_end)];
    let mut best: Option<Match> = None;
    let mut most_common = 0;
    for (column, pick) in COLUMNS {
        let gaps = gaps(overlap, rows, pick);
        most_common = most_common.max(gaps.len());
        if gaps.len() < MIN_COMMON_POINTS {
            continue;
        }
        let gap = median(&gaps);
        if best.as_ref().is_none_or(|b| gap < b.gap) {
            best = Some(Match {
                column,
                pick,
                gap,
                common: gaps.len(),
            });
        }
    }
    let Some(Match {
        column,
        pick,
        gap,
        common,
    }) = best
    else {
        return Err(format!(
            "Seulement {most_common} relevé(s) en commun avec l'API sur ses {} premiers jours : il en faut au moins {MIN_COMMON_POINTS} pour vérifier que l'archive mesure la même chose.",
            overlap_days(period_ms)
        ));
    };
    if gap > MAX_DISAGREEMENT {
        let (gap, tolerated) = (
            format!("{:.1} %", gap * 100.0).replace('.', ","),
            format!("{:.0} %", MAX_DISAGREEMENT * 100.0),
        );
        return Err(format!(
            "L'archive ne concorde pas avec l'API (écart médian de {gap} sur {common} relevés communs, au plus {tolerated} toléré) : elle ne mesure pas la même chose."
        ));
    }

    let mut points = resample(rows, pick, period_ms, since, first_api);
    let from_archive = points.len();
    points.extend_from_slice(&api[kept..]);
    Ok(Stitched {
        points,
        from_archive,
        column,
        gap,
        common,
    })
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
            kline_window_start(now, HISTORY_DAYS),
            7 * DAY_MS,
            "33 jours avant « maintenant »"
        );
        assert_eq!(
            kline_window_start(now, 180),
            (40 - 183) * DAY_MS,
            "la profondeur demandée, plus la marge"
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

    const HEADER: &str = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio";
    /// 2026-05-14 00:00:00 UTC
    const D0: i64 = 1_778_716_800_000;
    const HOUR: i64 = 3_600_000;
    const FIVE_MIN: i64 = 300_000;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn zip_of(name: &str, content: &str) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// Ratio de comptes synthétique, lent : deux relevés de 5 minutes voisins diffèrent de moins
    /// de 0,01 %.
    fn ratio_at(ts: i64) -> f64 {
        1.5 + 0.4 * (ts as f64 / DAY_MS as f64 / 3.0).sin()
    }

    /// Relevés de l'archive toutes les 5 minutes de `from` (inclus) à `until` (exclu) : la colonne
    /// `count` suit `count_of`, la colonne `sum` suit `sum_of`.
    fn rows_between(
        from: i64,
        until: i64,
        count_of: fn(f64) -> f64,
        sum_of: fn(f64) -> f64,
    ) -> Vec<ArchiveRow> {
        (from / FIVE_MIN..until / FIVE_MIN)
            .map(|i| i * FIVE_MIN)
            .map(|ts| ArchiveRow {
                ts,
                count: Some(count_of(ratio_at(ts))),
                sum: Some(sum_of(ratio_at(ts))),
            })
            .collect()
    }

    fn api_between(from: i64, until: i64, period_ms: i64) -> Vec<TopTraderPoint> {
        (from / period_ms..until / period_ms)
            .map(|i| i * period_ms)
            .map(|ts| point(ts, ratio_at(ts)))
            .collect()
    }

    fn same(r: f64) -> f64 {
        r
    }

    fn other(r: f64) -> f64 {
        r * 0.6
    }

    #[test]
    fn archive_files_hang_under_the_archive_address() {
        assert_eq!(
            archive_url(ARCHIVE_URL, "BTCUSDT", day("2026-05-14")),
            "https://data.binance.vision/data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-2026-05-14.zip"
        );
        assert_eq!(
            archive_url("http://127.0.0.1:9", "1000PEPEUSDT", day("2024-01-02")),
            "http://127.0.0.1:9/data/futures/um/daily/metrics/1000PEPEUSDT/1000PEPEUSDT-metrics-2024-01-02.zip"
        );
    }

    #[test]
    fn parses_the_rows_of_a_day() {
        let csv = format!(
            "{HEADER}\n\
             2026-05-14 00:05:00,BTCUSDT,80000.5,6.5e9,1.8105,1.2000,2.1,0.97\n\
             2026-05-14 00:10:00,BTCUSDT,80000.5,6.5e9,,1.25,2.1,0.97\n\
             2026-05-14 00:00:00,BTCUSDT,80000.5,6.5e9,1.7,0,2.1,\n\
             pas une ligne\n\
             not-a-date,BTCUSDT,1,1,1.5,1.5,1,1\n\
             \n"
        );
        let rows = parse_archive_csv(&csv).unwrap();
        // Triées par date ; un ratio vide ou nul vaut « absent ».
        assert_eq!(
            rows,
            [
                ArchiveRow {
                    ts: D0,
                    count: Some(1.7),
                    sum: None
                },
                ArchiveRow {
                    ts: D0 + FIVE_MIN,
                    count: Some(1.8105),
                    sum: Some(1.2)
                },
                ArchiveRow {
                    ts: D0 + 2 * FIVE_MIN,
                    count: None,
                    sum: Some(1.25)
                },
            ]
        );
    }

    #[test]
    fn columns_are_found_by_name_and_the_format_is_tolerant() {
        // Autre ordre, guillemets, fins de ligne Windows, BOM : seuls les noms comptent.
        let csv = "\u{feff}\"sum_toptrader_long_short_ratio\",\"create_time\",\"count_toptrader_long_short_ratio\"\r\n\
                   \"1.2\",\"2026-05-14 00:00:00\",\"1.7\"\r\n";
        let rows = parse_archive_csv(csv).unwrap();
        assert_eq!(
            rows,
            [ArchiveRow {
                ts: D0,
                count: Some(1.7),
                sum: Some(1.2)
            }]
        );

        // Une seule des deux colonnes suffit à lire le fichier.
        let csv = "create_time,count_toptrader_long_short_ratio\n2026-05-14 00:00:00,1.7\n";
        let rows = parse_archive_csv(csv).unwrap();
        assert_eq!(
            rows,
            [ArchiveRow {
                ts: D0,
                count: Some(1.7),
                sum: None
            }]
        );
    }

    #[test]
    fn unexpected_files_are_reported() {
        let e = parse_archive_csv("").unwrap_err();
        assert!(e.contains("vide"), "{e}");
        let e = parse_archive_csv("a,b,c\n1,2,3\n").unwrap_err();
        assert!(e.contains("create_time") && e.contains("a,b,c"), "{e}");
        let e = parse_archive_csv("create_time,symbol\n2026-05-14 00:00:00,BTCUSDT\n").unwrap_err();
        assert!(e.contains(COUNT_COLUMN), "{e}");
        // Un en-tête sans ligne de données : un jour sans relevé, pas une erreur.
        assert!(parse_archive_csv(&format!("{HEADER}\n"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn archive_dates_come_in_three_forms() {
        assert_eq!(parse_archive_time("2026-05-14 00:00:00"), Some(D0));
        assert_eq!(
            parse_archive_time("2026-05-14 01:05:00"),
            Some(D0 + HOUR + FIVE_MIN)
        );
        assert_eq!(
            parse_archive_time("2026-05-14 00:00:02.5"),
            Some(D0 + 2_500)
        );
        assert_eq!(parse_archive_time(" \"2026-05-14 00:00:00\" "), Some(D0));
        // Les premiers fichiers n'avaient qu'une date : minuit.
        assert_eq!(parse_archive_time("2026-05-14"), Some(D0));
        assert_eq!(parse_archive_time(&D0.to_string()), Some(D0));
        for bad in ["", "hier", "2026-13-40 00:00:00", "1778716800"] {
            assert_eq!(parse_archive_time(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn reads_the_csv_of_a_zip() {
        let csv = format!("{HEADER}\n2026-05-14 00:00:00,BTCUSDT,1,1,1.7,1.2,1,1\n");
        let zip = zip_of("BTCUSDT-metrics-2026-05-14.csv", &csv);
        assert_eq!(unzip_csv(&zip).unwrap(), csv);

        // Un corps qui n'est pas un zip (page d'erreur d'un proxy...) est signalé.
        let e = unzip_csv(b"<html>Access denied</html>").unwrap_err();
        assert!(e.contains("zip"), "{e}");
        // Un zip sans CSV aussi.
        let e = unzip_csv(&zip_of("lisez-moi.txt", "bonjour")).unwrap_err();
        assert!(e.contains("CSV"), "{e}");
    }

    #[test]
    fn resampling_picks_the_row_of_each_period_date_in_either_label_convention() {
        let period = 4 * HOUR;
        // Le fichier de la veille et celui du jour, mis bout à bout : avant le 25/06/2026 leurs
        // étiquettes vont de 00:05 à 00:00 le lendemain, ensuite de 00:00 à 23:55.
        let rows_of = |first_label: i64| {
            let mut rows = rows_between(first_label, first_label + 2 * DAY_MS, same, other);
            // La valeur d'un relevé est celle de son étiquette : on peut les distinguer.
            rows.iter_mut()
                .for_each(|r| r.count = Some(r.ts as f64 / 1e9));
            rows
        };
        for rows in [rows_of(D0 - DAY_MS + FIVE_MIN), rows_of(D0 - DAY_MS)] {
            let points = resample(&rows, |r| r.count, period, D0, D0 + DAY_MS);
            assert_eq!(
                points.iter().map(|p| p.ts).collect::<Vec<_>>(),
                (0..6).map(|i| D0 + i * period).collect::<Vec<_>>()
            );
            for p in &points {
                assert_eq!(
                    p.ratio,
                    p.ts as f64 / 1e9,
                    "la valeur de l'étiquette exacte"
                );
            }
        }
    }

    #[test]
    fn resampling_skips_dates_without_a_nearby_row_and_keeps_the_period_grid() {
        let mut rows = rows_between(D0, D0 + DAY_MS, same, same);
        // Plus aucun relevé entre 07:30 et 12:30 : les dates 08:00 et 12:00 sont sautées.
        rows.retain(|r| r.ts < D0 + 7 * HOUR + 30 * 60_000 || r.ts > D0 + 12 * HOUR + 30 * 60_000);
        // Un relevé hors grille (02:02:30) sert la date 02:00 (à 5 minutes près), pas 03:00.
        let mut rows: Vec<ArchiveRow> =
            rows.into_iter().filter(|r| r.ts != D0 + 2 * HOUR).collect();
        rows.push(ArchiveRow {
            ts: D0 + 2 * HOUR + 150_000,
            count: Some(9.0),
            sum: Some(9.0),
        });
        rows.sort_by_key(|r| r.ts);
        let hourly = resample(&rows, |r| r.count, HOUR, D0, D0 + 4 * HOUR);
        assert_eq!(
            hourly
                .iter()
                .map(|p| (p.ts - D0) / HOUR)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            hourly[2].ratio, 9.0,
            "le relevé hors grille est le plus proche de 02:00"
        );

        let four_hourly = resample(&rows, |r| r.count, 4 * HOUR, D0, D0 + DAY_MS);
        assert_eq!(
            four_hourly
                .iter()
                .map(|p| (p.ts - D0) / HOUR)
                .collect::<Vec<_>>(),
            [0, 4, 16, 20],
            "08:00 et 12:00 n'ont aucun relevé à moins de 5 minutes"
        );

        // La première date est la première de la grille à partir de `from`.
        let from_mid_period = resample(&rows, |r| r.count, 4 * HOUR, D0 + 30 * 60_000, D0 + DAY_MS);
        assert_eq!(from_mid_period[0].ts, D0 + 4 * HOUR);
        // `until` est exclu.
        let until_is_excluded = resample(&rows, |r| r.count, 4 * HOUR, D0, D0 + 4 * HOUR);
        assert_eq!(until_is_excluded.len(), 1);
    }

    #[test]
    fn shares_follow_from_the_resampled_ratio() {
        let rows = [ArchiveRow {
            ts: D0,
            count: Some(3.0),
            sum: None,
        }];
        let p = resample(&rows, |r| r.count, HOUR, D0, D0 + HOUR);
        assert_eq!(p.len(), 1);
        assert!((p[0].long - 0.75).abs() < 1e-12 && (p[0].short - 0.25).abs() < 1e-12);
        assert!(
            resample(&rows, |r| r.sum, HOUR, D0, D0 + HOUR).is_empty(),
            "une colonne vide ne donne aucun relevé"
        );
    }

    #[test]
    fn the_archive_is_joined_to_the_api_when_both_agree() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let now = D0 + 50 * DAY_MS;
        let api = api_between(api_start, now, period);
        let since = D0 + 3 * DAY_MS + 30 * 60_000;
        // Archive : de la veille du début voulu jusqu'à l'ouverture de l'API plus les jours de comparaison.
        let rows = rows_between(
            D0 + 2 * DAY_MS,
            api_start + (overlap_days(period) + 1) * DAY_MS,
            same,
            other,
        );
        let s = stitch(&api, &rows, period, since).unwrap();

        assert_eq!(s.column, COUNT_COLUMN);
        assert!(s.gap < 1e-3, "{}", s.gap);
        assert!(s.common >= MIN_COMMON_POINTS);
        // D'abord l'archive, à partir de la première date de la période après `since`, puis l'API.
        assert_eq!(s.points[0].ts, D0 + 3 * DAY_MS + 4 * HOUR);
        assert_eq!(s.points[s.from_archive].ts, api_start);
        assert_eq!(s.points.len(), s.from_archive + api.len());
        assert_eq!(
            s.from_archive as i64,
            (api_start - (D0 + 3 * DAY_MS + 4 * HOUR)) / period
        );
        // Sans trou ni doublon : une date par période, strictement croissantes.
        for pair in s.points.windows(2) {
            assert_eq!(pair[1].ts - pair[0].ts, period, "{pair:?}");
        }
        // La valeur de l'archive est celle de la colonne retenue, et les parts en découlent.
        assert!((s.points[0].ratio - ratio_at(s.points[0].ts)).abs() < 1e-12);
        let p = s.points[1];
        assert!((p.long - p.ratio / (1.0 + p.ratio)).abs() < 1e-12);
        // L'API garde ses propres valeurs.
        assert_eq!(&s.points[s.from_archive..], &api[..]);
    }

    #[test]
    fn the_column_that_agrees_with_the_api_is_the_one_kept() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let api = api_between(api_start, D0 + 50 * DAY_MS, period);
        // Ici la colonne « count » est une autre grandeur : c'est « sum » qui concorde.
        let rows = rows_between(D0 + 2 * DAY_MS, api_start + 3 * DAY_MS, other, same);
        let s = stitch(&api, &rows, period, D0 + 3 * DAY_MS).unwrap();
        assert_eq!(s.column, SUM_COLUMN);
        assert!((s.points[0].ratio - ratio_at(s.points[0].ts)).abs() < 1e-12);
    }

    #[test]
    fn an_archive_that_measures_something_else_is_refused() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let api = api_between(api_start, D0 + 50 * DAY_MS, period);
        let rows = rows_between(D0 + 2 * DAY_MS, api_start + 3 * DAY_MS, other, other);
        let e = stitch(&api, &rows, period, D0 + 3 * DAY_MS).unwrap_err();
        assert!(
            e.contains("ne concorde pas") && e.contains("40,0 %") && e.contains("au plus 10 %"),
            "{e}"
        );
    }

    #[test]
    fn too_few_common_points_cannot_confirm_the_archive() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let api = api_between(api_start, D0 + 50 * DAY_MS, period);
        // L'archive s'arrête avant l'API : rien à comparer.
        let rows = rows_between(D0 + 2 * DAY_MS, api_start - HOUR, same, same);
        let e = stitch(&api, &rows, period, D0 + 3 * DAY_MS).unwrap_err();
        assert!(e.contains("Seulement 0"), "{e}");
    }

    #[test]
    fn an_archive_with_nothing_older_adds_nothing() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let api = api_between(api_start, D0 + 50 * DAY_MS, period);
        // Contrat coté depuis le début de l'API : l'archive ne commence qu'avec elle.
        let rows = rows_between(api_start, api_start + 3 * DAY_MS, other, other);
        let s = stitch(&api, &rows, period, D0 + 3 * DAY_MS).unwrap();
        assert_eq!(s.from_archive, 0);
        assert_eq!(s.points, api);
        assert!(stitch(&api, &[], period, D0).unwrap().points == api);
    }

    #[test]
    fn dates_before_the_first_archive_row_are_simply_absent() {
        let period = 4 * HOUR;
        let api_start = D0 + 20 * DAY_MS;
        let api = api_between(api_start, D0 + 50 * DAY_MS, period);
        // Contrat coté à partir du jour 10 : l'historique demandé commence plus tôt.
        let rows = rows_between(D0 + 10 * DAY_MS + HOUR, api_start + 3 * DAY_MS, same, same);
        let s = stitch(&api, &rows, period, D0 + 2 * DAY_MS).unwrap();
        assert_eq!(s.points[0].ts, D0 + 10 * DAY_MS + 4 * HOUR);
    }

    #[test]
    fn overlap_days_gather_about_a_dozen_points() {
        assert_eq!(overlap_days(5 * 60_000), 2);
        assert_eq!(overlap_days(HOUR), 2);
        assert_eq!(overlap_days(4 * HOUR), 2);
        assert_eq!(overlap_days(6 * HOUR), 3);
        assert_eq!(overlap_days(12 * HOUR), 6);
        assert_eq!(overlap_days(DAY_MS), 12);
        for (code, _) in PERIODS {
            let days = overlap_days(period_ms(code).unwrap());
            assert!((2..=14).contains(&days), "{code}");
        }
    }

    #[test]
    fn the_history_is_limited_to_a_drawable_number_of_points() {
        let ms = |code| period_ms(code).unwrap();
        // Assez de relevés : la profondeur demandée est servie.
        assert_eq!(history_days(180, ms("4h")), 180);
        assert_eq!(history_days(365, ms("1d")), 365);
        // Trop de relevés : 6 000 au plus, mais jamais moins que les 30 jours de l'API.
        assert_eq!(history_days(365, ms("1h")), 250);
        assert_eq!(history_days(365, ms("30m")), 125);
        assert_eq!(history_days(180, ms("5m")), 30);
        assert_eq!(history_days(30, ms("5m")), 30);
        // Bornes de la demande.
        assert_eq!(history_days(0, ms("4h")), 1);
        assert_eq!(history_days(10_000, ms("1d")), MAX_HISTORY_DAYS);
        for (code, _) in PERIODS {
            let days = history_days(MAX_HISTORY_DAYS, ms(code));
            let points = days * DAY_MS / ms(code);
            assert!(
                points <= MAX_POINTS.max(HISTORY_DAYS * DAY_MS / ms(code)),
                "{code}"
            );
        }
    }
}

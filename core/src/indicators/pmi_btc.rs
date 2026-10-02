//! Business Cycle vs Bitcoin : momentum du Bitcoin comparé au cycle économique
//! (ISM Manufacturing PMI), d'après l'indicateur TradingView « deKoder |
//! Business Cycle vs Bitcoin » (dK=PMIvsBTC).
//!
//! Sur des données mensuelles :
//! - momentum BTC = ((clôture / SMA(clôture, 24)) × 100 − 100) × 0,15 ;
//! - vague PMI = (PMI − 50) × 3.
//!
//! Fond vert-bleu : momentum BTC positif alors que le PMI est négatif (le BTC
//! anticipe une reprise). Fond rouge sombre : momentum BTC négatif alors que le
//! PMI est positif (le BTC alerte d'un retournement).
//!
//! Le PMI vient d'un fichier intégré (`core/data/ism_pmi.csv`, janvier 2013 →),
//! complété à chaque affichage par un CSV public mis à jour chaque mois
//! ([`crate::data::ISM_FEED_URL`]) et, sans recompiler, par la section `[PMI]`
//! de config.ini (`AAAA-MM = valeur`).

use super::{param_u64, IndicatorOutput, Level, Metric};
use crate::data::{today, DataProvider, ISM_FEED_URL};
use crate::figure::{merge, two_rows, Figure};
use crate::series::{fmt_date, rolling_mean, PricePoint};
use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const EMBEDDED: &str = include_str!("../../data/ism_pmi.csv");
pub const CONFIG_SECTION: &str = "PMI";
pub const DEFAULT_SMA: u64 = 24;
pub const BTC_SCALE: f64 = 0.15;
pub const PMI_SCALE: f64 = 3.0;
/// Plage plausible d'un PMI ISM (minimum historique ~29, maximum ~77).
const PMI_RANGE: std::ops::RangeInclusive<f64> = 25.0..=80.0;
/// Écart maximal (points de PMI) toléré entre le flux et le fichier intégré sur
/// les mois communs (révisions saisonnières comprises). Au-delà, le flux est ignoré.
const FEED_TOLERANCE: f64 = 1.5;

/// PMI par mois (date = 1er du mois de référence).
pub type PmiSeries = BTreeMap<NaiveDate, f64>;

fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("jour 1")
}

/// Mois au format `AAAA-MM` ou date `AAAA-MM-JJ` (ramenée au 1er du mois).
pub fn parse_month(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d"))
        .ok()
        .map(month_start)
}

fn plausible(v: f64) -> bool {
    PMI_RANGE.contains(&v)
}

/// Lit un CSV dont l'en-tête contient une colonne de date (`date` ou `month`)
/// et une colonne `PMI`. Renvoie les valeurs plausibles et le nombre de valeurs
/// rejetées (hors plage).
pub fn parse_csv(csv: &str) -> Result<(PmiSeries, usize), String> {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .ok_or("CSV vide")?
        .split(',')
        .map(|h| h.trim().to_lowercase())
        .collect();
    let date_col = header
        .iter()
        .position(|h| h == "date" || h == "month")
        .ok_or("colonne de date absente")?;
    let pmi_col = header
        .iter()
        .position(|h| h == "pmi")
        .ok_or("colonne PMI absente")?;
    let mut out = PmiSeries::new();
    let mut rejected = 0;
    for line in lines {
        let cells: Vec<&str> = line.split(',').collect();
        let (Some(month), Some(value)) = (
            cells.get(date_col).and_then(|d| parse_month(d)),
            cells
                .get(pmi_col)
                .and_then(|v| v.trim().parse::<f64>().ok()),
        ) else {
            continue;
        };
        if plausible(value) {
            out.insert(month, value);
        } else {
            rejected += 1;
        }
    }
    if out.is_empty() {
        return Err("aucune valeur de PMI lisible".into());
    }
    Ok((out, rejected))
}

/// PMI du fichier intégré.
pub fn embedded() -> PmiSeries {
    parse_csv(EMBEDDED).expect("core/data/ism_pmi.csv valide").0
}

/// Section `[PMI]` de config.ini : `AAAA-MM = valeur`. Renvoie les valeurs et
/// les messages pour les lignes ignorées.
pub fn parse_config(entries: &[(String, String)]) -> (PmiSeries, Vec<String>) {
    let mut out = PmiSeries::new();
    let mut errors = Vec::new();
    for (key, value) in entries {
        match (
            parse_month(key),
            value.trim().replace(',', ".").parse::<f64>(),
        ) {
            (Some(m), Ok(v)) if plausible(v) => {
                out.insert(m, v);
            }
            _ => errors.push(format!(
                "« {key} = {value} » ignoré : ligne attendue « AAAA-MM = PMI » (ex. 2026-09 = 49.1), PMI entre 25 et 80."
            )),
        }
    }
    (out, errors)
}

/// Dernière clôture de chaque mois (date = 1er du mois). Le mois en cours est
/// inclus avec la dernière clôture connue, comme une bougie mensuelle.
pub fn monthly_closes(prices: &[PricePoint]) -> Vec<(NaiveDate, f64)> {
    let mut by_month: BTreeMap<NaiveDate, (NaiveDate, f64)> = BTreeMap::new();
    for p in prices {
        let entry = by_month
            .entry(month_start(p.date))
            .or_insert((p.date, p.close));
        if p.date >= entry.0 {
            *entry = (p.date, p.close);
        }
    }
    by_month.into_iter().map(|(m, (_, c))| (m, c)).collect()
}

/// Momentum BTC : ((clôture / SMA) × 100 − 100) × échelle.
pub fn btc_momentum(closes: &[f64], sma: usize, scale: f64) -> Vec<Option<f64>> {
    rolling_mean(closes, sma)
        .into_iter()
        .zip(closes)
        .map(|(m, c)| m.map(|m| ((c / m) * 100.0 - 100.0) * scale))
        .collect()
}

/// Vague PMI : (PMI − 50) × échelle.
pub fn pmi_wave(pmi: f64) -> f64 {
    (pmi - 50.0) * PMI_SCALE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Momentum BTC positif, PMI négatif : le BTC anticipe une reprise.
    BtcLeadsRecovery,
    /// Momentum BTC négatif, PMI positif : le BTC alerte d'un retournement.
    BtcWarnsRollover,
    BothPositive,
    BothNegative,
}

impl Signal {
    pub fn of(btc: f64, pmi: f64) -> Self {
        match (btc >= 0.0, pmi >= 0.0) {
            (true, false) => Self::BtcLeadsRecovery,
            (false, true) => Self::BtcWarnsRollover,
            (true, true) => Self::BothPositive,
            (false, false) => Self::BothNegative,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BtcLeadsRecovery => "divergence : le BTC anticipe une reprise",
            Self::BtcWarnsRollover => "divergence : le BTC alerte d'un retournement",
            Self::BothPositive => "alignés en hausse",
            Self::BothNegative => "alignés en baisse",
        }
    }

    fn fill(self) -> Option<&'static str> {
        match self {
            Self::BtcLeadsRecovery => Some("rgba(38, 166, 154, 0.18)"),
            Self::BtcWarnsRollover => Some("rgba(160, 30, 50, 0.22)"),
            _ => None,
        }
    }
}

/// Une ligne par mois où les deux séries sont définies.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub month: NaiveDate,
    pub btc_close: f64,
    pub btc: Option<f64>,
    pub pmi: Option<f64>,
}

/// Aligne le momentum BTC et le PMI par mois (à partir du premier momentum défini).
pub fn rows(closes: &[(NaiveDate, f64)], pmi: &PmiSeries, sma: usize) -> Vec<Row> {
    let values: Vec<f64> = closes.iter().map(|c| c.1).collect();
    let momentum = btc_momentum(&values, sma, BTC_SCALE);
    closes
        .iter()
        .zip(momentum)
        .skip_while(|(_, m)| m.is_none())
        .map(|((month, close), m)| Row {
            month: *month,
            btc_close: *close,
            btc: m,
            pmi: pmi.get(month).map(|v| pmi_wave(*v)),
        })
        .collect()
}

/// Découpe une série en parties positive et négative (pour les colorer), en
/// insérant le point de passage à zéro entre deux mois de signes opposés.
fn split_by_sign(points: &[(NaiveDate, f64)]) -> (Vec<String>, Vec<Value>, Vec<Value>) {
    let (mut x, mut pos, mut neg) = (Vec::new(), Vec::new(), Vec::new());
    let mut push = |d: NaiveDate, v: f64, x: &mut Vec<String>| {
        x.push(fmt_date(d));
        pos.push(if v >= 0.0 { json!(v) } else { Value::Null });
        neg.push(if v <= 0.0 { json!(v) } else { Value::Null });
    };
    for (i, &(d, v)) in points.iter().enumerate() {
        if i > 0 {
            let (d0, v0) = points[i - 1];
            if (v0 > 0.0 && v < 0.0) || (v0 < 0.0 && v > 0.0) {
                let frac = v0 / (v0 - v);
                let days = ((d - d0).num_days() as f64 * frac).round() as i64;
                push(d0 + Duration::days(days), 0.0, &mut x);
            }
        }
        push(d, v, &mut x);
    }
    (x, pos, neg)
}

/// Périodes consécutives de divergence (début inclus, fin exclue).
fn divergence_zones(rows: &[Row]) -> Vec<(NaiveDate, NaiveDate, Signal)> {
    let mut zones: Vec<(NaiveDate, NaiveDate, Signal)> = Vec::new();
    for r in rows {
        let (Some(b), Some(p)) = (r.btc, r.pmi) else {
            continue;
        };
        let signal = Signal::of(b, p);
        if signal.fill().is_none() {
            continue;
        }
        let next = (r.month + Duration::days(32)).with_day(1).expect("jour 1");
        match zones.last_mut() {
            Some(z) if z.2 == signal && z.1 == r.month => z.1 = next,
            _ => zones.push((r.month, next, signal)),
        }
    }
    zones
}

fn line_pair(
    fig: &mut Figure,
    name: &str,
    points: &[(NaiveDate, f64)],
    colors: (&str, &str),
    hover: &str,
) {
    let (x, pos, neg) = split_by_sign(points);
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": name, "legendgroup": name, "x": x, "y": pos,
        "xaxis": "x2", "yaxis": "y2", "line": {"color": colors.0, "width": 2}, "hoverinfo": "skip", "connectgaps": false}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": name, "legendgroup": name, "showlegend": false, "x": x, "y": neg,
        "xaxis": "x2", "yaxis": "y2", "line": {"color": colors.1, "width": 2}, "hoverinfo": "skip", "connectgaps": false}));
    // Trace invisible pour l'info-bulle (une valeur par mois).
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": name, "legendgroup": name, "showlegend": false,
        "x": points.iter().map(|p| fmt_date(p.0)).collect::<Vec<_>>(), "y": points.iter().map(|p| p.1).collect::<Vec<_>>(),
        "xaxis": "x2", "yaxis": "y2", "line": {"width": 0, "color": colors.0}, "hovertemplate": hover}));
}

pub(crate) fn build_figure(rows: &[Row], sma: usize) -> Figure {
    let btc_points: Vec<(NaiveDate, f64)> = rows
        .iter()
        .filter_map(|r| Some((r.month, r.btc?)))
        .collect();
    let pmi_points: Vec<(NaiveDate, f64)> = rows
        .iter()
        .filter_map(|r| Some((r.month, r.pmi?)))
        .collect();

    let mut layout = two_rows(
        [0.45, 0.55],
        0.08,
        Some([
            "Prix du Bitcoin (clôture mensuelle)",
            "Momentum BTC vs cycle économique (ISM PMI)",
        ]),
    );
    merge(
        &mut layout,
        json!({
            "title": {"text": format!("Business Cycle vs Bitcoin — momentum BTC (SMA {sma} mois) et ISM Manufacturing PMI")},
            "yaxis": {"title": {"text": "BTC (USD)"}, "type": "log"},
            "yaxis2": {"title": {"text": "Écart à la tendance / PMI centré"}, "zeroline": false},
            "xaxis2": {"title": {"text": "Date"}},
            "height": 850, "hovermode": "x unified",
            "legend": {"orientation": "h", "yanchor": "top", "y": -0.08, "xanchor": "center", "x": 0.5},
            "margin": {"t": 110, "b": 90}
        }),
    );
    let mut fig = Figure::new(layout);
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC",
        "x": rows.iter().map(|r| fmt_date(r.month)).collect::<Vec<_>>(), "y": rows.iter().map(|r| r.btc_close).collect::<Vec<_>>(),
        "line": {"color": "#e8ecf3", "width": 1.5}, "hovertemplate": "BTC : %{y:,.0f} $<extra></extra>"}));
    line_pair(
        &mut fig,
        "Momentum BTC",
        &btc_points,
        ("#f5c518", "#ff8c00"),
        "Momentum BTC : %{y:+.2f}<extra></extra>",
    );
    line_pair(
        &mut fig,
        "PMI ISM (centré)",
        &pmi_points,
        ("#26a69a", "#ef5350"),
        "PMI centré : %{y:+.2f}<extra></extra>",
    );

    // Fonds de divergence, sur les deux graphiques.
    for (x0, x1, signal) in divergence_zones(rows) {
        for yref in ["y domain", "y2 domain"] {
            let xref = if yref == "y domain" { "x" } else { "x2" };
            fig.shape(json!({"type": "rect", "xref": xref, "yref": yref, "x0": fmt_date(x0), "x1": fmt_date(x1),
                "y0": 0, "y1": 1, "fillcolor": signal.fill(), "line": {"width": 0}, "layer": "below"}));
        }
    }
    fig.hline(0.0, "rgba(200, 200, 200, 0.6)", "dash", "y2", "x2 domain");
    fig
}

fn fmt_month(m: NaiveDate) -> String {
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    format!("{} {}", MONTHS[m.month0() as usize], m.year())
}

fn fmt_fr(v: f64, decimals: usize) -> String {
    format!("{v:+.decimals$}").replace('.', ",")
}

/// PMI retenu (fichier intégré, puis flux en ligne, puis config.ini) et messages.
pub async fn load_pmi(data: &DataProvider) -> (PmiSeries, Vec<(Level, String)>) {
    let mut pmi = embedded();
    let mut notices = Vec::new();
    let last_embedded = pmi.keys().next_back().copied();

    match data.ism_feed().await.and_then(|csv| {
        parse_csv(&csv).map_err(|e| format!("CSV illisible ({ISM_FEED_URL}) : {e}"))
    }) {
        Ok((feed, rejected)) => {
            let (compared, worst) = feed
                .iter()
                .filter_map(|(m, v)| Some((v - pmi.get(m)?).abs()))
                .fold((0, 0.0f64), |(n, w), d| (n + 1, w.max(d)));
            if worst > FEED_TOLERANCE {
                notices.push((Level::Error, format!(
                    "PMI ISM en ligne : incohérent avec le fichier intégré (écart jusqu'à {worst:.1} point(s) sur {compared} mois communs). \
                     Flux ignoré, fichier intégré utilisé."
                )));
            } else {
                let new = feed
                    .keys()
                    .filter(|m| last_embedded.is_none_or(|l| **m > l))
                    .count();
                let (last_m, last_v) = feed
                    .iter()
                    .next_back()
                    .map(|(m, v)| (*m, *v))
                    .expect("non vide");
                pmi.extend(feed);
                let rejected = if rejected > 0 {
                    format!(" ; {rejected} valeur(s) hors plage 25-80 ignorée(s)")
                } else {
                    String::new()
                };
                let level = if rejected.is_empty() {
                    Level::Success
                } else {
                    Level::Error
                };
                notices.push((level, format!(
                    "PMI ISM en ligne : OK. Dernier PMI {} ({}), {new} nouveau(x) mois ; vérifié sur {compared} mois communs avec le fichier intégré (écart max {worst:.1}){rejected}.",
                    format!("{last_v:.1}").replace('.', ","), fmt_month(last_m)
                )));
            }
        }
        Err(e) => notices.push((
            Level::Error,
            format!(
                "PMI ISM en ligne : échec de la récupération, {}. Fichier intégré utilisé.",
                e.trim_end_matches('.')
            ),
        )),
    }

    let (cfg, errors) = parse_config(&data.config().section(CONFIG_SECTION));
    pmi.extend(cfg);
    for e in errors {
        notices.push((
            Level::Warning,
            format!("config.ini [{CONFIG_SECTION}] : {e}"),
        ));
    }
    (pmi, notices)
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let sma = param_u64(params, "sma", DEFAULT_SMA, 6, 60) as usize;
    let prices = data.combined_btc_history().await?;
    let closes = monthly_closes(&prices);
    let (pmi, notices) = load_pmi(data).await;
    let rows = rows(&closes, &pmi, sma);
    if rows.is_empty() {
        return Err(format!(
            "Historique BTC trop court pour une moyenne sur {sma} mois."
        ));
    }

    let mut out = IndicatorOutput::from(build_figure(&rows, sma));
    for (level, text) in notices {
        out = out.with_notice(level, text);
    }

    let last_btc = rows.iter().rev().find_map(|r| Some((r.month, r.btc?)));
    let last_pmi = rows.iter().rev().find_map(|r| Some((r.month, r.pmi?)));
    if let Some((m, _)) = last_pmi {
        // Le PMI d'un mois paraît le 1er jour ouvré du mois suivant.
        let expected = month_start(today() - Duration::days(35));
        if m < expected {
            out = out.with_notice(Level::Warning, format!(
                "Dernier PMI connu : {}. Pour ajouter les mois suivants, saisissez-les dans config.ini, section [{CONFIG_SECTION}] (ex. « {} = 49.1 »).",
                fmt_month(m), (m + Duration::days(32)).format("%Y-%m")
            ));
        }
    }
    if let (Some((_, b)), Some((pm, p))) = (last_btc, last_pmi) {
        out.metrics = vec![
            Metric {
                label: "Momentum BTC".into(),
                value: fmt_fr(b, 2),
            },
            Metric {
                label: format!("PMI ISM ({})", fmt_month(pm)),
                value: format!("{:.1}", p / PMI_SCALE + 50.0).replace('.', ","),
            },
            Metric {
                label: "Vague PMI".into(),
                value: fmt_fr(p, 2),
            },
            Metric {
                label: "Signal".into(),
                value: Signal::of(b, p).label().into(),
            },
        ];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

    #[test]
    fn embedded_file_is_valid() {
        let pmi = embedded();
        assert!(pmi.len() >= 160);
        assert_eq!(pmi.get(&d("2013-01-01")), Some(&53.1));
        assert_eq!(pmi.get(&d("2020-04-01")), Some(&41.5));
        assert!(pmi.values().all(|v| plausible(*v)));
        // Mois consécutifs, sans trou.
        let months: Vec<_> = pmi.keys().collect();
        assert!(months
            .windows(2)
            .all(|w| (*w[0] + Duration::days(32)).with_day(1).unwrap() == *w[1]));
    }

    #[test]
    fn feed_csv_is_parsed_and_implausible_values_rejected() {
        let csv = "date,PMI,New Orders\n2026-08-31,54.6,53.7\n2026-09-30,11.1,48.0\nbad,50,1\n";
        let (pmi, rejected) = parse_csv(csv).unwrap();
        assert_eq!(
            pmi.into_iter().collect::<Vec<_>>(),
            vec![(d("2026-08-01"), 54.6)]
        );
        assert_eq!(rejected, 1);
        assert!(parse_csv("date,value\n2026-08-31,54.6").is_err());
        assert!(parse_csv("").is_err());
    }

    #[test]
    fn config_lines() {
        let (pmi, errors) = parse_config(&[
            ("2026-09".into(), "49,1".into()),
            ("2026-10-01".into(), "50.2".into()),
            ("2026-11".into(), "500".into()),
            ("septembre".into(), "49".into()),
        ]);
        assert_eq!(pmi.get(&d("2026-09-01")), Some(&49.1));
        assert_eq!(pmi.get(&d("2026-10-01")), Some(&50.2));
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn monthly_closes_keep_last_day_of_month() {
        let p = |date: &str, close: f64| PricePoint {
            date: d(date),
            close,
        };
        let m = monthly_closes(&[
            p("2024-01-05", 1.0),
            p("2024-01-31", 2.0),
            p("2024-02-01", 3.0),
            p("2024-02-10", 4.0),
        ]);
        assert_eq!(m, vec![(d("2024-01-01"), 2.0), (d("2024-02-01"), 4.0)]);
    }

    #[test]
    fn formulas_match_the_tradingview_script() {
        // PMI 54,6 → (54,6 − 50) × 3 = 13,80 (valeur affichée par TradingView pour août 2026).
        assert!((pmi_wave(54.6) - 13.8).abs() < 1e-9);
        // Clôture 20 % au-dessus de la SMA → 20 × 0,15 = 3.
        let closes = [100.0, 100.0, 100.0, 130.0];
        let m = btc_momentum(&closes, 4, BTC_SCALE);
        assert_eq!(m[..3], [None, None, None]);
        let sma = (100.0 * 3.0 + 130.0) / 4.0;
        assert!((m[3].unwrap() - ((130.0 / sma) * 100.0 - 100.0) * 0.15).abs() < 1e-9);
    }

    #[test]
    fn signals_and_zones() {
        assert_eq!(Signal::of(1.0, -1.0), Signal::BtcLeadsRecovery);
        assert_eq!(Signal::of(-1.0, 1.0), Signal::BtcWarnsRollover);
        let row = |m: &str, btc: f64, pmi: f64| Row {
            month: d(m),
            btc_close: 1.0,
            btc: Some(btc),
            pmi: Some(pmi),
        };
        let zones = divergence_zones(&[
            row("2024-01-01", 1.0, -1.0),
            row("2024-02-01", 2.0, -2.0),
            row("2024-03-01", 1.0, 1.0),
            row("2024-04-01", -1.0, 1.0),
        ]);
        assert_eq!(
            zones,
            vec![
                (d("2024-01-01"), d("2024-03-01"), Signal::BtcLeadsRecovery),
                (d("2024-04-01"), d("2024-05-01"), Signal::BtcWarnsRollover),
            ]
        );
    }

    #[test]
    fn sign_split_inserts_zero_crossing() {
        let (x, pos, neg) = split_by_sign(&[(d("2024-01-01"), 2.0), (d("2024-02-01"), -2.0)]);
        assert_eq!(x, vec!["2024-01-01", "2024-01-17", "2024-02-01"]);
        assert_eq!(pos, vec![json!(2.0), json!(0.0), Value::Null]);
        assert_eq!(neg, vec![Value::Null, json!(0.0), json!(-2.0)]);
    }

    fn provider(name: &str) -> DataProvider {
        let dir = std::env::temp_dir().join(format!(
            "dashboard-crypto-pmi-{}-{name}",
            std::process::id()
        ));
        DataProvider::new(crate::config::Config::new(dir.join("config.ini")))
            .with_cache_dir(dir.join("cache"))
    }

    #[tokio::test]
    async fn online_feed_extends_embedded_history() {
        let data = provider("ok");
        data.seed_ism_feed("date,PMI\n2026-07-31,55.6\n2026-08-31,54.6\n2026-09-30,49.1\n");
        let (pmi, notices) = load_pmi(&data).await;
        assert_eq!(pmi.get(&d("2026-09-01")), Some(&49.1));
        assert!(matches!(notices[0].0, Level::Success), "{notices:?}");
        assert!(notices[0].1.contains("1 nouveau(x) mois") && notices[0].1.contains("sept. 2026"));
    }

    #[tokio::test]
    async fn inconsistent_or_unreadable_feed_is_reported_in_red() {
        let data = provider("bad");
        data.seed_ism_feed("date,PMI\n2026-07-31,40.0\n2026-09-30,49.1\n");
        let (pmi, notices) = load_pmi(&data).await;
        assert_eq!(pmi.get(&d("2026-09-01")), None);
        assert!(matches!(notices[0].0, Level::Error) && notices[0].1.contains("incohérent"));

        let data = provider("unreadable");
        data.seed_ism_feed("<html>404</html>");
        let (pmi, notices) = load_pmi(&data).await;
        assert_eq!(pmi, embedded());
        assert!(matches!(notices[0].0, Level::Error) && notices[0].1.contains("CSV illisible"));
    }
}

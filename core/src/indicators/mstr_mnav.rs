//! MSTR mNAV : cours de Strategy Inc. (MSTR) comparé à la valeur des bitcoins
//! qu'elle détient.
//!
//! mNAV = capitalisation boursière / (BTC détenus × prix du BTC)
//!      = (cours MSTR × actions en circulation) / (BTC détenus × prix du BTC)
//!
//! Les BTC détenus et le nombre d'actions viennent d'un fichier embarqué
//! (`core/data/mstr_holdings.json`, d'après les publications SEC de Strategy),
//! complété à chaque affichage par les dernières publications SEC EDGAR
//! (8-K pour les BTC, 10-Q/10-K pour les actions, voir [`crate::sec`]) et,
//! sans recompiler, par la section `[MSTR]` de config.ini :
//!
//! ```ini
//! [MSTR]
//! ; date = BTC détenus[, actions en circulation]
//! 2026-10-05 = 850000, 390000000
//! ; seuils de zones (mNAV)
//! seuils = 1.0, 1.5, 2.5
//! ```

use super::{param_bool, IndicatorOutput, Level, Metric};
use crate::config::Config;
use crate::data::DataProvider;
use crate::figure::{dates, merge, two_rows, Figure};
use crate::sec;
use crate::series::{fmt_date, Candle};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

const EMBEDDED: &str = include_str!("../../data/mstr_holdings.json");
pub const CONFIG_SECTION: &str = "MSTR";
const THRESHOLDS_KEY: &str = "seuils";
/// Au-delà, la dernière donnée de holdings est signalée comme ancienne.
const STALE_DAYS: i64 = 45;

/// Point de données tel que publié : un champ absent (`None`) n'a pas été
/// publié à cette date et reprend la dernière valeur connue.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HoldingsPoint {
    pub date: NaiveDate,
    pub btc_holdings: Option<f64>,
    pub shares_outstanding: Option<f64>,
}

/// Holdings complets à une date (après forward-fill).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Holdings {
    pub date: NaiveDate,
    pub btc: f64,
    pub shares: f64,
}

#[derive(Deserialize)]
struct EmbeddedFile {
    points: Vec<HoldingsPoint>,
}

/// Points du fichier embarqué.
pub fn embedded_points() -> Vec<HoldingsPoint> {
    serde_json::from_str::<EmbeddedFile>(EMBEDDED)
        .expect("core/data/mstr_holdings.json valide")
        .points
}

/// Lit un nombre entier ou décimal, en tolérant les séparateurs de milliers
/// usuels (espaces, espaces insécables, `_`).
fn parse_amount(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    cleaned
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Seuils de zones (croissants, positifs) : `seuils = 1.0, 1.5, 2.5`.
pub fn parse_thresholds(value: &str) -> Option<[f64; 3]> {
    let values: Vec<f64> = value
        .split(',')
        .map(|v| v.trim().parse::<f64>().ok())
        .collect::<Option<_>>()?;
    let t: [f64; 3] = values.try_into().ok()?;
    (t[0] > 0.0 && t[0] < t[1] && t[1] < t[2]).then_some(t)
}

/// Contenu de la section `[MSTR]` : points de holdings (`AAAA-MM-JJ = btc[, actions]`),
/// seuils éventuels, et messages pour les entrées ignorées.
#[derive(Debug, Default, PartialEq)]
pub struct MstrConfig {
    pub points: Vec<HoldingsPoint>,
    pub thresholds: Option<[f64; 3]>,
    pub errors: Vec<String>,
}

pub fn parse_config(entries: &[(String, String)]) -> MstrConfig {
    let mut out = MstrConfig::default();
    for (key, value) in entries {
        if key.eq_ignore_ascii_case(THRESHOLDS_KEY) {
            match parse_thresholds(value) {
                Some(t) => out.thresholds = Some(t),
                None => out.errors.push(format!(
                    "« {key} = {value} » ignoré : trois seuils croissants attendus (ex. 1.0, 1.5, 2.5)."
                )),
            }
            continue;
        }
        let Ok(date) = NaiveDate::parse_from_str(key, "%Y-%m-%d") else {
            out.errors.push(format!(
                "« {key} » ignoré : clé attendue au format AAAA-MM-JJ ou « seuils »."
            ));
            continue;
        };
        let fields: Vec<&str> = value.split(',').collect();
        let btc = fields.first().and_then(|v| parse_amount(v));
        let shares = fields.get(1).map(|v| parse_amount(v));
        match (btc, shares, fields.len()) {
            (Some(btc), None, 1) => out.points.push(HoldingsPoint { date, btc_holdings: Some(btc), shares_outstanding: None }),
            (Some(btc), Some(Some(shares)), 2) => {
                out.points.push(HoldingsPoint { date, btc_holdings: Some(btc), shares_outstanding: Some(shares) })
            }
            _ => out.errors.push(format!(
                "« {key} = {value} » ignoré : valeur attendue « BTC détenus » ou « BTC détenus, actions en circulation »."
            )),
        }
    }
    out
}

/// Fusionne les points de la configuration dans ceux du fichier : à date
/// égale, les valeurs fournies remplacent celles du fichier (champ par champ).
pub fn merge_points(base: Vec<HoldingsPoint>, overrides: &[HoldingsPoint]) -> Vec<HoldingsPoint> {
    let mut by_date: BTreeMap<NaiveDate, HoldingsPoint> =
        base.into_iter().map(|p| (p.date, p)).collect();
    for o in overrides {
        let entry = by_date.entry(o.date).or_insert(HoldingsPoint {
            date: o.date,
            btc_holdings: None,
            shares_outstanding: None,
        });
        entry.btc_holdings = o.btc_holdings.or(entry.btc_holdings);
        entry.shares_outstanding = o.shares_outstanding.or(entry.shares_outstanding);
    }
    by_date.into_values().collect()
}

/// Forward-fill champ par champ : chaque date reprend la dernière valeur
/// connue de chaque champ. Les dates où l'un des deux champs n'est pas encore
/// connu sont écartées.
pub fn forward_fill(points: &[HoldingsPoint]) -> Vec<Holdings> {
    let mut sorted = points.to_vec();
    sorted.sort_by_key(|p| p.date);
    let (mut btc, mut shares) = (None, None);
    sorted
        .iter()
        .filter_map(|p| {
            btc = p.btc_holdings.or(btc);
            shares = p.shares_outstanding.or(shares);
            Some(Holdings {
                date: p.date,
                btc: btc?,
                shares: shares?,
            })
        })
        .collect()
}

/// Holdings en vigueur à `date` : dernier point daté au plus tard ce jour-là
/// (pas d'interpolation). `None` avant le premier point.
pub fn holdings_at(holdings: &[Holdings], date: NaiveDate) -> Option<Holdings> {
    let idx = holdings.partition_point(|h| h.date <= date);
    idx.checked_sub(1).map(|i| holdings[i])
}

/// Aligne les deux séries sur les jours de cotation de MSTR : le BTC cotant
/// tous les jours, on prend sa clôture du même jour (ou la dernière connue).
pub fn align_prices(mstr: &[Candle], btc: &[Candle]) -> Vec<(NaiveDate, f64, f64)> {
    let btc_by_day: BTreeMap<NaiveDate, f64> = btc.iter().map(|c| (c.date, c.close)).collect();
    mstr.iter()
        .filter_map(|m| {
            let btc_close = btc_by_day.range(..=m.date).next_back().map(|(_, v)| *v)?;
            Some((m.date, m.close, btc_close))
        })
        .collect()
}

pub fn mnav(mstr_price: f64, shares: f64, btc_holdings: f64, btc_price: f64) -> f64 {
    (mstr_price * shares) / (btc_holdings * btc_price)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// L'action vaut moins que ses bitcoins.
    Undervalued,
    Neutral,
    Overvalued,
    StronglyOvervalued,
}

impl Zone {
    pub fn label(self) -> &'static str {
        match self {
            Zone::Undervalued => "sous-coté",
            Zone::Neutral => "neutre",
            Zone::Overvalued => "sur-coté",
            Zone::StronglyOvervalued => "fortement sur-coté",
        }
    }
}

pub const DEFAULT_THRESHOLDS: [f64; 3] = [1.0, 1.5, 2.5];

/// Zone d'un mNAV : < t0 sous-coté, t0–t1 neutre, t1–t2 sur-coté, ≥ t2 fortement sur-coté.
pub fn classify(mnav: f64, t: [f64; 3]) -> Zone {
    if mnav < t[0] {
        Zone::Undervalued
    } else if mnav < t[1] {
        Zone::Neutral
    } else if mnav < t[2] {
        Zone::Overvalued
    } else {
        Zone::StronglyOvervalued
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MnavPoint {
    pub date: NaiveDate,
    pub mstr: f64,
    pub btc_price: f64,
    pub holdings: Holdings,
    pub mnav: f64,
}

/// mNAV de chaque jour de cotation de MSTR couvert par les holdings.
pub fn compute(mstr: &[Candle], btc: &[Candle], holdings: &[Holdings]) -> Vec<MnavPoint> {
    align_prices(mstr, btc)
        .into_iter()
        .filter_map(|(date, mstr, btc_price)| {
            let h = holdings_at(holdings, date)?;
            let value = mnav(mstr, h.shares, h.btc, btc_price);
            value.is_finite().then_some(MnavPoint {
                date,
                mstr,
                btc_price,
                holdings: h,
                mnav: value,
            })
        })
        .collect()
}

/// Échelle de couleur continue, du vert (mNAV bas) au rouge (mNAV élevé),
/// dont les paliers tombent sur les seuils. Renvoie (cmax, échelle).
pub(crate) fn colorscale(t: [f64; 3]) -> (f64, Vec<(f64, &'static str)>) {
    let cmax = t[2] + (t[2] - t[1]);
    let stop = |v: f64| (v / cmax).clamp(0.0, 1.0);
    (
        cmax,
        vec![
            (0.0, "#006837"),
            (stop(t[0]), "#1a9850"),
            (stop((t[0] + t[1]) / 2.0), "#a6d96a"),
            (stop(t[1]), "#fee08b"),
            (stop((t[1] + t[2]) / 2.0), "#fdae61"),
            (stop(t[2]), "#d73027"),
            (1.0, "#a50026"),
        ],
    )
}

/// Couleur de fond (transparente) de chaque zone.
fn zone_fill(zone: Zone) -> &'static str {
    match zone {
        Zone::Undervalued => "rgba(26, 152, 80, 0.18)",
        Zone::Neutral => "rgba(166, 217, 106, 0.12)",
        Zone::Overvalued => "rgba(253, 174, 97, 0.14)",
        Zone::StronglyOvervalued => "rgba(215, 48, 39, 0.16)",
    }
}

fn group_thousands(v: f64) -> String {
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn build_figure(points: &[MnavPoint], t: [f64; 3], show_btc: bool) -> Figure {
    let last = points.last().expect("au moins un point");
    let current_zone = classify(last.mnav, t);
    let (cmax, scale) = colorscale(t);
    let x = dates(points.iter().map(|p| p.date));
    let mnav_values: Vec<f64> = points.iter().map(|p| p.mnav).collect();
    let mnav_top = mnav_values.iter().cloned().fold(t[2], f64::max) * 1.08;

    let mut layout = two_rows(
        [0.65, 0.35],
        0.08,
        Some(["Cours MSTR (USD), coloré selon le mNAV", "mNAV"]),
    );
    merge(
        &mut layout,
        json!({
            "title": {"text": format!(
                "Strategy (MSTR) : mNAV actuel {:.2} ({}) — holdings au {}",
                last.mnav, current_zone.label(), last.holdings.date.format("%d/%m/%Y")
            )},
            "xaxis2": {"title": {"text": "Date"}},
            "yaxis": {"title": {"text": "Cours MSTR (USD)"}, "type": "log"},
            "yaxis2": {"title": {"text": "mNAV"}, "range": [0, mnav_top], "rangemode": "tozero"},
            "height": 900, "hovermode": "closest",
            "legend": {"orientation": "h", "yanchor": "top", "y": -0.07, "xanchor": "center", "x": 0.5},
            "margin": {"t": 110, "r": 90, "b": 90}
        }),
    );
    if show_btc {
        layout["yaxis3"] = json!({"title": {"text": "Prix BTC (USD)", "font": {"color": "#8a8f98"}}, "overlaying": "y",
            "side": "right", "type": "log", "showgrid": false, "tickfont": {"color": "#8a8f98"}, "anchor": "x"});
    }
    let mut fig = Figure::new(layout);

    // Continuité du cours (gris fin), puis points colorés selon le mNAV.
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Cours MSTR", "x": x, "showlegend": false,
        "y": points.iter().map(|p| p.mstr).collect::<Vec<_>>(), "line": {"color": "rgba(180, 180, 180, 0.45)", "width": 1},
        "hoverinfo": "skip"}));
    let hover: Vec<String> = points
        .iter()
        .map(|p| {
            format!(
                "<b>{}</b><br>MSTR : {:.2} $<br>BTC : {} $<br>BTC détenus : {}<br>Actions : {}<br><b>mNAV : {:.2}</b> ({})<br><i>holdings au {}</i>",
                p.date.format("%d/%m/%Y"),
                p.mstr,
                group_thousands(p.btc_price),
                group_thousands(p.holdings.btc),
                group_thousands(p.holdings.shares),
                p.mnav,
                classify(p.mnav, t).label(),
                p.holdings.date.format("%d/%m/%Y")
            )
        })
        .collect();
    let ticks: Vec<f64> = (1..=((cmax * 2.0).floor() as usize))
        .map(|i| i as f64 / 2.0)
        .collect();
    fig.trace(json!({"type": "scatter", "mode": "markers", "name": "Cours MSTR (couleur = mNAV)", "x": x,
        "y": points.iter().map(|p| p.mstr).collect::<Vec<_>>(), "text": hover, "hovertemplate": "%{text}<extra></extra>",
        "marker": {"size": 4, "color": mnav_values, "cmin": 0, "cmax": cmax, "colorscale": scale,
            "colorbar": {"title": {"text": "mNAV"}, "tickvals": ticks, "len": 0.6, "y": 0.99, "yanchor": "top", "x": 1.07, "thickness": 14}}}));
    if show_btc {
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x, "yaxis": "y3",
            "y": points.iter().map(|p| p.btc_price).collect::<Vec<_>>(), "line": {"color": "rgba(138, 143, 152, 0.6)", "width": 1, "dash": "dot"},
            "hoverinfo": "skip"}));
    }

    // mNAV : bandes de zones, parité, courbe.
    let bands = [
        (0.0, t[0], Zone::Undervalued),
        (t[0], t[1], Zone::Neutral),
        (t[1], t[2], Zone::Overvalued),
        (t[2], mnav_top, Zone::StronglyOvervalued),
    ];
    for (y0, y1, zone) in bands {
        fig.shape(json!({"type": "rect", "xref": "x2 domain", "yref": "y2", "x0": 0, "x1": 1, "y0": y0, "y1": y1,
            "fillcolor": zone_fill(zone), "line": {"width": 0}, "layer": "below"}));
        fig.annotation(json!({"xref": "x2 domain", "yref": "y2", "x": 0.005, "y": (y0 + y1) / 2.0, "xanchor": "left",
            "text": zone.label(), "showarrow": false, "font": {"size": 10, "color": "rgba(230, 230, 230, 0.6)"}}));
    }
    fig.hline(1.0, "rgba(255, 255, 255, 0.8)", "dash", "y2", "x2 domain");
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "mNAV", "x": x, "y": mnav_values, "xaxis": "x2", "yaxis": "y2",
        "line": {"color": "#e8ecf3", "width": 1.5}, "hovertemplate": "%{x|%d/%m/%Y} : mNAV %{y:.2f}<extra></extra>"}));
    fig
}

/// Holdings retenus (fichier + configuration) et messages de configuration.
pub fn load_holdings(config: &Config) -> (Vec<Holdings>, [f64; 3], Vec<String>) {
    load_holdings_with(config, &[])
}

/// Holdings retenus : points SEC, remplacés à date égale par ceux du fichier,
/// eux-mêmes remplacés par ceux de la configuration.
fn load_holdings_with(
    config: &Config,
    sec_points: &[HoldingsPoint],
) -> (Vec<Holdings>, [f64; 3], Vec<String>) {
    let cfg = parse_config(&config.section(CONFIG_SECTION));
    let base = merge_points(sec_points.to_vec(), &embedded_points());
    let holdings = forward_fill(&merge_points(base, &cfg.points));
    (
        holdings,
        cfg.thresholds.unwrap_or(DEFAULT_THRESHOLDS),
        cfg.errors,
    )
}

/// Écart relatif maximal toléré entre le nombre d'actions SEC et celui du
/// fichier intégré à une même date. Au-delà, les actions SEC sont ignorées
/// (classe manquante, unité différente...).
const SHARES_TOLERANCE: f64 = 0.02;
/// Nombre maximal de 8-K lus à chaque mise à jour.
const MAX_8K: usize = 15;

/// Résultat de la mise à jour SEC : points à ajouter et messages de
/// diagnostic (vert si tout va bien, rouge sinon).
#[derive(Debug, Default)]
pub struct SecUpdate {
    pub points: Vec<HoldingsPoint>,
    pub notices: Vec<(Level, String)>,
}

fn fmt_day(d: NaiveDate) -> String {
    d.format("%d/%m/%Y").to_string()
}

/// Nombre d'actions : faits XBRL, vérifiés contre le fichier intégré aux dates
/// communes. Seules les dates postérieures au fichier sont ajoutées.
async fn sec_shares(data: &DataProvider, known: &[HoldingsPoint], out: &mut SecUpdate) {
    let url = sec::shares_concept_url();
    let facts = match data.sec_text(&url).await.and_then(|body| {
        let v = serde_json::from_str(&body).map_err(|e| format!("JSON illisible ({url}) : {e}"))?;
        sec::parse_shares_concept(&v)
    }) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            out.notices.push((
                Level::Error,
                "SEC, nombre d'actions : aucune valeur publiée trouvée. Fichier intégré utilisé."
                    .into(),
            ));
            return;
        }
        Err(e) => {
            out.notices.push((Level::Error, format!("SEC, nombre d'actions : échec de la récupération, {e}. Fichier intégré utilisé.")));
            return;
        }
    };

    let known_shares: BTreeMap<NaiveDate, f64> = known
        .iter()
        .filter_map(|p| Some((p.date, p.shares_outstanding?)))
        .collect();
    let mut compared = 0;
    let mut worst: Option<(f64, &sec::SharesFact, f64)> = None;
    for f in &facts {
        if let Some(&k) = known_shares.get(&f.date) {
            compared += 1;
            let gap = f.shares / k - 1.0;
            if worst.is_none_or(|(w, _, _)| gap.abs() > w.abs()) {
                worst = Some((gap, f, k));
            }
        }
    }
    if let Some((gap, f, k)) = worst.filter(|(gap, _, _)| gap.abs() > SHARES_TOLERANCE) {
        out.notices.push((
            Level::Error,
            format!(
                "SEC, nombre d'actions : incohérent avec le fichier intégré au {} ({} ({}) contre {} dans le fichier, écart {:+.1} %). \
                 Classe d'actions manquante ? Valeurs SEC ignorées, fichier intégré utilisé.",
                fmt_day(f.date), group_thousands(f.shares), f.form, group_thousands(k), gap * 100.0
            ),
        ));
        return;
    }

    let last_known = known_shares.keys().next_back().copied();
    let new: Vec<&sec::SharesFact> = facts
        .iter()
        .filter(|f| last_known.is_none_or(|d| f.date > d))
        .collect();
    out.points.extend(new.iter().map(|f| HoldingsPoint {
        date: f.date,
        btc_holdings: None,
        shares_outstanding: Some(f.shares),
    }));
    let last = facts.last().expect("non vide");
    let check = match worst {
        Some((gap, _, _)) => format!(
            "vérifié sur {compared} date(s) communes avec le fichier intégré (écart max {:.2} %)",
            gap.abs() * 100.0
        ),
        None => "aucune date commune avec le fichier intégré pour vérifier".into(),
    };
    out.notices.push((
        Level::Success,
        format!(
            "SEC, nombre d'actions : OK. Dernière valeur publiée {} au {} ({}), {} nouvelle(s) valeur(s) ajoutée(s) ; {check}.",
            group_thousands(last.shares), fmt_day(last.date), last.form, new.len()
        ),
    ));
}

/// BTC détenus : 8-K déposés depuis la dernière date connue.
async fn sec_btc(data: &DataProvider, known: &[HoldingsPoint], out: &mut SecUpdate) {
    let last_known = known
        .iter()
        .filter(|p| p.btc_holdings.is_some())
        .map(|p| p.date)
        .max();
    let url = sec::submissions_url();
    let filings = match data.sec_text(&url).await.and_then(|body| {
        let v = serde_json::from_str(&body).map_err(|e| format!("JSON illisible ({url}) : {e}"))?;
        sec::parse_submissions(&v)
    }) {
        Ok(f) => f,
        Err(e) => {
            out.notices.push((Level::Error, format!("SEC, BTC détenus : échec de la liste des dépôts, {e}. Fichier intégré utilisé.")));
            return;
        }
    };
    let mut recent: Vec<&sec::Filing> = filings
        .iter()
        .filter(|f| f.form == "8-K" && last_known.is_none_or(|d| f.filing_date > d))
        .collect();
    recent.sort_by_key(|f| f.filing_date);
    let skipped = recent.len().saturating_sub(MAX_8K);
    let recent = &recent[skipped..];
    let since = last_known.map_or_else(|| "le début".to_string(), fmt_day);
    if recent.is_empty() {
        out.notices.push((
            Level::Success,
            format!("SEC, BTC détenus : OK. Aucun nouveau 8-K depuis le {since}, données à jour."),
        ));
        return;
    }

    let mut found: Vec<(NaiveDate, f64)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for f in recent {
        match data.sec_text(&f.document_url()).await {
            Ok(html) => {
                if let Some(btc) = sec::extract_btc_holdings(&sec::html_to_text(&html)) {
                    found.push((f.date(), btc));
                }
            }
            Err(e) => failures.push(format!("8-K du {} : {e}", fmt_day(f.filing_date))),
        }
    }
    out.points
        .extend(found.iter().map(|&(date, btc)| HoldingsPoint {
            date,
            btc_holdings: Some(btc),
            shares_outstanding: None,
        }));

    let mut msg = format!(
        "{} 8-K déposé(s) depuis le {since}{} : {} annonce(s) de BTC détenus lue(s)",
        recent.len(),
        if skipped > 0 {
            format!(" ({skipped} plus anciens non lus)")
        } else {
            String::new()
        },
        found.len()
    );
    if let Some((date, btc)) = found.last() {
        msg += &format!(
            ", dernière valeur {} BTC au {}",
            group_thousands(*btc),
            fmt_day(*date)
        );
    }
    if failures.is_empty() {
        out.notices
            .push((Level::Success, format!("SEC, BTC détenus : OK. {msg}.")));
    } else {
        out.notices.push((
            Level::Error,
            format!(
                "SEC, BTC détenus : {msg}, mais {} document(s) illisible(s) : {}.",
                failures.len(),
                failures.join(" ; ")
            ),
        ));
    }
}

/// Dernières publications SEC (actions et BTC détenus), avec leurs messages.
pub async fn sec_update(data: &DataProvider) -> SecUpdate {
    let cfg = parse_config(&data.config().section(CONFIG_SECTION));
    let known = merge_points(embedded_points(), &cfg.points);
    let mut out = SecUpdate::default();
    sec_btc(data, &known, &mut out).await;
    sec_shares(data, &known, &mut out).await;
    out
}

pub async fn render(
    data: &DataProvider,
    params: &serde_json::Value,
) -> Result<IndicatorOutput, String> {
    let show_btc = param_bool(params, "btc", true);
    let update = sec_update(data).await;
    let (holdings, thresholds, config_errors) = load_holdings_with(data.config(), &update.points);
    let first = holdings
        .first()
        .ok_or("Aucune donnée de holdings MSTR.")?
        .date;

    let mstr = data
        .ticker_range("MSTR", Some(first), None)
        .await
        .map_err(|e| format!("Impossible de récupérer le cours de MSTR. {e}"))?;
    let btc = data
        .ticker_range("BTC-USD", Some(first - chrono::Duration::days(7)), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;
    let points = compute(&mstr, &btc, &holdings);
    let last = points
        .last()
        .ok_or("Aucune date commune entre le cours de MSTR, le prix du BTC et les holdings.")?
        .clone();

    let zone = classify(last.mnav, thresholds);
    let mut out = IndicatorOutput::from(build_figure(&points, thresholds, show_btc));
    for (level, text) in update.notices {
        out = out.with_notice(level, text);
    }
    out.metrics = vec![
        Metric {
            label: "mNAV actuel".into(),
            value: format!("{:.2}", last.mnav),
        },
        Metric {
            label: "Zone".into(),
            value: zone.label().into(),
        },
        Metric {
            label: format!("BTC détenus (au {})", last.holdings.date.format("%d/%m/%Y")),
            value: group_thousands(last.holdings.btc),
        },
    ];
    let age = (last.date - last.holdings.date).num_days();
    if age > STALE_DAYS {
        out = out.with_notice(
            Level::Warning,
            format!(
                "La dernière donnée de holdings date du {} ({age} jours avant le dernier cours) : le mNAV récent peut être faussé. \
                 Ajoutez les derniers achats dans config.ini, section [{CONFIG_SECTION}] (ex. « {} = BTC détenus, actions »).",
                last.holdings.date.format("%d/%m/%Y"),
                fmt_date(last.date)
            ),
        );
    }
    for e in config_errors {
        out = out.with_notice(
            Level::Warning,
            format!("config.ini [{CONFIG_SECTION}] : {e}"),
        );
    }
    Ok(out)
}

/// Dernière donnée de holdings retenue (pour l'accueil).
pub fn latest_holdings(config: &Config) -> Option<Holdings> {
    load_holdings(config).0.last().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;
    use serde_json::json;

    fn sec_provider(name: &str, submissions: &str, concept: &str) -> DataProvider {
        let dir = std::env::temp_dir().join(format!(
            "dashboard-crypto-sec-{}-{name}",
            std::process::id()
        ));
        let data = DataProvider::new(Config::new(dir.join("config.ini")))
            .with_cache_dir(dir.join("cache"));
        data.seed_sec(&sec::submissions_url(), submissions);
        data.seed_sec(&sec::shares_concept_url(), concept);
        data
    }

    /// Faits XBRL cohérents avec le fichier intégré (classes A et B au 22/07/2021,
    /// total au 24/07/2026), plus une valeur postérieure au fichier.
    const CONCEPT_OK: &str = r#"{"units": {"shares": [
        {"end": "2021-07-22", "val": 7783443, "accn": "a", "form": "10-Q", "filed": "2021-07-29"},
        {"end": "2021-07-22", "val": 1964025, "accn": "a", "form": "10-Q", "filed": "2021-07-29"},
        {"end": "2026-07-24", "val": 384225751, "accn": "b", "form": "10-Q", "filed": "2026-07-30"},
        {"end": "2026-10-20", "val": 395000000, "accn": "c", "form": "10-Q", "filed": "2026-10-28"}
    ]}}"#;

    fn submissions(filings: &[(&str, &str, &str, &str)]) -> String {
        let col = |k: usize| {
            filings
                .iter()
                .map(|f| json!([f.0, f.1, f.2, f.3][k]))
                .collect::<Vec<_>>()
        };
        json!({"filings": {"recent": {
            "accessionNumber": col(0), "form": col(1), "filingDate": col(2), "reportDate": col(2), "primaryDocument": col(3)
        }}})
        .to_string()
    }

    fn filing(accn: &str, date: &str, doc: &str) -> sec::Filing {
        sec::Filing {
            accession: accn.into(),
            form: "8-K".into(),
            filing_date: d(date),
            report_date: Some(d(date)),
            primary_document: doc.into(),
        }
    }

    #[tokio::test]
    async fn sec_update_adds_new_filings_and_reports_success() {
        let subs = submissions(&[
            ("0001-26-000001", "8-K", "2026-10-05", "buy.htm"),
            ("0001-26-000002", "8-K", "2026-10-06", "vote.htm"),
            ("0001-26-000003", "10-Q", "2026-10-28", "q.htm"),
            ("0001-26-000004", "8-K", "2026-09-01", "old.htm"),
        ]);
        let data = sec_provider("ok", &subs, CONCEPT_OK);
        data.seed_sec(
            &filing("0001-26-000001", "2026-10-05", "buy.htm").document_url(),
            "<p>As of October 4, 2026, the Company held an aggregate of approximately 851,200 bitcoins.</p>",
        );
        data.seed_sec(
            &filing("0001-26-000002", "2026-10-06", "vote.htm").document_url(),
            "<p>Item 5.07 Submission of Matters to a Vote</p>",
        );

        let up = sec_update(&data).await;
        assert_eq!(
            up.points,
            vec![
                p("2026-10-05", Some(851_200.0), None),
                p("2026-10-20", None, Some(395_000_000.0)),
            ]
        );
        assert_eq!(up.notices.len(), 2);
        assert!(
            up.notices.iter().all(|(l, _)| matches!(l, Level::Success)),
            "{:?}",
            up.notices
        );
        assert!(
            up.notices[0].1.contains("2 8-K") && up.notices[0].1.contains("851\u{202F}200 BTC")
        );
        assert!(up.notices[1].1.contains("vérifié sur 2 date(s)"));

        let (h, _, _) = load_holdings_with(data.config(), &up.points);
        let last = h.last().unwrap();
        assert_eq!((last.btc, last.shares), (851_200.0, 395_000_000.0));
    }

    #[tokio::test]
    async fn sec_update_reports_up_to_date_and_rejects_inconsistent_shares() {
        // Classe B absente : ~5 % d'actions en moins que le fichier.
        let concept = r#"{"units": {"shares": [
            {"end": "2021-07-22", "val": 7783443, "accn": "a", "form": "10-Q", "filed": "2021-07-29"},
            {"end": "2026-10-20", "val": 395000000, "accn": "c", "form": "10-Q", "filed": "2026-10-28"}
        ]}}"#;
        let data = sec_provider(
            "stale",
            &submissions(&[("0001-26-000004", "8-K", "2026-09-01", "old.htm")]),
            concept,
        );
        let up = sec_update(&data).await;
        assert!(up.points.is_empty());
        assert!(matches!(up.notices[0].0, Level::Success));
        assert!(up.notices[0].1.contains("Aucun nouveau 8-K"));
        assert!(matches!(up.notices[1].0, Level::Error));
        assert!(
            up.notices[1].1.contains("incohérent"),
            "{}",
            up.notices[1].1
        );
    }

    #[tokio::test]
    async fn sec_update_reports_unreadable_responses() {
        let data = sec_provider("bad", "<html>maintenance</html>", "{}");
        let up = sec_update(&data).await;
        assert!(up.points.is_empty());
        assert_eq!(up.notices.len(), 2);
        assert!(up.notices.iter().all(|(l, _)| matches!(l, Level::Error)));
        assert!(up.notices[0].1.contains("JSON illisible"));
    }

    fn p(date: &str, btc: Option<f64>, shares: Option<f64>) -> HoldingsPoint {
        HoldingsPoint {
            date: d(date),
            btc_holdings: btc,
            shares_outstanding: shares,
        }
    }

    fn candle(date: &str, close: f64) -> Candle {
        Candle {
            date: d(date),
            open: close,
            high: close,
            low: close,
            close,
            volume: 0.0,
        }
    }

    #[test]
    fn embedded_file_is_valid_and_sorted() {
        let pts = embedded_points();
        assert!(
            pts.len() >= 24,
            "au moins un point par trimestre depuis 2020"
        );
        assert!(pts.windows(2).all(|w| w[0].date < w[1].date));
        assert!(pts[0].btc_holdings.is_some() && pts[0].shares_outstanding.is_some());
        let filled = forward_fill(&pts);
        assert_eq!(filled.len(), pts.len());
        assert!(filled.iter().all(|h| h.btc > 0.0 && h.shares > 0.0));
    }

    #[test]
    fn forward_fill_keeps_last_known_value_per_field() {
        let pts = [
            p("2024-01-01", Some(100.0), None), // actions encore inconnues : écarté
            p("2024-01-05", None, Some(1000.0)),
            p("2024-02-01", Some(150.0), None),
            p("2024-03-01", None, Some(1200.0)),
        ];
        let h = forward_fill(&pts);
        assert_eq!(
            h,
            vec![
                Holdings {
                    date: d("2024-01-05"),
                    btc: 100.0,
                    shares: 1000.0
                },
                Holdings {
                    date: d("2024-02-01"),
                    btc: 150.0,
                    shares: 1000.0
                },
                Holdings {
                    date: d("2024-03-01"),
                    btc: 150.0,
                    shares: 1200.0
                },
            ]
        );
        // Pas d'interpolation : entre deux points, la valeur reste celle du point précédent.
        assert_eq!(holdings_at(&h, d("2024-02-29")).unwrap().btc, 150.0);
        assert_eq!(holdings_at(&h, d("2024-02-01")).unwrap().btc, 150.0);
        assert!(holdings_at(&h, d("2024-01-04")).is_none());
    }

    #[test]
    fn mnav_on_a_known_case() {
        // 100 $ × 200 M actions = 20 Md$ ; 400 000 BTC × 50 000 $ = 20 Md$ → mNAV 1.
        assert_eq!(mnav(100.0, 200e6, 400_000.0, 50_000.0), 1.0);
        // Cours doublé → mNAV 2.
        assert_eq!(mnav(200.0, 200e6, 400_000.0, 50_000.0), 2.0);
    }

    #[test]
    fn thresholds_classify_zones() {
        let t = DEFAULT_THRESHOLDS;
        assert_eq!(classify(0.8, t), Zone::Undervalued);
        assert_eq!(classify(1.0, t), Zone::Neutral);
        assert_eq!(classify(1.49, t), Zone::Neutral);
        assert_eq!(classify(1.5, t), Zone::Overvalued);
        assert_eq!(classify(2.5, t), Zone::StronglyOvervalued);
        assert_eq!(classify(0.8, t).label(), "sous-coté");
        assert_eq!(classify(3.0, [1.0, 2.0, 4.0]), Zone::Overvalued);
    }

    #[test]
    fn alignment_keeps_mstr_trading_days() {
        // Vendredi 05/01, lundi 08/01 : le week-end (BTC seul) est ignoré.
        let mstr = [candle("2024-01-05", 50.0), candle("2024-01-08", 55.0)];
        let btc = [
            candle("2024-01-05", 44_000.0),
            candle("2024-01-06", 44_100.0),
            candle("2024-01-07", 44_200.0),
        ];
        let aligned = align_prices(&mstr, &btc);
        // Le 08/01 manque côté BTC : dernière clôture connue (07/01).
        assert_eq!(
            aligned,
            vec![
                (d("2024-01-05"), 50.0, 44_000.0),
                (d("2024-01-08"), 55.0, 44_200.0)
            ]
        );
    }

    #[test]
    fn compute_uses_holdings_in_force_each_day() {
        let holdings = [
            Holdings {
                date: d("2024-01-01"),
                btc: 400_000.0,
                shares: 200e6,
            },
            Holdings {
                date: d("2024-01-08"),
                btc: 500_000.0,
                shares: 200e6,
            },
        ];
        let mstr = [candle("2024-01-05", 100.0), candle("2024-01-08", 100.0)];
        let btc = [
            candle("2024-01-05", 50_000.0),
            candle("2024-01-08", 50_000.0),
        ];
        let pts = compute(&mstr, &btc, &holdings);
        assert_eq!(pts[0].mnav, 1.0);
        assert_eq!(pts[1].mnav, 0.8);
        assert_eq!(pts[1].holdings.date, d("2024-01-08"));
    }

    #[test]
    fn config_adds_and_overrides_points() {
        let entries = vec![
            ("seuils".to_string(), "0.9, 1.4, 2.0".to_string()),
            ("2026-10-05".to_string(), "850 000, 390_000_000".to_string()),
            ("2026-10-12".to_string(), "851000".to_string()),
            ("2024-12-31".to_string(), "447470, 246000000".to_string()),
            ("demain".to_string(), "1".to_string()),
            ("2026-10-19".to_string(), "abc".to_string()),
        ];
        let cfg = parse_config(&entries);
        assert_eq!(cfg.thresholds, Some([0.9, 1.4, 2.0]));
        assert_eq!(cfg.errors.len(), 2, "{:?}", cfg.errors);
        assert_eq!(cfg.points[0], p("2026-10-05", Some(850_000.0), Some(390e6)));
        assert_eq!(cfg.points[1], p("2026-10-12", Some(851_000.0), None));

        let merged = merge_points(embedded_points(), &cfg.points);
        let dec = merged.iter().find(|x| x.date == d("2024-12-31")).unwrap();
        assert_eq!(
            dec.shares_outstanding,
            Some(246e6),
            "la configuration remplace le fichier"
        );
        let h = forward_fill(&merged);
        let last = h.last().unwrap();
        assert_eq!(
            (last.date, last.btc, last.shares),
            (d("2026-10-12"), 851_000.0, 390e6)
        );
    }

    #[test]
    fn invalid_thresholds_are_rejected() {
        assert_eq!(parse_thresholds("1, 1.5, 2.5"), Some([1.0, 1.5, 2.5]));
        assert!(parse_thresholds("1.5, 1.0, 2.5").is_none());
        assert!(parse_thresholds("1, 2").is_none());
        assert!(parse_thresholds("0, 1, 2").is_none());
    }

    #[test]
    fn colorscale_stops_follow_thresholds() {
        let (cmax, scale) = colorscale(DEFAULT_THRESHOLDS);
        assert_eq!(cmax, 3.5);
        assert!(scale.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(scale.first().unwrap().0, 0.0);
        assert_eq!(scale.last().unwrap().0, 1.0);
        // Le jaune (neutre / sur-coté) tombe sur le seuil 1.5.
        assert!(scale
            .iter()
            .any(|(pos, c)| (*pos - 1.5 / 3.5).abs() < 1e-12 && *c == "#fee08b"));
    }

    #[test]
    fn figure_has_price_markers_mnav_and_title() {
        let h = Holdings {
            date: d("2024-01-01"),
            btc: 400_000.0,
            shares: 200e6,
        };
        let pts = vec![
            MnavPoint {
                date: d("2024-01-05"),
                mstr: 100.0,
                btc_price: 50_000.0,
                holdings: h,
                mnav: 1.0,
            },
            MnavPoint {
                date: d("2024-01-08"),
                mstr: 60.0,
                btc_price: 50_000.0,
                holdings: h,
                mnav: 0.6,
            },
        ];
        let fig = build_figure(&pts, DEFAULT_THRESHOLDS, true);
        let title = fig.layout["title"]["text"].as_str().unwrap();
        assert!(
            title.contains("0.60") && title.contains("sous-coté") && title.contains("01/01/2024"),
            "{title}"
        );
        assert_eq!(fig.data[1]["marker"]["color"], json!([1.0, 0.6]));
        assert!(
            fig.data.iter().any(|t| t["yaxis"] == "y3"),
            "prix BTC sur l'axe secondaire"
        );
        assert!(fig
            .data
            .iter()
            .any(|t| t["yaxis"] == "y2" && t["name"] == "mNAV"));
        assert!(!build_figure(&pts, DEFAULT_THRESHOLDS, false)
            .data
            .iter()
            .any(|t| t["yaxis"] == "y3"));
    }
}

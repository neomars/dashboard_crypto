//! Realized Cap HODL Waves : part du Realized Cap détenue par chaque tranche
//! d'ancienneté des UTXO (BGeometrics).

use super::{IndicatorOutput, Level};
use crate::bgeometrics;
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use crate::series::opt_json;
use crate::table::{self, Row};
use serde_json::json;

/// Ancienneté maximale (en jours) d'une tranche d'après son nom (`24h`,
/// `1d_1w`, `1m-3m`, `age3y5y`, `10y+`...) : dernier nombre suivi d'une unité
/// h/d/w/m/y (la borne haute, pour départager `24h` et `1d_1w`).
pub(crate) fn age_days(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let bytes = lower.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let unit = match bytes.get(i) {
            Some(b'h') => 1.0 / 24.0,
            Some(b'd') => 1.0,
            Some(b'w') => 7.0,
            Some(b'm') => 30.0,
            Some(b'y') => 365.0,
            _ => continue,
        };
        if let Ok(value) = lower[start..i].parse::<f64>() {
            last = Some(value * unit);
        }
    }
    last
}

/// Tranches triées de la plus ancienne à la plus récente (la plus ancienne
/// en bas de l'empilement) ; ordre de l'API si les noms ne sont pas lisibles.
pub(crate) fn band_columns(rows: &[Row]) -> Vec<String> {
    let mut cols = bgeometrics::value_columns(rows);
    if cols.iter().all(|c| age_days(c).is_some()) {
        cols.sort_by(|a, b| age_days(b).unwrap().total_cmp(&age_days(a).unwrap()));
    }
    cols
}

/// Parts en % de chaque tranche (la somme de chaque ligne vaut 100).
pub(crate) fn percentages(rows: &[&Row], bands: &[String]) -> Vec<Vec<Option<f64>>> {
    rows.iter()
        .map(|r| {
            let values: Vec<Option<f64>> = bands.iter().map(|b| table::num(r, b)).collect();
            let total: f64 = values.iter().flatten().sum();
            values
                .into_iter()
                .map(|v| v.filter(|_| total > 0.0).map(|v| v / total * 100.0))
                .collect()
        })
        .collect()
}

fn color(i: usize, n: usize) -> String {
    // Dégradé du vert foncé (anciens détenteurs) au rouge (pièces récentes).
    let t = if n > 1 {
        i as f64 / (n - 1) as f64
    } else {
        0.0
    };
    format!(
        "hsl({:.0}, 70%, {:.0}%)",
        150.0 - 150.0 * t,
        25.0 + 30.0 * t
    )
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let waves = data.bgeometrics("realized_cap_hodl_waves").await?;
    let bands = band_columns(&waves.rows);
    if bands.is_empty() {
        return Err(format!(
            "Aucune tranche d'âge dans les données. Colonnes : {:?}",
            table::columns(&waves.rows)
        ));
    }
    let rows = bgeometrics::dated_rows(&waves.rows);
    let x = dates(rows.iter().map(|(d, _)| *d));
    let pct = percentages(&rows.iter().map(|(_, r)| *r).collect::<Vec<_>>(), &bands);

    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin : Realized Cap HODL Waves (%)"},
        "height": 750, "hovermode": "x unified",
        "legend": {"orientation": "h", "y": -0.15, "x": 0.5, "xanchor": "center"},
        "yaxis": {"title": {"text": "Part du Realized Cap (%)"}, "range": [0, 100], "ticksuffix": "%"},
        "xaxis": {"title": {"text": "Date"}}, "margin": {"t": 50, "b": 100}
    }));
    for (i, band) in bands.iter().enumerate() {
        let c = color(i, bands.len());
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": band, "x": x, "stackgroup": "one", "fillcolor": c,
            "y": pct.iter().map(|row| opt_json(row[i])).collect::<Vec<_>>(),
            "line": {"width": 0.5, "color": "rgba(255,255,255,0.2)"}, "hovertemplate": "%{y:.1f} %"}));
    }
    let out = IndicatorOutput::from(fig);
    Ok(match waves.stale_notice() {
        Some(n) => out.with_notice(Level::Warning, n),
        None => out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ages_from_band_names() {
        assert_eq!(age_days("24h"), Some(1.0));
        assert_eq!(age_days("1d_1w"), Some(7.0));
        assert_eq!(age_days("age_1m_3m"), Some(90.0));
        assert_eq!(age_days("10y+"), Some(3650.0));
        assert_eq!(age_days("unixTs"), None);
    }

    #[test]
    fn bands_sorted_oldest_first_and_normalised() {
        let rows: Vec<Row> = vec![
            json!({"d": "2024-01-01", "unixTs": 1, "1d_1w": 1.0, "24h": 0.0, "1y_2y": 3.0, "10y": 4.0})
                .as_object()
                .unwrap()
                .clone(),
        ];
        let bands = band_columns(&rows);
        assert_eq!(bands, vec!["10y", "1y_2y", "1d_1w", "24h"]);
        let pct = percentages(&[&rows[0]], &bands);
        assert_eq!(pct[0], vec![Some(50.0), Some(37.5), Some(12.5), Some(0.0)]);
    }

    #[test]
    fn unreadable_names_keep_api_order() {
        let rows: Vec<Row> = vec![json!({"d": "2024-01-01", "young": 1.0, "old": 2.0})
            .as_object()
            .unwrap()
            .clone()];
        assert_eq!(band_columns(&rows), vec!["young", "old"]);
    }
}

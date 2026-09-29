use super::IndicatorOutput;
use crate::data::{parse_ymd, DataProvider};
use crate::figure::Figure;
use crate::series::{rolling_mean, rolling_std};
use chrono::NaiveDate;
use serde_json::json;

const ROTATION_DAYS: f64 = 4.0 * 365.25;

/// Palette ColorBrewer « RdYlGn » (rouge → vert), inconnue de Plotly.js par son nom.
const RD_YL_GN: [(f64, &str); 11] = [
    (0.0, "#a50026"),
    (0.1, "#d73027"),
    (0.2, "#f46d43"),
    (0.3, "#fdae61"),
    (0.4, "#fee08b"),
    (0.5, "#ffffbf"),
    (0.6, "#d9ef8b"),
    (0.7, "#a6d96a"),
    (0.8, "#66bd63"),
    (0.9, "#1a9850"),
    (1.0, "#006837"),
];

/// Angle (degrés) sur la roue : un tour complet tous les 4 ans depuis le 01/01/2009.
pub(crate) fn angle(date: NaiveDate) -> f64 {
    (date - parse_ymd("2009-01-01")).num_days() as f64 / ROTATION_DAYS * 360.0
}

/// Z-score du prix par rapport à sa moyenne 200 j, borné à [-2, 4] (0 avant 200 j).
pub(crate) fn zscores(closes: &[f64]) -> Vec<f64> {
    let mean = rolling_mean(closes, 200);
    let std = rolling_std(closes, 200);
    closes
        .iter()
        .zip(mean.iter().zip(&std))
        .map(|(c, (m, s))| match (m, s) {
            (Some(m), Some(s)) if *s > 0.0 => ((c - m) / s).clamp(-2.0, 4.0),
            _ => 0.0,
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data.ticker_history("BTC-USD").await?;
    let closes: Vec<f64> = btc.iter().map(|c| c.close).collect();
    let halvings: Vec<NaiveDate> = [
        "2009-01-03",
        "2012-11-28",
        "2016-07-09",
        "2020-05-11",
        "2024-04-20",
    ]
    .map(parse_ymd)
    .to_vec();
    let days_post = |d: NaiveDate| {
        halvings
            .iter()
            .rev()
            .find(|h| **h <= d)
            .map(|h| (d - *h).num_days())
            .unwrap_or(0)
    };

    let mut fig = Figure::new(json!({
        "polar": {
            "bgcolor": "black",
            "angularaxis": {"direction": "clockwise", "period": 360, "visible": false, "rotation": 90},
            "radialaxis": {"visible": false, "range": [0, 12]}
        },
        "showlegend": false, "paper_bgcolor": "black",
        "margin": {"l": 40, "r": 40, "t": 80, "b": 40}, "height": 900,
        "title": {"text": "BITCOIN 4-YEAR CYCLE - Hodler's Cheat Sheet", "x": 0.5, "y": 0.98,
                  "font": {"size": 24, "family": "serif", "color": "#00FFAA"}}
    }));
    let none = |t: serde_json::Value| {
        let mut t = t;
        t["type"] = json!("scatterpolar");
        t["hoverinfo"] = json!("none");
        t["showlegend"] = json!(false);
        t
    };

    // Secteurs de fond (une année par quadrant)
    let colors = [
        "rgba(0, 255, 0, 0.03)",
        "rgba(255, 255, 0, 0.03)",
        "rgba(255, 0, 0, 0.03)",
        "rgba(0, 255, 255, 0.03)",
    ];
    for (i, color) in colors.iter().enumerate() {
        let start = i as f64 * 90.0;
        let mut theta: Vec<f64> = (0..50).map(|k| start + 90.0 * k as f64 / 49.0).collect();
        let mut r = vec![11.0; theta.len()];
        theta.push(start);
        r.push(0.0);
        fig.trace(none(json!({"r": r, "theta": theta, "fill": "toself", "fillcolor": color, "line": {"color": "rgba(0,0,0,0)"}})));
    }

    // Niveaux de prix (cercles concentriques)
    for p in [1u64, 10, 100, 1_000, 10_000, 100_000, 1_000_000] {
        let r = (p as f64).log10();
        fig.trace(none(
            json!({"r": vec![r; 361], "theta": (0..=360).collect::<Vec<_>>(), "mode": "lines",
            "line": {"color": "rgba(100,100,100,0.1)", "width": 1}}),
        ));
        fig.trace(none(
            json!({"r": [r], "theta": [0], "mode": "text", "text": [format!("${}", thousands(p))],
            "textfont": {"size": 10, "color": "#888888"}}),
        ));
    }

    // Spirale du prix
    let text: Vec<String> = btc
        .iter()
        .map(|c| {
            format!(
                "Date: {}<br>Prix: ${:.2}<br>Jours post-halving: {}",
                c.date,
                c.close,
                days_post(c.date)
            )
        })
        .collect();
    fig.trace(json!({
        "type": "scatterpolar", "mode": "markers", "name": "Prix BTC", "hoverinfo": "text", "text": text,
        "r": closes.iter().map(|c| c.log10()).collect::<Vec<_>>(),
        "theta": btc.iter().map(|c| angle(c.date)).collect::<Vec<_>>(),
        "marker": {"size": 4, "color": zscores(&closes), "colorscale": RD_YL_GN, "reversescale": true, "showscale": true,
            "colorbar": {"title": {"text": "Sentiment (Z-Score)"}, "thickness": 15, "x": 1.05, "tickvals": [-2, 1, 4], "ticktext": ["Froid", "Neutre", "Chaud"]}}
    }));

    for (angle, label) in [
        (0, "2009, '13, '17, '21, '25"),
        (90, "2010, '14, '18, '22, '26"),
        (180, "2011, '15, '19, '23, '27"),
        (270, "2012, '16, '20, '24, '28"),
    ] {
        fig.trace(none(json!({"r": [0, 11], "theta": [angle, angle], "mode": "lines", "line": {"color": "rgba(255,255,255,0.15)", "width": 1.5}})));
        fig.trace(none(json!({"r": [11.5], "theta": [angle], "mode": "text", "text": [label], "textfont": {"size": 11, "color": "#00FFAA", "family": "serif"}})));
    }

    let phases = [
        "BELIEF",
        "INTERMISSION",
        "THRILL",
        "EUPHORIA",
        "COMPLACENCY",
        "DENIAL",
        "PANIC",
        "CAPITULATION",
        "DEPRESSION",
        "ENDURING",
        "STAGNATION",
        "DISBELIEF",
        "DOUBT",
        "HOPE",
        "CALM",
        "OPTIMISM",
    ];
    fig.trace(none(json!({"r": vec![9.5; phases.len()], "theta": (0..phases.len()).map(|i| 11.25 + 22.5 * i as f64).collect::<Vec<_>>(),
        "mode": "text", "text": phases, "textfont": {"size": 9, "color": "#AAAAAA", "family": "serif"}})));
    fig.trace(none(json!({"r": [7.5, 7.5, 7.5, 7.5], "theta": [45, 135, 225, 315], "mode": "text",
        "text": ["ANNÉE 1", "ANNÉE 2", "ANNÉE 3", "ANNÉE 4"], "textfont": {"size": 24, "color": "rgba(255,255,255,0.05)", "family": "serif"}})));
    Ok(fig.into())
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_turn_every_four_years() {
        assert_eq!(angle(parse_ymd("2009-01-01")), 0.0);
        assert!((angle(parse_ymd("2013-01-01")) - 360.0).abs() < 0.5);
    }

    #[test]
    fn zscore_is_zero_before_window_and_clamped() {
        let mut closes = vec![100.0; 250];
        closes[210] = 101.0;
        closes.push(1_000_000.0);
        let z = zscores(&closes);
        assert_eq!(z[100], 0.0);
        assert_eq!(*z.last().unwrap(), 4.0);
    }

    #[test]
    fn thousands_separator() {
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(100), "100");
    }
}

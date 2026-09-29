use super::IndicatorOutput;
use crate::data::{parse_ymd, DataProvider};
use crate::figure::{dates, Figure};
use crate::series::PricePoint;
use serde_json::json;

struct Cycle {
    name: &'static str,
    bottom: &'static str,
    top: &'static str,
}

const CYCLES: [Cycle; 4] = [
    Cycle {
        name: "Cycle 2011-2013",
        bottom: "2011-11-01",
        top: "2013-12-17",
    },
    Cycle {
        name: "Cycle 2013-2017",
        bottom: "2015-01-15",
        top: "2017-12-17",
    },
    Cycle {
        name: "Cycle 2018-2021",
        bottom: "2018-12-15",
        top: "2021-11-10",
    },
    Cycle {
        name: "Cycle 2022-2025",
        bottom: "2022-11-21",
        top: "2025-10-15",
    },
];

/// Baisses (en %, négatives) d'au moins `min_drop` % par rapport au plus haut
/// atteint depuis le début de la série, jour par jour.
pub fn find_corrections(closes: &[f64], min_drop: f64) -> Vec<f64> {
    let Some(&first) = closes.first() else {
        return Vec::new();
    };
    let mut peak = first;
    let mut out = Vec::new();
    for &c in &closes[1..] {
        if c > peak {
            peak = c;
        } else {
            let drop = (c / peak - 1.0) * 100.0;
            if drop <= min_drop {
                out.push(drop);
            }
        }
    }
    out
}

pub(crate) struct CycleStats {
    pub name: &'static str,
    pub avg: f64,
    pub max: f64,
}

pub(crate) fn cycle_stats(btc: &[PricePoint]) -> Vec<CycleStats> {
    let round1 = |v: f64| (v * 10.0).round() / 10.0;
    CYCLES
        .iter()
        .filter_map(|c| {
            let (b, t) = (parse_ymd(c.bottom), parse_ymd(c.top));
            let closes: Vec<f64> = btc
                .iter()
                .filter(|p| p.date >= b && p.date <= t)
                .map(|p| p.close)
                .collect();
            let corr = find_corrections(&closes, -15.0);
            if corr.is_empty() {
                return None;
            }
            let abs: Vec<f64> = corr.iter().map(|v| v.abs()).collect();
            Some(CycleStats {
                name: c.name,
                avg: round1(abs.iter().sum::<f64>() / abs.len() as f64),
                max: round1(abs.iter().cloned().fold(0.0, f64::max)),
            })
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data.combined_btc_history().await?;
    let (first, last) = (btc[0].date, btc[btc.len() - 1].date);
    let stats = cycle_stats(&btc);

    // Deux graphiques superposés ; celui du bas a un axe X par catégorie (cycles),
    // indépendant de l'axe des dates.
    let mut fig = Figure::new(json!({
        "height": 850, "hovermode": "x unified", "barmode": "group", "margin": {"t": 100},
        "xaxis": {"anchor": "y", "domain": [0, 1], "title": {"text": "Date"}},
        "yaxis": {"anchor": "x", "domain": [0.47, 1], "title": {"text": "BTC Price (USD)"}, "type": "log"},
        "xaxis2": {"anchor": "y2", "domain": [0, 1]},
        "yaxis2": {"anchor": "x2", "domain": [0, 0.3], "title": {"text": "Correction (%)"}},
        "annotations": [
            {"text": "Prix BTC (échelle logarithmique)", "x": 0.5, "y": 1, "xref": "paper", "yref": "paper", "xanchor": "center", "yanchor": "bottom", "showarrow": false, "font": {"size": 16}},
            {"text": "Bitcoin Cycle Correction Analysis", "x": 0.5, "y": 0.3, "xref": "paper", "yref": "paper", "xanchor": "center", "yanchor": "bottom", "showarrow": false, "font": {"size": 16}}
        ]
    }));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": dates(btc.iter().map(|p| p.date)),
        "y": btc.iter().map(|p| p.close).collect::<Vec<_>>(), "line": {"color": "#00CCFF", "width": 2}}));

    // Repères positionnés en fraction de hauteur (valable en échelle linéaire comme logarithmique).
    let marker = |fig: &mut Figure,
                  x: &str,
                  y: f64,
                  text: &str,
                  color: &str,
                  dash: &str,
                  bg: Option<&str>| {
        fig.vline(x, color, 2.0, dash, "y domain");
        let mut a = json!({"x": x, "y": y, "xref": "x", "yref": "y domain", "text": text, "showarrow": true, "arrowhead": 2, "arrowcolor": color, "font": {"size": 9}});
        if let Some(bg) = bg {
            a["bgcolor"] = json!(bg);
            a["font"] = json!({"color": "white", "size": 10});
        }
        fig.annotation(a);
    };
    for (date, label) in [
        ("2012-11-28", "1er Halving"),
        ("2016-07-09", "2ème Halving"),
        ("2020-05-11", "3ème Halving"),
        ("2024-04-20", "4ème Halving"),
    ] {
        if parse_ymd(date) >= first {
            marker(
                &mut fig,
                date,
                0.85,
                label,
                "red",
                "dash",
                Some("rgba(200,0,0,0.8)"),
            );
        }
    }
    for c in &CYCLES {
        if parse_ymd(c.bottom) >= first {
            marker(&mut fig, c.bottom, 0.7, "Bottom", "lime", "dot", None);
        }
        if parse_ymd(c.top) <= last {
            marker(&mut fig, c.top, 0.9, "Top", "orange", "dot", None);
        }
    }

    let names: Vec<&str> = stats.iter().map(|s| s.name).collect();
    fig.trace(json!({"type": "bar", "name": "Moyenne des corrections", "x": names, "y": stats.iter().map(|s| s.avg).collect::<Vec<_>>(),
        "xaxis": "x2", "yaxis": "y2", "marker": {"color": "#FFAA00"}, "hovertemplate": "Cycle: %{x}<br>Moyenne: %{y:.1f}%<extra></extra>"}));
    fig.trace(json!({"type": "bar", "name": "Correction max", "x": names, "y": stats.iter().map(|s| s.max).collect::<Vec<_>>(),
        "xaxis": "x2", "yaxis": "y2", "marker": {"color": "#FF3333"}, "hovertemplate": "Cycle: %{x}<br>Max: %{y:.1f}%<extra></extra>"}));
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_correction_when_price_only_rises() {
        assert!(find_corrections(&[100.0, 110.0, 120.0, 130.0], -15.0).is_empty());
    }

    #[test]
    fn detects_single_correction_below_threshold() {
        let c = find_corrections(&[100.0, 120.0, 100.0], -15.0);
        assert_eq!(c.len(), 1);
        assert!(c[0] < -15.0);
    }

    #[test]
    fn ignores_correction_above_threshold() {
        assert!(find_corrections(&[100.0, 120.0, 110.0], -15.0).is_empty());
    }

    #[test]
    fn empty_series_returns_empty() {
        assert!(find_corrections(&[], -15.0).is_empty());
    }

    #[test]
    fn stats_average_and_max_per_cycle() {
        let d = parse_ymd;
        let btc = vec![
            PricePoint {
                date: d("2016-01-01"),
                close: 100.0,
            },
            PricePoint {
                date: d("2016-02-01"),
                close: 80.0,
            },
            PricePoint {
                date: d("2016-03-01"),
                close: 60.0,
            },
        ];
        let s = cycle_stats(&btc);
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].name, s[0].avg, s[0].max),
            ("Cycle 2013-2017", 30.0, 40.0)
        );
    }
}

//! Figures Plotly de la page « Fibonacci à levier » : prix avec zigzag, niveaux
//! et trades ; zoom sur un trade ; courbe du budget ; histogramme des R ;
//! histogramme placebo.

use crate::chart::{rows_layout, Chart};
use crate::contract::Side;
use crate::fibo::{Analysis, Setup};
use crate::leveraged::{ExitReason, Trade};
use crate::market::{fmt_time, Bar, Interval};
use crate::validation::PlaceboResult;
use serde_json::{json, Value};

fn legend_top() -> Value {
    json!({"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1})
}

/// Prix sur l'unité de détection avec le zigzag des pivots confirmés, le point
/// de confirmation de chacun, les niveaux des mises en place et les trades.
///
/// `bars` : bougies de détection affichées ; `offset` : index de `bars[0]` dans
/// la suite analysée (les index des pivots et des mises en place en dépendent).
pub fn price_chart(
    bars: &[Bar],
    offset: usize,
    interval: Interval,
    analysis: &Analysis,
    trades: &[Trade],
    title: &str,
) -> Chart {
    let x: Vec<String> = bars.iter().map(|b| fmt_time(b.time, interval)).collect();
    let col = |f: fn(&Bar) -> f64| bars.iter().map(f).collect::<Vec<f64>>();
    let (t_min, t_max) = (
        bars.first().map_or(0, |b| b.time),
        bars.last().map_or(0, |b| b.time + interval.millis()),
    );
    let in_window = |t: i64| t >= t_min && t <= t_max;
    let tm = |t: i64| fmt_time(t, Interval::M1);

    let mut data = vec![json!({"type": "candlestick", "name": "Cours", "x": x,
        "open": col(|b| b.open), "high": col(|b| b.high), "low": col(|b| b.low), "close": col(|b| b.close),
        "increasing": {"line": {"color": "#00ff88"}}, "decreasing": {"line": {"color": "#ff3366"}}})];

    // Zigzag : les pivots confirmés, reliés dans l'ordre.
    let pivots: Vec<_> = analysis
        .pivots
        .iter()
        .filter(|p| in_window(p.time))
        .collect();
    data.push(json!({"type": "scatter", "mode": "lines+markers", "name": "Zigzag (pivots confirmés)",
        "x": pivots.iter().map(|p| tm(p.time)).collect::<Vec<_>>(),
        "y": pivots.iter().map(|p| p.price).collect::<Vec<_>>(),
        "line": {"color": "#FFD166", "width": 1.5}, "marker": {"size": 6, "color": "#FFD166"},
        "text": pivots.iter().map(|p| format!("{} confirmé le {}",
            if p.kind == crate::fibo::PivotKind::High { "Sommet" } else { "Creux" }, tm(p.confirmed_time))).collect::<Vec<_>>(),
        "hoverinfo": "text+y"}));
    // Confirmation : l'instant où le pivot devient connu, sur la clôture de cette bougie.
    let confirm: Vec<(String, f64)> = pivots
        .iter()
        .filter_map(|p| {
            let b = bars.get(p.confirmed_idx.checked_sub(offset)?)?;
            Some((tm(p.confirmed_time), b.close))
        })
        .collect();
    data.push(json!({"type": "scatter", "mode": "markers", "name": "Confirmation du pivot",
        "x": confirm.iter().map(|c| c.0.clone()).collect::<Vec<_>>(),
        "y": confirm.iter().map(|c| c.1).collect::<Vec<_>>(),
        "marker": {"symbol": "diamond-open", "size": 9, "color": "#FFD166", "line": {"width": 1.5}}}));

    // Niveaux : de la création de la mise en place à sa fin (nouveau pivot ou délai).
    let setups: Vec<&Setup> = analysis
        .setups
        .iter()
        .filter(|s| in_window(s.created_time()))
        .collect();
    let (mut lx, mut ly, mut lt) = (Vec::new(), Vec::new(), Vec::new());
    for (k, s) in setups.iter().enumerate() {
        let end = setups.get(k + 1).map_or(t_max, |n| n.created_time()).min(
            s.created_time()
                + s.expires_idx.saturating_sub(s.created_idx()) as i64 * interval.millis(),
        );
        for l in &s.levels {
            lx.extend([
                Value::from(tm(s.created_time())),
                Value::from(tm(end)),
                Value::Null,
            ]);
            ly.extend([json!(l.price), json!(l.price), Value::Null]);
            lt.extend([
                json!(format!("{:.1} %", l.ratio * 100.0)),
                json!(format!("{:.1} %", l.ratio * 100.0)),
                Value::Null,
            ]);
        }
    }
    data.push(json!({"type": "scatter", "mode": "lines", "name": "Niveaux de retracement", "x": lx, "y": ly,
        "text": lt, "hoverinfo": "text+y", "connectgaps": false,
        "line": {"color": "rgba(100, 180, 255, 0.85)", "width": 1, "dash": "dot"}}));

    // Trades : entrées et sorties.
    let shown: Vec<&Trade> = trades.iter().filter(|t| in_window(t.entry_time)).collect();
    let entry_symbol = |s: Side| {
        if s == Side::Long {
            "triangle-up"
        } else {
            "triangle-down"
        }
    };
    data.push(json!({"type": "scatter", "mode": "markers", "name": "Entrées",
        "x": shown.iter().map(|t| tm(t.entry_time)).collect::<Vec<_>>(),
        "y": shown.iter().map(|t| t.entry_price).collect::<Vec<_>>(),
        "text": shown.iter().map(|t| format!("{} {:.1} % à {:.1} (x{})", t.side.label(), t.ratio * 100.0, t.entry_price, t.leverage)).collect::<Vec<_>>(),
        "hoverinfo": "text",
        "marker": {"size": 11, "symbol": shown.iter().map(|t| entry_symbol(t.side)).collect::<Vec<_>>(),
            "color": "#19d3f3", "line": {"color": "white", "width": 1}}}));
    let exit_color = |r: ExitReason| match r {
        ExitReason::Target => "#00ff88",
        ExitReason::Stop => "#ff3366",
        ExitReason::Liquidation => "#ffa500",
        ExitReason::Expiration => "#9aa0a6",
    };
    data.push(json!({"type": "scatter", "mode": "markers", "name": "Sorties",
        "x": shown.iter().map(|t| tm(t.exit_time)).collect::<Vec<_>>(),
        "y": shown.iter().map(|t| t.exit_price).collect::<Vec<_>>(),
        "text": shown.iter().map(|t| format!("{:?} : {:+.2} R, {:+.2} USDT", t.reason, t.r_net, t.net)).collect::<Vec<_>>(),
        "hoverinfo": "text",
        "marker": {"size": 10, "symbol": "x", "color": shown.iter().map(|t| exit_color(t.reason)).collect::<Vec<_>>(),
            "line": {"color": "white", "width": 1}}}));

    let mut layout = json!({"title": {"text": title, "y": 0.985, "yanchor": "top"}, "height": 640,
        "margin": {"t": 110}, "hovermode": "closest", "legend": legend_top(),
        "xaxis": {"rangeslider": {"visible": false}}, "yaxis": {"title": {"text": "Prix (USDT)"}}});
    layout["legend"] = legend_top();
    Chart { data, layout }
}

/// Zoom sur un trade : bougies 1 minute autour de l'entrée et de la sortie, avec
/// le niveau, le stop, l'objectif et la liquidation.
pub fn trade_zoom_chart(trade: &Trade, minutes: &[Bar]) -> Chart {
    let tm = |t: i64| fmt_time(t, Interval::M1);
    let x: Vec<String> = minutes.iter().map(|b| tm(b.time)).collect();
    let col = |f: fn(&Bar) -> f64| minutes.iter().map(f).collect::<Vec<f64>>();
    let (x0, x1) = (
        x.first().cloned().unwrap_or_default(),
        x.last().cloned().unwrap_or_default(),
    );
    let mut data = vec![json!({"type": "candlestick", "name": "1 minute", "x": x,
        "open": col(|b| b.open), "high": col(|b| b.high), "low": col(|b| b.low), "close": col(|b| b.close),
        "increasing": {"line": {"color": "#00ff88"}}, "decreasing": {"line": {"color": "#ff3366"}}})];
    let mut lines: Vec<(String, f64, &str, &str)> = vec![
        (
            format!("Entrée {:.1}", trade.entry_price),
            trade.entry_price,
            "#19d3f3",
            "dot",
        ),
        (
            format!("Stop {:.1}", trade.stop),
            trade.stop,
            "#ff3366",
            "dash",
        ),
        (
            format!("Objectif {:.1}", trade.target),
            trade.target,
            "#00ff88",
            "dash",
        ),
    ];
    if let Some(l) = trade.liquidation {
        lines.push((format!("Liquidation {l:.1}"), l, "#ffa500", "dashdot"));
    }
    for (name, y, color, dash) in lines {
        data.push(
            json!({"type": "scatter", "mode": "lines", "name": name, "x": [x0, x1], "y": [y, y],
            "line": {"color": color, "width": 1.2, "dash": dash}}),
        );
    }
    data.push(json!({"type": "scatter", "mode": "markers+text", "name": "Entrée / sortie",
        "x": [tm(trade.entry_time), tm(trade.exit_time)], "y": [trade.entry_price, trade.exit_price],
        "text": [trade.side.label().to_string(), format!("{:?}", trade.reason)], "textposition": "top center",
        "marker": {"size": 12, "symbol": [if trade.side == Side::Long { "triangle-up" } else { "triangle-down" }, "x"],
            "color": ["#19d3f3", "#FECB52"], "line": {"color": "white", "width": 1}}}));
    let layout = json!({"title": {"text": format!("Trade n°{} : {} au niveau {:.1} %, {:+.2} R ({:+.2} USDT)",
            trade.id + 1, trade.side.label(), trade.ratio * 100.0, trade.r_net, trade.net), "y": 0.985, "yanchor": "top"},
        "height": 520, "margin": {"t": 110}, "legend": legend_top(),
        "xaxis": {"rangeslider": {"visible": false}}, "yaxis": {"title": {"text": "Prix (USDT)"}}});
    Chart { data, layout }
}

/// Courbe du budget, après chaque trade.
pub fn budget_chart(title: &str, curve: &[(i64, f64)], initial: f64) -> Chart {
    let x: Vec<String> = curve.iter().map(|c| fmt_time(c.0, Interval::M1)).collect();
    let y: Vec<f64> = curve.iter().map(|c| c.1).collect();
    let data = vec![
        json!({"type": "scatter", "mode": "lines", "name": "Budget (USDT)", "x": x, "y": y,
            "line": {"color": "#00FFAA", "width": 1.8, "shape": "hv"}}),
        json!({"type": "scatter", "mode": "lines", "name": "Budget initial",
            "x": [x.first().cloned().unwrap_or_default(), x.last().cloned().unwrap_or_default()], "y": [initial, initial],
            "line": {"color": "rgba(255,255,255,0.5)", "width": 1, "dash": "dot"}}),
    ];
    let layout = json!({"title": {"text": title}, "height": 340, "margin": {"t": 60}, "legend": legend_top(),
        "yaxis": {"title": {"text": "Budget (USDT)"}}});
    Chart { data, layout }
}

/// Histogramme des résultats nets, en R.
pub fn r_histogram(title: &str, trades: &[Trade]) -> Chart {
    let r: Vec<f64> = trades.iter().map(|t| t.r_net).collect();
    let mean = (!r.is_empty()).then(|| r.iter().sum::<f64>() / r.len() as f64);
    let mut data = vec![
        json!({"type": "histogram", "name": "Résultat net par trade", "x": r,
        "xbins": {"size": 0.5}, "marker": {"color": "#636efa"}}),
    ];
    let mut shapes = vec![
        json!({"type": "line", "x0": 0, "x1": 0, "yref": "paper", "y0": 0, "y1": 1,
        "line": {"color": "rgba(255,255,255,0.6)", "width": 1, "dash": "dot"}}),
    ];
    if let Some(m) = mean {
        shapes.push(
            json!({"type": "line", "x0": m, "x1": m, "yref": "paper", "y0": 0, "y1": 1,
            "line": {"color": "#FECB52", "width": 2}}),
        );
        data.push(
            json!({"type": "scatter", "mode": "lines", "name": format!("Espérance {m:+.3} R"),
            "x": [m, m], "y": [0, 0], "line": {"color": "#FECB52", "width": 2}}),
        );
    }
    let layout = json!({"title": {"text": title}, "height": 340, "margin": {"t": 60}, "legend": legend_top(),
        "xaxis": {"title": {"text": "Résultat net (R)"}}, "yaxis": {"title": {"text": "Trades"}},
        "shapes": shapes, "bargap": 0.05});
    Chart { data, layout }
}

/// Histogramme de l'espérance nette des tirages placebo, avec la position de Fibonacci.
pub fn placebo_chart(title: &str, placebo: &PlaceboResult) -> Chart {
    let values: Vec<f64> = placebo
        .draws
        .iter()
        .filter_map(|d| d.expectancy_r_net)
        .collect();
    let mut data = vec![
        json!({"type": "histogram", "name": "Niveaux placebo", "x": values,
        "marker": {"color": "rgba(150,150,150,0.8)"}}),
    ];
    let mut shapes = Vec::new();
    let mut annotations = Vec::new();
    if let Some(f) = placebo.fibo_expectancy_r_net {
        shapes.push(
            json!({"type": "line", "x0": f, "x1": f, "yref": "paper", "y0": 0, "y1": 1,
            "line": {"color": "#F7931A", "width": 3}}),
        );
        annotations.push(
            json!({"x": f, "yref": "paper", "y": 1.0, "showarrow": false, "yanchor": "bottom",
            "text": format!("Fibonacci : {f:+.3} R{}", placebo.percentile.map_or(String::new(),
                |p| format!(" (rang {:.1} %)", p * 100.0)))}),
        );
        data.push(json!({"type": "scatter", "mode": "lines", "name": "Fibonacci", "x": [f, f], "y": [0, 0],
            "line": {"color": "#F7931A", "width": 3}}));
    }
    let layout = json!({"title": {"text": title}, "height": 360, "margin": {"t": 100},
        "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.12, "xanchor": "right", "x": 1},
        "xaxis": {"title": {"text": "Espérance nette par trade (R)"}}, "yaxis": {"title": {"text": "Tirages"}},
        "shapes": shapes, "annotations": annotations, "bargap": 0.05});
    Chart { data, layout }
}

/// Histogramme du budget final des tirages Monte-Carlo.
pub fn monte_carlo_chart(title: &str, histogram: &[(f64, usize)], initial: f64) -> Chart {
    let data = vec![
        json!({"type": "bar", "name": "Tirages", "x": histogram.iter().map(|h| h.0).collect::<Vec<_>>(),
        "y": histogram.iter().map(|h| h.1).collect::<Vec<_>>(), "marker": {"color": "#19d3f3"}}),
    ];
    let layout = json!({"title": {"text": title}, "height": 320, "margin": {"t": 60},
        "xaxis": {"title": {"text": "Budget final (USDT)"}}, "yaxis": {"title": {"text": "Tirages"}},
        "shapes": [{"type": "line", "x0": initial, "x1": initial, "yref": "paper", "y0": 0, "y1": 1,
            "line": {"color": "rgba(255,255,255,0.6)", "width": 1, "dash": "dot"}}]});
    Chart { data, layout }
}

/// Rangées partagées, pour réutiliser la mise en page des autres pages.
pub fn stacked_layout(heights: &[f64]) -> Value {
    rows_layout(heights, 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fibo::{analyze, FiboParams, ThresholdMode};
    use crate::validation::{PlaceboDraw, PlaceboResult};

    #[test]
    fn price_chart_shows_zigzag_confirmations_levels_and_trades() {
        let closes = [
            100.0, 110.0, 150.0, 200.0, 195.0, 188.0, 175.0, 165.0, 160.0,
        ];
        let mut prev = closes[0];
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let b = Bar {
                    time: i as i64 * 3_600_000,
                    open: prev,
                    high: prev.max(c),
                    low: prev.min(c),
                    close: c,
                    volume: 0.0,
                };
                prev = c;
                b
            })
            .collect();
        let params = FiboParams {
            threshold_mode: ThresholdMode::Percent,
            threshold_pct: 5.0,
            ..FiboParams::default()
        };
        let analysis = analyze(&bars, &params);
        assert_eq!(analysis.setups.len(), 1);
        let chart = price_chart(&bars, 0, Interval::H1, &analysis, &[], "test");
        let names: Vec<&str> = chart
            .data
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Cours",
                "Zigzag (pivots confirmés)",
                "Confirmation du pivot",
                "Niveaux de retracement",
                "Entrées",
                "Sorties"
            ]
        );
        assert_eq!(
            chart.data[1]["x"].as_array().unwrap().len(),
            2,
            "creux puis sommet"
        );
        // 4 niveaux : chacun un segment (2 points + séparateur).
        assert_eq!(chart.data[3]["y"].as_array().unwrap().len(), 4 * 3);
        assert_eq!(chart.layout["height"], 640);
    }

    #[test]
    fn small_charts_are_well_formed() {
        let budget = budget_chart("b", &[(0, 500.0), (60_000, 520.0)], 500.0);
        assert_eq!(budget.data.len(), 2);
        let hist = r_histogram("r", &[]);
        assert_eq!(hist.data.len(), 1, "sans trade : pas de ligne d'espérance");
        let placebo = PlaceboResult {
            draws: vec![PlaceboDraw {
                levels: vec![0.4],
                trades: 10,
                expectancy_r_net: Some(-0.2),
                final_budget: 1.0,
            }],
            fibo_expectancy_r_net: Some(-0.1),
            percentile: Some(0.97),
            valid_draws: 1,
            mean: Some(-0.2),
            p5: None,
            median: None,
            p95: None,
        };
        let c = placebo_chart("p", &placebo);
        assert_eq!(c.layout["shapes"].as_array().unwrap().len(), 1);
        assert!(c.layout["annotations"][0]["text"]
            .as_str()
            .unwrap()
            .contains("97.0"));
        let mc = monte_carlo_chart("m", &[(0.0, 3), (10.0, 5)], 500.0);
        assert_eq!(mc.data[0]["y"], json!([3, 5]));
        assert!(stacked_layout(&[0.5, 0.5]).get("yaxis2").is_some());
    }
}

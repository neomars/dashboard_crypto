//! Figures Plotly (JSON `{data, layout}`) des pages Trading du tableau de
//! bord. Le moteur n'en a pas besoin pour trader : seul l'affichage les utilise.

use crate::backtest::{Backtest, FillKind};
use crate::market::{fmt_time, Bar, Interval};
use crate::strategy::{TrendVariant, EMA_FAST, EMA_SLOW, SMA_FILTER};
use crate::ta::{ema, highest, lowest, previous, sma};
use serde::Serialize;
use serde_json::{json, Value};

/// Figure Plotly, même format que les figures des indicateurs du tableau de bord.
#[derive(Debug, Clone, Serialize)]
pub struct Chart {
    pub data: Vec<Value>,
    pub layout: Value,
}

/// Graphiques superposés partageant l'axe des dates : `heights` relatives, de
/// haut en bas ; seules les dates du graphique du bas sont affichées.
pub fn rows_layout(heights: &[f64], gap: f64) -> Value {
    let total: f64 = heights.iter().sum();
    let usable = 1.0 - gap * heights.len().saturating_sub(1) as f64;
    let last = heights.len().saturating_sub(1);
    let mut layout = json!({});
    let mut top = 1.0;
    for (k, h) in heights.iter().enumerate() {
        let bottom = (top - usable * h / total).max(0.0);
        let s = if k == 0 {
            String::new()
        } else {
            (k + 1).to_string()
        };
        let mut x = json!({"anchor": format!("y{s}"), "domain": [0, 1],
            "showticklabels": k == last, "rangeslider": {"visible": false}});
        if k > 0 {
            x["matches"] = json!("x");
        }
        layout[format!("xaxis{s}")] = x;
        layout[format!("yaxis{s}")] = json!({"anchor": format!("x{s}"), "domain": [bottom, top]});
        top = bottom - gap;
    }
    layout
}

fn opt(v: Option<f64>) -> Value {
    v.filter(|x| x.is_finite())
        .map_or(Value::Null, |x| json!(x))
}

/// Données affichées par la page « Signaux de tendance ».
pub struct SignalSeries<'a> {
    /// Toutes les bougies, préchauffage compris (les indicateurs en ont besoin).
    pub bars: &'a [Bar],
    /// Première bougie affichée.
    pub from: usize,
    pub interval: Interval,
    pub variant: TrendVariant,
    pub score: &'a [Option<f64>],
    /// Exposition cible à chaque clôture.
    pub exposure: &'a [f64],
    pub title: String,
}

pub fn signals_chart(s: &SignalSeries) -> Chart {
    let shown = &s.bars[s.from..];
    let x: Vec<String> = shown.iter().map(|b| fmt_time(b.time, s.interval)).collect();
    let col = |f: fn(&Bar) -> f64| shown.iter().map(f).collect::<Vec<f64>>();
    let tail = |v: Vec<Option<f64>>| v[s.from..].iter().map(|x| opt(*x)).collect::<Vec<Value>>();

    let mut layout = rows_layout(&[0.68, 0.32], 0.06);
    layout["title"] = json!({"text": s.title});
    layout["height"] = json!(820);
    layout["hovermode"] = json!("x unified");
    layout["legend"] =
        json!({"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1});
    layout["yaxis"]["title"] = json!({"text": "Prix (USDT)"});
    layout["yaxis2"]["title"] = json!({"text": "Score / exposition"});
    layout["yaxis2"]["range"] = json!([-2.1, 2.1]);
    layout["shapes"] = json!([{"type": "line", "xref": "x2 domain", "yref": "y2", "x0": 0, "x1": 1,
        "y0": 0, "y1": 0, "line": {"color": "gray", "width": 1, "dash": "dot"}}]);

    let mut data = vec![json!({"type": "candlestick", "name": "Cours", "x": x,
        "open": col(|b| b.open), "high": col(|b| b.high), "low": col(|b| b.low), "close": col(|b| b.close),
        "increasing": {"line": {"color": "#00ff88"}}, "decreasing": {"line": {"color": "#ff3366"}}})];

    let highs: Vec<f64> = s.bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = s.bars.iter().map(|b| b.low).collect();
    let closes: Vec<f64> = s.bars.iter().map(|b| b.close).collect();
    match s.variant {
        TrendVariant::Donchian => {
            data.push(json!({"type": "scatter", "mode": "lines", "name": "Plus haut 55 bougies (cassure)",
                "x": x, "y": tail(previous(&highest(&highs, 55))), "line": {"color": "rgba(100, 180, 255, 0.9)", "width": 1.2}}));
            data.push(json!({"type": "scatter", "mode": "lines", "name": "Plus bas 55 bougies",
                "x": x, "y": tail(previous(&lowest(&lows, 55))), "line": {"color": "rgba(100, 180, 255, 0.9)", "width": 1.2},
                "fill": "tonexty", "fillcolor": "rgba(100, 180, 255, 0.07)"}));
        }
        TrendVariant::Ema => {
            for (name, values, color, dash) in [
                (
                    format!("EMA {EMA_FAST}"),
                    ema(&closes, EMA_FAST),
                    "#19d3f3",
                    "solid",
                ),
                (
                    format!("EMA {EMA_SLOW}"),
                    ema(&closes, EMA_SLOW),
                    "#FFA15A",
                    "solid",
                ),
                (
                    format!("SMA {SMA_FILTER} (filtre)"),
                    sma(&closes, SMA_FILTER),
                    "#FF5555",
                    "dash",
                ),
            ] {
                data.push(json!({"type": "scatter", "mode": "lines", "name": name, "x": x, "y": tail(values),
                    "line": {"color": color, "width": 1.5, "dash": dash}}));
            }
        }
    }

    let score: Vec<Value> = s.score[s.from..].iter().map(|v| opt(*v)).collect();
    let colors: Vec<&str> = s.score[s.from..]
        .iter()
        .map(|v| match v {
            Some(x) if *x > 0.0 => "rgba(0, 255, 136, 0.55)",
            Some(x) if *x < 0.0 => "rgba(255, 51, 102, 0.55)",
            _ => "rgba(150, 150, 150, 0.4)",
        })
        .collect();
    data.push(json!({"type": "bar", "name": "Score de tendance", "x": x, "y": score, "xaxis": "x2", "yaxis": "y2",
        "marker": {"color": colors, "line": {"width": 0}}}));
    data.push(json!({"type": "scatter", "mode": "lines", "name": "Exposition cible", "x": x,
        "y": s.exposure[s.from..].to_vec(), "xaxis": "x2", "yaxis": "y2", "line": {"color": "white", "width": 1.5, "shape": "hv"}}));
    Chart { data, layout }
}

pub fn backtest_chart(bt: &Backtest) -> Chart {
    let interval = bt.params.interval;
    let x: Vec<String> = bt
        .points
        .iter()
        .map(|p| fmt_time(p.time, interval))
        .collect();
    let col =
        |f: fn(&crate::backtest::EquityPoint) -> f64| bt.points.iter().map(f).collect::<Vec<f64>>();

    let mut layout = rows_layout(&[0.55, 0.2, 0.25], 0.05);
    layout["title"] =
        json!({"text": format!("Backtest {} — {}", bt.symbol, bt.params.variant.label())});
    layout["height"] = json!(900);
    layout["hovermode"] = json!("x unified");
    layout["legend"] =
        json!({"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1});
    layout["yaxis"]["title"] = json!({"text": "Capital (USDT)"});
    layout["yaxis2"]["title"] = json!({"text": "Drawdown (%)"});
    layout["yaxis3"]["title"] = json!({"text": "Exposition (x)"});

    let mut data = vec![
        json!({"type": "scatter", "mode": "lines", "name": "Stratégie", "x": x, "y": col(|p| p.equity),
            "line": {"color": "#00FFAA", "width": 2.2}}),
        json!({"type": "scatter", "mode": "lines", "name": "Buy & Hold", "x": x, "y": col(|p| p.buy_hold),
            "line": {"color": "rgba(255, 165, 0, 0.7)", "width": 1.5, "dash": "dash"}}),
        json!({"type": "scatter", "mode": "lines", "name": "Drawdown", "x": x, "y": col(|p| p.drawdown_pct),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "#FF5555", "width": 1}, "fill": "tozeroy",
            "fillcolor": "rgba(255, 85, 85, 0.2)"}),
        json!({"type": "scatter", "mode": "lines", "name": "Exposition", "x": x, "y": col(|p| p.exposure),
            "xaxis": "x3", "yaxis": "y3", "line": {"color": "#636efa", "width": 1, "shape": "hv"}, "fill": "tozeroy",
            "fillcolor": "rgba(99, 110, 250, 0.25)"}),
    ];

    // Repères des stops, coupe-circuits et liquidation sur la courbe de capital.
    let equity_at = |t: i64| bt.points.iter().find(|p| p.time == t).map(|p| p.equity);
    for (kind, name, symbol, color) in [
        (FillKind::Stop, "Stop suiveur", "x", "#FF5555"),
        (
            FillKind::CircuitBreaker,
            "Coupe-circuit",
            "triangle-down",
            "#FECB52",
        ),
        (FillKind::Liquidation, "Liquidation", "x", "orange"),
    ] {
        let pts: Vec<(String, f64)> = bt
            .fills
            .iter()
            .filter(|f| f.kind == kind)
            .filter_map(|f| Some((fmt_time(f.time, interval), equity_at(f.time)?)))
            .collect();
        if !pts.is_empty() {
            data.push(json!({"type": "scatter", "mode": "markers", "name": name,
                "x": pts.iter().map(|p| p.0.clone()).collect::<Vec<_>>(), "y": pts.iter().map(|p| p.1).collect::<Vec<_>>(),
                "marker": {"color": color, "size": 10, "symbol": symbol, "line": {"color": "white", "width": 1}}}));
        }
    }
    Chart { data, layout }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_share_the_x_axis_and_fill_the_height() {
        let l = rows_layout(&[0.5, 0.25, 0.25], 0.05);
        assert_eq!(l["xaxis2"]["matches"], "x");
        assert_eq!(l["xaxis3"]["showticklabels"], true);
        assert_eq!(l["xaxis"]["showticklabels"], false);
        let top = l["yaxis"]["domain"][1].as_f64().unwrap();
        let bottom = l["yaxis3"]["domain"][0].as_f64().unwrap();
        assert_eq!(top, 1.0);
        assert!(bottom.abs() < 1e-9, "{bottom}");
        let y1_bottom = l["yaxis"]["domain"][0].as_f64().unwrap();
        let y2_top = l["yaxis2"]["domain"][1].as_f64().unwrap();
        assert!(
            (y1_bottom - y2_top - 0.05).abs() < 1e-9,
            "écart entre graphiques"
        );
    }
}

//! Figures Plotly (JSON `{data, layout}`) des pages Trading du tableau de
//! bord. Le moteur n'en a pas besoin pour trader : seul l'affichage les utilise.

use crate::accumulation::{Accumulation, WeightSource};
use crate::backtest::{Backtest, FillKind};
use crate::market::{fmt_time, Bar, Interval, DAY_MS};
use crate::strategy::{TrendVariant, BREAKOUT_LOOKBACK, EMA_FAST, EMA_SLOW, SMA_FILTER};
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
        TrendVariant::Breakout => {
            data.push(json!({"type": "scatter", "mode": "lines",
                "name": format!("Plus haute clôture {BREAKOUT_LOOKBACK} bougies (entrée)"),
                "x": x, "y": tail(previous(&highest(&closes, BREAKOUT_LOOKBACK))),
                "line": {"color": "rgba(100, 180, 255, 0.9)", "width": 1.2}}));
            data.push(json!({"type": "scatter", "mode": "lines",
                "name": format!("SMA {SMA_FILTER} (filtre et sortie)"),
                "x": x, "y": tail(sma(&closes, SMA_FILTER)),
                "line": {"color": "#FF5555", "width": 1.5, "dash": "dash"}}));
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

    // Le nombre de BTC passe en premier : c'est la mesure de réussite.
    let mut layout = rows_layout(&[0.3, 0.26, 0.2, 0.24], 0.045);
    layout["title"] =
        json!({"text": format!("Backtest {} — {}", bt.symbol, bt.params.variant.label())});
    layout["height"] = json!(1000);
    layout["hovermode"] = json!("x unified");
    layout["margin"] = json!({"t": 130});
    layout["title"]["y"] = json!(0.985);
    layout["title"]["yanchor"] = json!("top");
    layout["legend"] =
        json!({"orientation": "h", "yanchor": "bottom", "y": 1.01, "xanchor": "right", "x": 1});
    layout["yaxis"]["title"] = json!({"text": "Nombre de BTC"});
    layout["yaxis2"]["title"] = json!({"text": "Capital (USDT)"});
    layout["yaxis3"]["title"] = json!({"text": "Drawdown (%)"});
    layout["yaxis4"]["title"] = json!({"text": "Exposition (x)"});

    let mut data = vec![
        json!({"type": "scatter", "mode": "lines", "name": "Nombre de BTC", "x": x, "y": col(|p| p.equity_btc),
            "line": {"color": "#F7931A", "width": 2.2}}),
        json!({"type": "scatter", "mode": "lines", "name": "Garder ses BTC (×1,00)", "x": x, "y": col(|p| p.hold_btc),
            "line": {"color": "rgba(255, 255, 255, 0.7)", "width": 1.5, "dash": "dot"}}),
        json!({"type": "scatter", "mode": "lines", "name": "Stratégie (USDT)", "x": x, "y": col(|p| p.equity),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "#00FFAA", "width": 2.0}}),
        json!({"type": "scatter", "mode": "lines", "name": "Buy & Hold (USDT)", "x": x, "y": col(|p| p.buy_hold),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "rgba(255, 165, 0, 0.7)", "width": 1.5, "dash": "dash"}}),
        json!({"type": "scatter", "mode": "lines", "name": "Drawdown (USDT)", "x": x, "y": col(|p| p.drawdown_pct),
            "xaxis": "x3", "yaxis": "y3", "line": {"color": "#FF5555", "width": 1}, "fill": "tozeroy",
            "fillcolor": "rgba(255, 85, 85, 0.2)"}),
        json!({"type": "scatter", "mode": "lines", "name": "Exposition", "x": x, "y": col(|p| p.exposure),
            "xaxis": "x4", "yaxis": "y4", "line": {"color": "#636efa", "width": 1, "shape": "hv"}, "fill": "tozeroy",
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
                "xaxis": "x2", "yaxis": "y2",
                "marker": {"color": color, "size": 10, "symbol": symbol, "line": {"color": "white", "width": 1}}}));
        }
    }
    Chart { data, layout }
}

/// Figure de la page « Accumulation BTC », en trois rangées :
/// 1. prix en échelle logarithmique, achats en marqueurs de taille proportionnelle au montant ;
/// 2. BTC accumulés : DCA pondéré, DCA fixe à budget égal et au montant de base ;
/// 3. poids appliqué à chaque achat (barres) et MVRV connu (ligne, échelle de droite).
pub fn accumulation_chart(acc: &Accumulation) -> Chart {
    let x: Vec<String> = acc
        .points
        .iter()
        .map(|p| fmt_time(p.time, Interval::D1))
        .collect();
    let mut layout = rows_layout(&[0.42, 0.33, 0.25], 0.06);
    layout["title"] = json!({"text": format!(
        "Accumulation BTC — {} (montant de base {:.0} USDT)",
        acc.params.weighting.label(),
        acc.params.base_amount
    )});
    layout["height"] = json!(950);
    layout["hovermode"] = json!("x unified");
    layout["margin"] = json!({"t": 130});
    layout["title"]["y"] = json!(0.985);
    layout["title"]["yanchor"] = json!("top");
    layout["legend"] =
        json!({"orientation": "h", "yanchor": "bottom", "y": 1.01, "xanchor": "right", "x": 1});
    layout["yaxis"]["title"] = json!({"text": "Prix BTC (USDT)"});
    layout["yaxis"]["type"] = json!("log");
    layout["yaxis2"]["title"] = json!({"text": "BTC accumulés"});
    layout["yaxis3"]["title"] = json!({"text": "Poids (×)"});
    layout["yaxis4"] = json!({"title": {"text": "MVRV"}, "overlaying": "y3", "side": "right",
        "anchor": "x3", "showgrid": false});

    // Taille des marqueurs : de 6 à 26 px selon le montant, relativement au plus gros achat.
    let max_amount = acc.purchases.iter().map(|p| p.amount).fold(0.0, f64::max);
    let sizes: Vec<f64> = acc
        .purchases
        .iter()
        .map(|p| {
            if max_amount > 0.0 {
                3.0 + 7.0 * p.amount / max_amount
            } else {
                3.0
            }
        })
        .collect();
    let dates: Vec<&str> = acc.purchases.iter().map(|p| p.date.as_str()).collect();
    let hover: Vec<String> = acc
        .purchases
        .iter()
        .map(|p| {
            format!(
                "{} : {:.2} USDT (×{:.2}, {})<br>{:.6} BTC à {:.0}{}",
                p.date,
                p.amount,
                p.weight,
                match p.source {
                    WeightSource::Fixed => "fixe",
                    WeightSource::Mvrv => "MVRV",
                    WeightSource::PriceMa200 => "prix / moyenne 200 j",
                    WeightSource::PriceMa200Fallback => "prix / moyenne 200 j, secours",
                    WeightSource::NoData => "sans donnée",
                },
                p.btc,
                p.price,
                p.mvrv.map_or(String::new(), |m| format!(" · MVRV {m:.2}"))
            )
        })
        .collect();
    let col = |f: fn(&crate::accumulation::AccPoint) -> f64| {
        acc.points.iter().map(f).collect::<Vec<f64>>()
    };
    let colors: Vec<&str> = acc
        .purchases
        .iter()
        .map(|p| match p.source {
            WeightSource::Mvrv => "#F7931A",
            WeightSource::PriceMa200 => "#19d3f3",
            WeightSource::PriceMa200Fallback => "#ab63fa",
            WeightSource::Fixed | WeightSource::NoData => "#9aa0a6",
        })
        .collect();

    // Largeur des barres de poids : 80 % de l'intervalle le plus court entre deux achats.
    let bar_width = acc
        .purchases
        .windows(2)
        .map(|w| w[1].time - w[0].time)
        .filter(|g| *g > 0)
        .min()
        .map_or(DAY_MS as f64, |g| g as f64)
        * 0.8;
    let mut data = vec![
        json!({"type": "scatter", "mode": "lines", "name": "Prix BTC/USDT (clôture)", "x": x, "y": col(|p| p.close),
            "line": {"color": "rgba(200, 200, 200, 0.8)", "width": 1.2}}),
        json!({"type": "scatter", "mode": "markers", "name": "Achats (taille = montant)", "x": dates,
            "y": acc.purchases.iter().map(|p| p.price).collect::<Vec<f64>>(),
            "text": hover, "hoverinfo": "text",
            "marker": {"color": "#F7931A", "size": sizes, "opacity": 0.8, "line": {"color": "white", "width": 0.4}}}),
        json!({"type": "scatter", "mode": "lines", "name": "BTC — DCA pondéré", "x": x, "y": col(|p| p.btc),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "#F7931A", "width": 2.2}}),
        json!({"type": "scatter", "mode": "lines", "name": "BTC — DCA fixe, même budget", "x": x, "y": col(|p| p.btc_equal_budget),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "rgba(255, 255, 255, 0.8)", "width": 1.5, "dash": "dash"}}),
        json!({"type": "scatter", "mode": "lines", "name": "BTC — DCA fixe, montant de base", "x": x, "y": col(|p| p.btc_fixed_base),
            "xaxis": "x2", "yaxis": "y2", "line": {"color": "rgba(150, 150, 150, 0.8)", "width": 1.2, "dash": "dot"}}),
        json!({"type": "bar", "name": "Poids de l'achat", "x": dates,
            "y": acc.purchases.iter().map(|p| p.weight).collect::<Vec<f64>>(),
            "xaxis": "x3", "yaxis": "y3", "width": bar_width, "marker": {"color": colors}}),
    ];
    if acc.points.iter().any(|p| p.mvrv.is_some()) {
        data.push(
            json!({"type": "scatter", "mode": "lines", "name": "MVRV", "x": x,
            "y": acc.points.iter().map(|p| opt(p.mvrv)).collect::<Vec<Value>>(),
            "xaxis": "x3", "yaxis": "y4", "connectgaps": false,
            "line": {"color": "#00cc96", "width": 1.3}}),
        );
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

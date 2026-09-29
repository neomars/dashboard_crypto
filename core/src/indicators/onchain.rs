use super::IndicatorOutput;
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use crate::series::{opt_json, rolling_mean};
use serde_json::{json, Value};

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data.ticker_history("BTC-USD").await?;
    let x = dates(btc.iter().map(|c| c.date));
    let price: Vec<f64> = btc.iter().map(|c| c.close).collect();
    let sma = |w: usize, k: f64| -> Vec<Value> {
        rolling_mean(&price, w)
            .into_iter()
            .map(|v| opt_json(v.map(|v| v * k)))
            .collect()
    };

    let mut fig = Figure::new(json!({
        "title": {"text": "On-chain Top & Bottom Indicators"},
        "xaxis": {"title": {"text": "Date"}}, "yaxis": {"title": {"text": "BTC Price (USD)"}},
        "height": 800, "hovermode": "x unified",
        "legend": {"x": 0.01, "y": 0.99, "bgcolor": "rgba(0,0,0,0.6)"}
    }));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "BTC Price", "x": x, "y": price, "line": {"color": "white", "width": 2}}));
    let lines: [(&str, usize, f64, &str, &str); 5] = [
        ("200-week SMA", 1400, 1.0, "#FF3333", "solid"),
        ("Realized Price (730d SMA)", 730, 1.0, "#FFAA33", "solid"),
        ("Pi Cycle Top (350d × 2)", 350, 2.0, "#33FF99", "solid"),
        ("111d SMA × 2", 111, 2.0, "#33AAFF", "solid"),
        ("2-year SMA", 730, 1.0, "#3366FF", "dot"),
    ];
    for (name, window, k, color, dash) in lines {
        fig.trace(
            json!({"type": "scatter", "mode": "lines", "name": name, "x": x, "y": sma(window, k),
            "line": {"color": color, "width": 1.5, "dash": dash}}),
        );
    }
    Ok(fig.into())
}

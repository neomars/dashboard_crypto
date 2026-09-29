use super::IndicatorOutput;
use crate::data::DataProvider;
use crate::figure::{dates, merge, two_rows, Figure};
use serde_json::json;

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data.ticker_history("BTC-USD").await?;
    let x = dates(btc.iter().map(|c| c.date));
    let col = |f: fn(&crate::series::Candle) -> f64| btc.iter().map(f).collect::<Vec<f64>>();
    let colors: Vec<&str> = btc
        .iter()
        .map(|c| if c.close >= c.open { "green" } else { "red" })
        .collect();

    let mut layout = two_rows([0.7, 0.3], 0.08, Some(["BTC Price", "Volume"]));
    merge(
        &mut layout,
        json!({
            "title": {"text": "Bitcoin - Price + Volume (jours positifs/negatifs)"},
            "height": 850, "hovermode": "x unified", "legend": {"x": 0.01, "y": 0.99},
            "xaxis": {"rangeslider": {"visible": false}},
            "xaxis2": {"title": {"text": "Date"}},
            "yaxis": {"title": {"text": "BTC Price (USD)"}}, "yaxis2": {"title": {"text": "Volume"}}
        }),
    );
    let mut fig = Figure::new(layout);
    fig.trace(json!({
        "type": "candlestick", "name": "BTC Price", "x": x,
        "open": col(|c| c.open), "high": col(|c| c.high), "low": col(|c| c.low), "close": col(|c| c.close),
        "increasing": {"line": {"color": "#00FF88"}}, "decreasing": {"line": {"color": "#FF3333"}}
    }));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Close Price", "x": x, "y": col(|c| c.close),
        "line": {"color": "white", "width": 1.5}, "opacity": 0.4}));
    fig.trace(json!({"type": "bar", "name": "Volume", "x": x, "y": col(|c| c.volume), "xaxis": "x2", "yaxis": "y2",
        "marker": {"color": colors, "line": {"width": 0}}}));
    Ok(fig.into())
}

use super::{param_u64, IndicatorOutput, Metric};
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use crate::series::{ewm_mean, rolling_mean_min_periods, weekly_candles, Candle};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Regime {
    Bull,
    Bear,
}

impl Regime {
    pub fn label(self) -> &'static str {
        match self {
            Regime::Bull => "Bull Market",
            Regime::Bear => "Bear Market",
        }
    }
}

/// Bear Market Support Band (Benjamin Cowen) : SMA et EMA hebdomadaires, et
/// régime de marché (haussier si la clôture dépasse la SMA).
pub fn calculate_bmsb(
    closes: &[f64],
    sma_len: usize,
    ema_len: usize,
) -> (Vec<f64>, Vec<f64>, Vec<Regime>) {
    let sma: Vec<f64> = rolling_mean_min_periods(closes, sma_len, 1)
        .into_iter()
        .map(|v| v.unwrap_or(f64::NAN))
        .collect();
    let ema = ewm_mean(closes, ema_len);
    let regime = closes
        .iter()
        .zip(&sma)
        .map(|(c, s)| if c > s { Regime::Bull } else { Regime::Bear })
        .collect();
    (sma, ema, regime)
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let sma_len = param_u64(params, "sma", 20, 10, 50) as usize;
    let ema_len = param_u64(params, "ema", 21, 10, 50) as usize;
    let daily = data
        .ticker_history("BTC-USD")
        .await
        .map_err(|e| format!("Impossible de récupérer les données Yahoo Finance. {e}"))?;
    let weekly: Vec<Candle> = weekly_candles(&daily)
        .into_iter()
        .filter(|c| c.close > 0.0)
        .collect();
    let closes: Vec<f64> = weekly.iter().map(|c| c.close).collect();
    let (sma, ema, regime) = calculate_bmsb(&closes, sma_len, ema_len);
    let current = *regime.last().ok_or("Aucune donnée hebdomadaire.")?;

    let x = dates(weekly.iter().map(|c| c.date));
    let col = |f: fn(&Candle) -> f64| weekly.iter().map(f).collect::<Vec<f64>>();
    let mut fig = Figure::new(json!({
        "height": 700, "hovermode": "x unified",
        "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.07, "xanchor": "right", "x": 1},
        "xaxis": {"title": {"text": "Date"}, "rangeslider": {"visible": false}},
        "yaxis": {"title": {"text": "Prix BTC (USD)"}},
        "margin": {"t": 90},
        "annotations": [{"text": "Bitcoin - Bear Market Support Band (Benjamin Cowen)", "x": 0.5, "y": 1, "xref": "paper", "yref": "paper",
            "xanchor": "center", "yanchor": "bottom", "showarrow": false, "font": {"size": 16}}]
    }));
    fig.trace(json!({"type": "candlestick", "name": "BTC Price", "x": x, "open": col(|c| c.open), "high": col(|c| c.high),
        "low": col(|c| c.low), "close": col(|c| c.close),
        "increasing": {"line": {"color": "#00ff88"}}, "decreasing": {"line": {"color": "#ff3366"}}}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("{sma_len}-week SMA (Support)"), "x": x, "y": sma, "line": {"color": "#00ff00", "width": 2.5}}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("{ema_len}-week EMA (Resistance)"), "x": x, "y": ema, "line": {"color": "#ff0000", "width": 2.5}}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "x": x, "y": sma, "line": {"color": "rgba(0,0,0,0)"}, "showlegend": false, "hoverinfo": "skip"}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Zone Bear Market", "x": x, "y": ema, "fill": "tonexty",
        "line": {"color": "rgba(0,0,0,0)"}, "fillcolor": "rgba(255, 100, 100, 0.15)", "hoverinfo": "skip"}));

    let icon = if current == Regime::Bull {
        "🟢"
    } else {
        "🔴"
    };
    let mut out = IndicatorOutput::from(fig);
    out.metrics.push(Metric {
        label: "Régime actuel".into(),
        value: format!("{icon} {}", current.label()),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bull_market_when_price_above_sma() {
        let closes: Vec<f64> = (0..25).map(|i| 100.0 + i as f64).collect();
        assert_eq!(
            *calculate_bmsb(&closes, 20, 21).2.last().unwrap(),
            Regime::Bull
        );
    }

    #[test]
    fn bear_market_when_price_below_sma() {
        let closes: Vec<f64> = (0..25).map(|i| 200.0 - 5.0 * i as f64).collect();
        assert_eq!(
            *calculate_bmsb(&closes, 20, 21).2.last().unwrap(),
            Regime::Bear
        );
    }

    #[test]
    fn sma_uses_partial_window_at_start() {
        let (sma, ema, _) = calculate_bmsb(&[10.0, 20.0], 20, 21);
        assert_eq!(sma, vec![10.0, 15.0]);
        assert_eq!(ema[0], 10.0);
    }
}

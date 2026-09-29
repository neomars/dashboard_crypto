//! Positions Long/Short des traders sur les contrats perpétuels OKX
//! (`<PAIRE>-USDT-SWAP`) : ratio de comptes nets long / nets short et open
//! interest, depuis l'API publique d'OKX (sans clé).

use super::{param_str, IndicatorOutput};
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use crate::series::opt_json;
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PAIRS: [&str; 6] = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"];

/// Part des comptes nets long et nets short (%) à partir du ratio long/short.
pub(crate) fn account_shares(ratio: f64) -> (f64, f64) {
    let long = ratio / (1.0 + ratio) * 100.0;
    (long, 100.0 - long)
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let pair = param_str(params, "pair", "BTC").to_uppercase();
    if !PAIRS.contains(&pair.as_str()) {
        return Err(format!("Paire inconnue : {pair}"));
    }
    let mode = param_str(params, "mode", "Long vs Short");

    let mut fig = Figure::new(json!({
        "height": 650, "hovermode": "x unified",
        "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1},
        "yaxis2": {"title": {"text": format!("Prix {pair} (USD)")}, "overlaying": "y", "side": "right", "showgrid": false},
        "margin": {"t": 80, "b": 60}
    }));

    let (x, title) = if mode == "Open Interest" {
        // [contrats, actif de base, USD]
        let oi = data.okx_history("open-interest-history", &pair).await?;
        let x = dates(oi.iter().map(|p| p.0));
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Open interest (USD)", "x": x,
            "y": oi.iter().map(|p| opt_json(p.1.get(2).copied())).collect::<Vec<_>>(),
            "line": {"color": "#FFD600", "width": 2}, "fill": "tozeroy", "fillcolor": "rgba(255,214,0,0.1)"}));
        fig.layout["yaxis"] = json!({"title": {"text": "Open interest (USD)"}});
        (
            oi.iter().map(|p| p.0).collect::<Vec<NaiveDate>>(),
            format!("{pair} - Open interest des perpétuels OKX"),
        )
    } else {
        let ratio = data
            .okx_history("long-short-account-ratio-contract", &pair)
            .await?;
        let x = dates(ratio.iter().map(|p| p.0));
        let values: Vec<Option<f64>> = ratio
            .iter()
            .map(|p| p.1.first().copied().filter(|v| v.is_finite()))
            .collect();
        if mode == "Ratio Long/Short" {
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Ratio Long/Short (comptes)", "x": x,
                "y": values.iter().map(|v| opt_json(*v)).collect::<Vec<_>>(), "line": {"color": "#FFD600", "width": 2.5}}));
            fig.hline(1.0, "white", "dash", "y", "paper");
            fig.annotation(json!({"xref": "paper", "yref": "y", "x": 1, "y": 1.0, "xanchor": "right", "yanchor": "bottom", "text": "Équilibre", "showarrow": false}));
            fig.layout["yaxis"] = json!({"title": {"text": "Comptes long / comptes short"}});
        } else {
            let shares: Vec<Option<(f64, f64)>> =
                values.iter().map(|v| v.map(account_shares)).collect();
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Comptes long (%)", "x": x,
                "y": shares.iter().map(|s| opt_json(s.map(|s| s.0))).collect::<Vec<_>>(), "line": {"color": "#00C853", "width": 2.5}}));
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Comptes short (%)", "x": x,
                "y": shares.iter().map(|s| opt_json(s.map(|s| s.1))).collect::<Vec<_>>(), "line": {"color": "#FF1744", "width": 2.5}}));
            fig.layout["yaxis"] = json!({"title": {"text": "Part des comptes (%)"}});
        }
        (
            ratio.iter().map(|p| p.0).collect::<Vec<NaiveDate>>(),
            format!("{pair} - Positions Long/Short des traders (perpétuels OKX)"),
        )
    };
    fig.layout["title"] = json!({"text": title});

    // Prix de l'actif superposé (dernier prix connu à chaque date)
    if let Some(start) = x.first() {
        if let Ok(prices) = data
            .ticker_range(&format!("{pair}-USD"), Some(*start), None)
            .await
        {
            let by_day: BTreeMap<NaiveDate, f64> =
                prices.iter().map(|c| (c.date, c.close)).collect();
            let y: Vec<Value> = x
                .iter()
                .map(|d| opt_json(by_day.range(..=d).next_back().map(|(_, v)| *v)))
                .collect();
            fig.trace(json!({"type": "scatter", "mode": "lines", "name": format!("Prix {pair} (USD)"), "x": dates(x.iter().copied()),
                "y": y, "yaxis": "y2", "line": {"color": "#00CCFF", "width": 1.5, "dash": "dot"}}));
        }
    }
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::account_shares;

    #[test]
    fn shares_from_ratio() {
        assert_eq!(account_shares(1.0), (50.0, 50.0));
        let (l, s) = account_shares(3.0);
        assert!((l - 75.0).abs() < 1e-9 && (s - 25.0).abs() < 1e-9);
    }
}

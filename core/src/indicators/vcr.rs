use super::IndicatorOutput;
use crate::data::{parse_ymd, today, DataProvider};
use crate::figure::{dates, merge, two_rows, Figure};
use crate::series::{fmt_date, opt_json, rolling_std};
use chrono::Duration;
use serde_json::{json, Value};

/// Volatilité annualisée (%) des rendements sur une fenêtre glissante.
pub fn annualized_vol(returns: &[f64], window: usize) -> Vec<Option<f64>> {
    rolling_std(returns, window)
        .into_iter()
        .map(|s| s.map(|s| s * 365f64.sqrt() * 100.0))
        .collect()
}

/// Ratio de compression de volatilité 30 j / 365 j.
pub(crate) fn vcr(closes: &[f64]) -> Vec<Option<f64>> {
    let mut returns = vec![f64::NAN];
    returns.extend(closes.windows(2).map(|w| (w[1] / w[0]).ln()));
    let v30 = annualized_vol(&returns, 30);
    let v365 = annualized_vol(&returns, 365);
    v30.iter()
        .zip(&v365)
        .map(|(a, b)| match (a, b) {
            (Some(a), Some(b)) if *b != 0.0 => Some(a / b),
            _ => None,
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data
        .ticker_range("BTC-USD", Some(parse_ymd("2018-01-01")), None)
        .await?;
    let x = dates(btc.iter().map(|c| c.date));
    let closes: Vec<f64> = btc.iter().map(|c| c.close).collect();
    let ratio: Vec<Value> = vcr(&closes).into_iter().map(opt_json).collect();

    let now = today();
    let ago = |days: i64| fmt_date(now - Duration::days(days));
    let end = fmt_date(now + Duration::days(30));
    let mut layout = two_rows([0.65, 0.35], 0.1, None);
    merge(
        &mut layout,
        json!({
            "xaxis2": {"title": {"text": "Date"}},
            "yaxis": {"title": {"text": "BTC Price (USD)"}, "type": "log"},
            "yaxis2": {"title": {"text": "VCR (30j / 365j)"}},
            "height": 800, "hovermode": "x unified",
            "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1},
            "margin": {"t": 150, "b": 50},
            "updatemenus": [{"type": "buttons", "direction": "right", "x": 0.0, "y": 1.2, "showactive": true,
                "bgcolor": "#9aa4b5", "bordercolor": "#506784", "font": {"color": "#111111"}, "buttons": [
                {"label": "Tout", "method": "relayout", "args": [{"xaxis.autorange": true, "xaxis2.autorange": true}]},
                {"label": "3 ans", "method": "relayout", "args": [{"xaxis2.range": [ago(1095), end]}]},
                {"label": "2 ans", "method": "relayout", "args": [{"xaxis2.range": [ago(730), end]}]},
                {"label": "1 an", "method": "relayout", "args": [{"xaxis2.range": [ago(365), end]}]}
            ]}]
        }),
    );
    let mut fig = Figure::new(layout);
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x, "y": closes, "line": {"color": "white", "width": 2}}));
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "VCR 30/365", "x": x, "y": ratio, "xaxis": "x2", "yaxis": "y2",
        "line": {"color": "#00FFAA", "width": 3}}));

    for (y0, y1, color, text) in [
        (0.0, 0.55, "red", "Très forte probabilité de fort mouvement"),
        (0.55, 0.75, "orange", "Forte probabilité d'expansion"),
    ] {
        fig.shape(json!({"type": "rect", "xref": "x2 domain", "yref": "y2", "x0": 0, "x1": 1, "y0": y0, "y1": y1,
            "fillcolor": color, "opacity": 0.15, "line": {"width": 0}, "layer": "below"}));
        fig.annotation(json!({"xref": "x2 domain", "yref": "y2", "x": 0, "y": y1, "xanchor": "left", "yanchor": "top",
            "text": text, "showarrow": false, "font": {"size": 10, "color": color}}));
    }
    fig.hline(1.0, "gray", "dash", "y2", "x2 domain");
    fig.hline(0.6, "red", "dot", "y2", "x2 domain");
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vol_is_zero_for_constant_series() {
        let r = annualized_vol(&[0.0; 40], 30);
        assert!(r.iter().flatten().all(|v| *v == 0.0));
        assert_eq!(r.iter().flatten().count(), 11);
    }

    #[test]
    fn vol_scales_with_dispersion() {
        let wave = |amp: f64| {
            (0..400)
                .map(|i| if i % 2 == 0 { amp } else { -amp })
                .collect::<Vec<_>>()
        };
        let mean = |v: Vec<Option<f64>>| {
            let f: Vec<f64> = v.into_iter().flatten().collect();
            f.iter().sum::<f64>() / f.len() as f64
        };
        assert!(mean(annualized_vol(&wave(0.05), 30)) > mean(annualized_vol(&wave(0.001), 30)));
    }

    #[test]
    fn vcr_needs_a_full_year() {
        let closes: Vec<f64> = (0..400)
            .map(|i| 100.0 * (1.0 + 0.01 * ((i % 7) as f64 - 3.0)))
            .collect();
        let r = vcr(&closes);
        assert!(r[364].is_none());
        assert!(r[365].is_some());
    }
}

use super::{IndicatorOutput, Level};
use crate::bgeometrics;
use crate::data::DataProvider;
use crate::figure::{dates, Figure};
use serde_json::json;

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let mut notices = Vec::new();
    let mut series = Vec::new();
    for (key, name, color) in [
        ("sth_sopr", "STH-SOPR", "#00FFAA"),
        ("lth_sopr", "LTH-SOPR", "#FF00AA"),
    ] {
        match data.bgeometrics(key).await {
            Ok(s) => {
                notices.extend(s.stale_notice());
                series.push((name, color, bgeometrics::single_series(&s.rows)?.1));
            }
            // Le graphique reste utile avec une seule des deux séries.
            Err(e) if !series.is_empty() => notices.push(format!("{name} indisponible : {e}")),
            Err(e) => return Err(e),
        }
    }
    let start = series
        .iter()
        .filter_map(|s| s.2.first())
        .map(|p| p.0)
        .min()
        .ok_or("Aucune donnée SOPR.")?;
    let btc = data
        .ticker_range("BTC-USD", Some(start), None)
        .await
        .map_err(|e| format!("Impossible de récupérer les prix BTC. {e}"))?;

    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin - SOPR des détenteurs court terme (STH) et long terme (LTH)"},
        "xaxis": {"title": {"text": "Date"}},
        "yaxis": {"title": {"text": "SOPR"}, "type": "log", "showgrid": true, "gridcolor": "rgba(128, 128, 128, 0.2)"},
        "yaxis2": {"title": {"text": "Prix BTC (USD)"}, "type": "log", "overlaying": "y", "side": "right", "showgrid": false},
        "height": 700, "hovermode": "x unified", "legend": {"x": 0.01, "y": 0.99}
    }));
    for (name, color, points) in &series {
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": name, "x": dates(points.iter().map(|p| p.0)),
            "y": points.iter().map(|p| p.1).collect::<Vec<_>>(), "line": {"color": color, "width": 2}}));
    }
    fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "yaxis": "y2",
        "x": dates(btc.iter().map(|c| c.date)), "y": btc.iter().map(|c| c.close).collect::<Vec<_>>(),
        "line": {"color": "rgba(255, 255, 255, 0.4)", "width": 1.5}}));
    fig.hline(1.0, "orange", "dash", "y", "paper");

    let mut out = IndicatorOutput::from(fig);
    for n in notices {
        out = out.with_notice(Level::Warning, n);
    }
    Ok(out)
}

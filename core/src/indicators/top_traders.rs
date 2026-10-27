//! Ratio long/short des « top traders » de Binance Futures (les 20 % de comptes
//! ayant le plus de marge) sur le contrat perpétuel de la monnaie choisie : une
//! courbe, comme le panneau « Meilleurs traders longs/courts (Comptes) » de CoinGlass.

use super::{param_str, IndicatorOutput, Level, Metric};
use crate::binance::{self, TopTraderPoint};
use crate::data::DataProvider;
use crate::figure::Figure;
use chrono::DateTime;
use serde_json::{json, Value};

/// Horodatage Binance (millisecondes, UTC) selon le format `strftime` donné.
fn time(ts: i64, format: &str) -> String {
    DateTime::from_timestamp_millis(ts)
        .map(|t| t.format(format).to_string())
        .unwrap_or_default()
}

/// Part (0 à 1) en pourcentage à la française : `0.7042` → `70,4 %`.
fn percent(share: f64) -> String {
    format!("{:.1} %", share * 100.0).replace('.', ",")
}

pub(crate) fn build_figure(points: &[TopTraderPoint], symbol: &str, period: &str) -> Figure {
    let mut fig = Figure::new(json!({
        "title": {"text": format!("{symbol} - Top traders Binance : ratio Long/Short des comptes ({period})")},
        "height": 600, "hovermode": "x unified",
        "xaxis": {"title": {"text": "Date (UTC)"}, "hoverformat": "%d/%m/%Y %H:%M UTC"},
        "yaxis": {"title": {"text": "Comptes long / comptes short"}},
        "margin": {"t": 80, "b": 60}
    }));
    fig.trace(json!({
        "type": "scatter", "mode": "lines", "name": "Ratio Long/Short (top traders)",
        "x": points.iter().map(|p| time(p.ts, "%Y-%m-%d %H:%M")).collect::<Vec<_>>(),
        "y": points.iter().map(|p| p.ratio).collect::<Vec<_>>(),
        "customdata": points.iter().map(|p| [p.long * 100.0, p.short * 100.0]).collect::<Vec<_>>(),
        "hovertemplate": "Ratio : %{y:.4f}<br>Comptes long : %{customdata[0]:.1f} %<br>Comptes short : %{customdata[1]:.1f} %<extra></extra>",
        "line": {"color": "#26A69A", "width": 2}
    }));
    fig.hline(1.0, "white", "dash", "y", "paper");
    fig.annotation(json!({"xref": "paper", "yref": "y", "x": 1, "y": 1.0, "xanchor": "right", "yanchor": "bottom", "text": "Équilibre", "showarrow": false}));
    fig
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let period = param_str(params, "period", "4h");
    let period_label =
        binance::period_label(period).ok_or_else(|| format!("Période inconnue : {period}"))?;
    let symbol = binance::symbol(param_str(params, "coin", "BTC"))?;

    let points = data.binance_top_traders(&symbol, period).await?;
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Err(format!("Aucune donnée Binance pour {symbol}."));
    };

    let mut out = IndicatorOutput::from(build_figure(&points, &symbol, period_label));
    out.metrics = vec![
        Metric {
            label: "Ratio Long/Short actuel".into(),
            value: format!("{:.4}", last.ratio).replace('.', ","),
        },
        Metric {
            label: "Comptes long".into(),
            value: percent(last.long),
        },
        Metric {
            label: "Comptes short".into(),
            value: percent(last.short),
        },
    ];
    Ok(out.with_notice(
        Level::Info,
        format!(
            "Binance ne publie que les {} derniers jours pour ce ratio : {} relevés (période de {period_label}), du {} au {} UTC.",
            binance::HISTORY_DAYS,
            points.len(),
            time(first.ts, "%d/%m/%Y %H:%M"),
            time(last.ts, "%d/%m/%Y %H:%M"),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: i64, ratio: f64) -> TopTraderPoint {
        TopTraderPoint {
            ts,
            ratio,
            long: ratio / (1.0 + ratio),
            short: 1.0 / (1.0 + ratio),
        }
    }

    #[test]
    fn percent_uses_a_decimal_comma() {
        assert_eq!(percent(0.7042), "70,4 %");
        assert_eq!(percent(0.5), "50,0 %");
    }

    #[test]
    fn timestamps_are_shown_in_utc() {
        // 2026-10-27 08:00:00 UTC
        assert_eq!(
            time(1_793_088_000_000, "%Y-%m-%d %H:%M"),
            "2026-10-27 08:00"
        );
        assert_eq!(
            time(1_793_088_000_000, "%d/%m/%Y %H:%M"),
            "27/10/2026 08:00"
        );
    }

    #[test]
    fn figure_is_one_ratio_line_around_the_balance_line() {
        let points = [point(1_793_088_000_000, 3.0), point(1_793_102_400_000, 0.8)];
        let fig = serde_json::to_value(build_figure(&points, "ETHUSDT", "4 heures")).unwrap();

        let title = fig["layout"]["title"]["text"].as_str().unwrap();
        assert!(
            title.contains("ETHUSDT") && title.contains("4 heures"),
            "{title}"
        );

        // L'infobulle donne l'heure : Plotly n'affiche sinon que le jour, même en période de 4 h.
        assert_eq!(fig["layout"]["xaxis"]["hoverformat"], "%d/%m/%Y %H:%M UTC");

        assert_eq!(fig["data"].as_array().unwrap().len(), 1);
        let line = &fig["data"][0];
        assert_eq!(line["x"], json!(["2026-10-27 08:00", "2026-10-27 12:00"]));
        assert_eq!(line["y"], json!([3.0, 0.8]));
        // Infobulle : parts de comptes en pourcentage, alignées sur les points.
        assert_eq!(line["customdata"][0], json!([75.0, 25.0]));

        let balance = &fig["layout"]["shapes"][0];
        assert_eq!(balance["y0"], json!(1.0));
        assert_eq!(balance["y1"], json!(1.0));
    }
}

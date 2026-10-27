//! Cours de la monnaie choisie (chandeliers du contrat perpétuel Binance Futures) et, en
//! dessous, ratio long/short des « top traders » (les 20 % de comptes ayant le plus de marge),
//! comme sur CoinGlass (panneau « Meilleurs traders longs/courts (Comptes) »).

use super::{param_str, IndicatorOutput, Level, Metric};
use crate::binance::{self, Kline, TopTraderPoint};
use crate::data::DataProvider;
use crate::figure::{merge, two_rows_shared_x, Figure};
use chrono::DateTime;
use serde_json::{json, Value};

/// Format des dates de l'axe (Plotly lit `AAAA-MM-JJ HH:MM`).
const AXIS_TIME: &str = "%Y-%m-%d %H:%M";
/// Format de l'en-tête de l'infobulle : Plotly n'y affiche sinon que le jour, même en 4 h.
const HOVER_TIME: &str = "%d/%m/%Y %H:%M UTC";
/// Format des prix de l'infobulle (d3) : 6 chiffres significatifs, zéros inutiles retirés
/// (`80 523,1` comme `0,3521`, là où Plotly écrirait `80.52k`).
const PRICE_HOVER: &str = ",.6~g";

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

/// Bougies à partir de `first_ts` (les bougies demandées commencent un peu avant le premier relevé).
fn candles_from(klines: &[Kline], first_ts: i64) -> &[Kline] {
    &klines[klines.partition_point(|k| k.ts < first_ts)..]
}

/// Figure : le cours (chandeliers) au-dessus du ratio quand `candles` n'est pas vide, le ratio
/// seul sinon.
pub(crate) fn build_figure(
    points: &[TopTraderPoint],
    candles: &[Kline],
    symbol: &str,
    period: &str,
) -> Figure {
    // Le ratio est le panneau du bas (`y2`) sous le cours, ou le seul graphique (`y`).
    let ratio_axis = if candles.is_empty() { "y" } else { "y2" };
    let mut layout = if candles.is_empty() {
        json!({
            "title": {"text": format!("{symbol} - Top traders Binance : ratio Long/Short des comptes ({period})")},
            "height": 600,
            "xaxis": {"title": {"text": "Date (UTC)"}, "hoverformat": HOVER_TIME},
            "yaxis": {"title": {"text": "Comptes long / comptes short"}},
            "margin": {"t": 80, "b": 60}
        })
    } else {
        // Un seul axe des dates pour les deux panneaux : l'infobulle unifiée les réunit.
        let price_title = format!("Cours {symbol}");
        let mut layout = two_rows_shared_x(
            [0.68, 0.32],
            0.08,
            Some([&price_title, "Ratio Long/Short des comptes des top traders"]),
        );
        merge(
            &mut layout,
            json!({
                "title": {"text": format!("{symbol} - Cours et top traders Binance ({period})")},
                "height": 850, "showlegend": false,
                "xaxis": {"title": {"text": "Date (UTC)"}, "rangeslider": {"visible": false}, "hoverformat": HOVER_TIME},
                "yaxis": {"title": {"text": "Cours (USDT)"}, "hoverformat": PRICE_HOVER},
                "yaxis2": {"title": {"text": "Comptes long / comptes short"}}
            }),
        );
        layout
    };
    merge(&mut layout, json!({"hovermode": "x unified"}));

    let mut fig = Figure::new(layout);
    if !candles.is_empty() {
        let col = |f: fn(&Kline) -> f64| candles.iter().map(f).collect::<Vec<f64>>();
        fig.trace(json!({
            "type": "candlestick", "name": "Cours",
            "x": candles.iter().map(|k| time(k.ts, AXIS_TIME)).collect::<Vec<_>>(),
            "open": col(|k| k.open), "high": col(|k| k.high), "low": col(|k| k.low), "close": col(|k| k.close),
            "increasing": {"line": {"color": "#00FF88"}}, "decreasing": {"line": {"color": "#FF3333"}}
        }));
    }
    fig.trace(json!({
        "type": "scatter", "mode": "lines", "name": "Ratio Long/Short (top traders)",
        "x": points.iter().map(|p| time(p.ts, AXIS_TIME)).collect::<Vec<_>>(),
        "y": points.iter().map(|p| p.ratio).collect::<Vec<_>>(),
        "customdata": points.iter().map(|p| [p.long * 100.0, p.short * 100.0]).collect::<Vec<_>>(),
        "hovertemplate": "Ratio : %{y:.4f}<br>Comptes long : %{customdata[0]:.1f} %<br>Comptes short : %{customdata[1]:.1f} %<extra></extra>",
        "line": {"color": "#26A69A", "width": 2}, "yaxis": ratio_axis
    }));
    fig.hline(1.0, "white", "dash", ratio_axis, "paper");
    fig.annotation(json!({"xref": "paper", "yref": ratio_axis, "x": 1, "y": 1.0, "xanchor": "right", "yanchor": "bottom", "text": "Équilibre", "showarrow": false}));
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

    // Le cours est la moitié de l'indicateur, mais s'il manque, le ratio reste affiché seul.
    let klines = data.binance_klines(&symbol, period).await;
    let candles = klines
        .as_deref()
        .map_or(&[][..], |k| candles_from(k, first.ts));

    let mut out = IndicatorOutput::from(build_figure(&points, candles, &symbol, period_label));
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
    out = out.with_notice(
        Level::Info,
        format!(
            "Binance ne publie que les {} derniers jours pour ce ratio : {} relevés (période de {period_label}), du {} au {} UTC.",
            binance::HISTORY_DAYS,
            points.len(),
            time(first.ts, "%d/%m/%Y %H:%M"),
            time(last.ts, "%d/%m/%Y %H:%M"),
        ),
    );
    if let Err(e) = &klines {
        out = out.with_notice(
            Level::Warning,
            format!("Cours indisponible, seul le ratio est affiché : {e}"),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const PERIOD: i64 = 4 * HOUR;
    /// 2026-10-27 08:00:00 UTC
    const T0: i64 = 1_793_088_000_000;

    fn point(ts: i64, ratio: f64) -> TopTraderPoint {
        TopTraderPoint {
            ts,
            ratio,
            long: ratio / (1.0 + ratio),
            short: 1.0 / (1.0 + ratio),
        }
    }

    fn kline(ts: i64, close: f64) -> Kline {
        Kline {
            ts,
            open: close - 1.0,
            high: close + 2.0,
            low: close - 2.0,
            close,
        }
    }

    #[test]
    fn percent_uses_a_decimal_comma() {
        assert_eq!(percent(0.7042), "70,4 %");
        assert_eq!(percent(0.5), "50,0 %");
    }

    #[test]
    fn timestamps_are_shown_in_utc() {
        assert_eq!(time(T0, "%Y-%m-%d %H:%M"), "2026-10-27 08:00");
        assert_eq!(time(T0, "%d/%m/%Y %H:%M"), "27/10/2026 08:00");
    }

    #[test]
    fn candles_start_at_the_first_ratio_point() {
        let klines = [
            kline(T0 - 2 * PERIOD, 1.0),
            kline(T0 - PERIOD, 2.0),
            kline(T0, 3.0),
            kline(T0 + PERIOD, 4.0),
        ];
        let shown = candles_from(&klines, T0);
        assert_eq!(
            shown.iter().map(|k| k.close).collect::<Vec<_>>(),
            [3.0, 4.0]
        );
        assert!(candles_from(&klines, T0 + 2 * PERIOD).is_empty());
        assert_eq!(candles_from(&klines, 0).len(), 4);
    }

    #[test]
    fn figure_without_candles_is_one_ratio_line_around_the_balance_line() {
        let points = [point(T0, 3.0), point(T0 + PERIOD, 0.8)];
        let fig = serde_json::to_value(build_figure(&points, &[], "ETHUSDT", "4 heures")).unwrap();

        let title = fig["layout"]["title"]["text"].as_str().unwrap();
        assert!(
            title.contains("ETHUSDT") && title.contains("4 heures"),
            "{title}"
        );
        assert_eq!(fig["layout"]["hovermode"], "x unified");
        // L'infobulle donne l'heure : Plotly n'affiche sinon que le jour, même en période de 4 h.
        assert_eq!(fig["layout"]["xaxis"]["hoverformat"], HOVER_TIME);

        assert_eq!(fig["data"].as_array().unwrap().len(), 1);
        let line = &fig["data"][0];
        assert_eq!(line["x"], json!(["2026-10-27 08:00", "2026-10-27 12:00"]));
        assert_eq!(line["y"], json!([3.0, 0.8]));
        assert_eq!(line["yaxis"], "y");
        assert!(
            fig["layout"].get("hoversubplots").is_none(),
            "un seul panneau"
        );
        // Infobulle : parts de comptes en pourcentage, alignées sur les points.
        assert_eq!(line["customdata"][0], json!([75.0, 25.0]));

        let balance = &fig["layout"]["shapes"][0];
        assert_eq!(balance["y0"], json!(1.0));
        assert_eq!(balance["y1"], json!(1.0));
        assert_eq!(balance["yref"], "y");
    }

    #[test]
    fn figure_with_candles_puts_the_price_above_the_ratio() {
        let points = [point(T0, 3.0), point(T0 + PERIOD, 0.8)];
        let klines = [kline(T0, 100.0), kline(T0 + PERIOD, 110.0)];
        let fig =
            serde_json::to_value(build_figure(&points, &klines, "BTCUSDT", "4 heures")).unwrap();
        let layout = &fig["layout"];

        // Deux panneaux sur un seul axe des dates (sans cela, l'infobulle unifiée ne réunit pas le
        // cours et le ratio), le cours en haut.
        assert!(layout.get("xaxis2").is_none());
        assert_eq!(layout["yaxis"]["anchor"], "x");
        assert_eq!(layout["yaxis2"]["anchor"], "x");
        assert!(layout["yaxis"]["domain"][0].as_f64() > layout["yaxis2"]["domain"][1].as_f64());
        assert_eq!(layout["hovermode"], "x unified");
        assert_eq!(layout["hoversubplots"], "axis");
        assert_eq!(layout["xaxis"]["rangeslider"]["visible"], false);
        assert_eq!(layout["xaxis"]["hoverformat"], HOVER_TIME);
        assert_eq!(layout["xaxis"]["title"]["text"], "Date (UTC)");
        assert_eq!(layout["yaxis"]["title"]["text"], "Cours (USDT)");
        assert_eq!(layout["yaxis"]["hoverformat"], PRICE_HOVER);
        // Les panneaux se nomment eux-mêmes : pas de légende.
        assert_eq!(layout["showlegend"], false);
        assert_eq!(layout["annotations"][0]["text"], "Cours BTCUSDT");
        let title = layout["title"]["text"].as_str().unwrap();
        assert!(
            title.contains("BTCUSDT") && title.contains("4 heures"),
            "{title}"
        );

        // Rien d'autre que le cours et le ratio.
        let traces = fig["data"].as_array().unwrap();
        assert_eq!(traces.len(), 2);
        let candles = &traces[0];
        assert_eq!(candles["type"], "candlestick");
        assert_eq!(
            candles["x"],
            json!(["2026-10-27 08:00", "2026-10-27 12:00"])
        );
        assert_eq!(candles["open"], json!([99.0, 109.0]));
        assert_eq!(candles["high"], json!([102.0, 112.0]));
        assert_eq!(candles["low"], json!([98.0, 108.0]));
        assert_eq!(candles["close"], json!([100.0, 110.0]));
        assert!(candles.get("yaxis").is_none(), "le cours est sur l'axe y");

        let ratio = &traces[1];
        assert_eq!(ratio["yaxis"], "y2");
        assert_eq!(ratio["y"], json!([3.0, 0.8]));
        assert!(
            traces.iter().all(|t| t.get("xaxis").is_none()),
            "tous les panneaux partagent l'axe des dates"
        );

        // La ligne d'équilibre suit le ratio, donc l'axe du bas.
        assert_eq!(layout["shapes"][0]["yref"], "y2");
        assert_eq!(
            layout["annotations"].as_array().unwrap().last().unwrap()["yref"],
            "y2"
        );
    }
}

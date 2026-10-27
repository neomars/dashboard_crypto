//! Cours de la monnaie choisie et ratio long/short des « top traders » de Binance Futures
//! (les 20 % de comptes ayant le plus de marge), avec un signal d'achat ou de vente
//! indicatif. Le cours est en haut, le ratio en dessous, comme sur CoinGlass (panneau
//! « Meilleurs traders longs/courts (Comptes) »). Règle du signal : `top_traders_signal`.

use super::top_traders_signal::{
    self as signal, Action, Analysis, Crowd, Reading, Trend, Windows, RATIO_DAYS, THRESHOLD,
    TREND_DAYS,
};
use super::{param_str, IndicatorOutput, Level, Metric};
use crate::binance::{self, Kline, TopTraderPoint};
use crate::data::DataProvider;
use crate::figure::{merge, two_rows_shared_x, Figure};
use crate::series::opt_json;
use chrono::DateTime;
use serde_json::{json, Value};

/// Format des dates de l'axe (Plotly lit `AAAA-MM-JJ HH:MM`).
const AXIS_TIME: &str = "%Y-%m-%d %H:%M";
/// Format de l'en-tête de l'infobulle : Plotly n'y affiche sinon que le jour, même en 4 h.
const HOVER_TIME: &str = "%d/%m/%Y %H:%M UTC";
/// Format des prix de l'infobulle (d3) : 6 chiffres significatifs, zéros inutiles retirés.
const PRICE_HOVER: &str = ",.6~g";

/// Horodatage Binance (millisecondes, UTC) selon le format `strftime` donné.
fn time(ts: i64, format: &str) -> String {
    DateTime::from_timestamp_millis(ts)
        .map(|t| t.format(format).to_string())
        .unwrap_or_default()
}

/// Nombre à la française : `2.375` → `2,3750` (4 décimales).
fn fr(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Nombre signé à la française : `2.1` → `+2,1`.
fn fr_signed(value: f64, decimals: usize) -> String {
    format!("{value:+.decimals$}").replace('.', ",")
}

/// Part (0 à 1) en pourcentage à la française : `0.7042` → `70,4 %`.
fn percent(share: f64) -> String {
    format!("{} %", fr(share * 100.0, 1))
}

/// Cours à la française : espace fine insécable entre les milliers, virgule décimale, et
/// assez de décimales pour les petits prix (`80 523,1`, `0,35210`).
fn fmt_price(price: f64) -> String {
    let decimals = match price {
        p if p >= 1000.0 => 1,
        p if p >= 100.0 => 2,
        p if p >= 1.0 => 3,
        p if p >= 0.01 => 5,
        _ => 7,
    };
    let text = format!("{price:.decimals$}");
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, digit) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push('\u{202F}');
        }
        grouped.push(digit);
    }
    if frac.is_empty() {
        grouped
    } else {
        format!("{grouped},{frac}")
    }
}

fn series(values: &[Option<f64>]) -> Vec<Value> {
    values.iter().map(|v| opt_json(*v)).collect()
}

/// Cours connu à chaque relevé : clôture de la dernière bougie terminée avant lui (ouverte une
/// période plus tôt, ou avant s'il manque des bougies). Jamais celle de la bougie en cours :
/// le signal ne doit pas connaître l'avenir.
fn known_prices(points: &[TopTraderPoint], klines: &[Kline], period_ms: i64) -> Vec<Option<f64>> {
    points
        .iter()
        .map(|p| {
            let last_closed_open = p.ts - period_ms;
            let n = klines.partition_point(|k| k.ts <= last_closed_open);
            n.checked_sub(1).map(|i| klines[i].close)
        })
        .collect()
}

/// Le cours et le signal, alignés sur les relevés du ratio qui ont un cours connu.
pub(crate) struct Study<'a> {
    pub points: &'a [TopTraderPoint],
    /// Cours connu à chaque relevé.
    pub prices: Vec<f64>,
    /// Bougies affichées, du premier relevé à maintenant.
    pub klines: &'a [Kline],
    pub analysis: Analysis,
    pub reading: Reading,
    pub windows: Windows,
}

impl<'a> Study<'a> {
    /// `None` quand aucune bougie ne précède les relevés (rien à comparer).
    pub fn new(
        points: &'a [TopTraderPoint],
        klines: &'a [Kline],
        period_ms: i64,
        reading: Reading,
    ) -> Option<Self> {
        let prices = known_prices(points, klines, period_ms);
        let start = prices.iter().position(Option::is_some)?;
        let points = &points[start..];
        let prices: Vec<f64> = prices[start..].iter().copied().collect::<Option<_>>()?;
        let ratios: Vec<f64> = points.iter().map(|p| p.ratio).collect();
        let windows = Windows::for_period(period_ms);
        let analysis = signal::analyze(&ratios, &prices, reading, windows);
        let shown_from = klines.partition_point(|k| k.ts < points[0].ts);
        Some(Self {
            points,
            prices,
            klines: &klines[shown_from..],
            analysis,
            reading,
            windows,
        })
    }

    /// Cours actuel : clôture de la bougie en cours, ou à défaut du dernier relevé.
    fn current_price(&self) -> f64 {
        self.klines
            .last()
            .map(|k| k.close)
            .or(self.prices.last().copied())
            .unwrap_or_default()
    }

    /// Signal en vigueur au dernier relevé : `None` tant que l'historique est insuffisant.
    fn current_state(&self) -> Option<Option<Action>> {
        let last = self.points.len() - 1;
        self.analysis.crowd[last]?;
        Some(self.analysis.state[last])
    }
}

/// Valeur de la tuile « Signal ».
fn signal_tile(study: &Study) -> String {
    match study.current_state() {
        None => "⚪ INDÉTERMINÉ".into(),
        Some(Some(Action::Buy)) => "🟢 ACHAT".into(),
        Some(Some(Action::Sell)) => "🔴 VENTE".into(),
        Some(None) => "⚪ NEUTRE".into(),
    }
}

/// Explique le signal du dernier relevé : positionnement, tendance du cours, et ce qu'en dit la lecture.
fn describe(study: &Study) -> String {
    let a = &study.analysis;
    let i = study.points.len() - 1;
    let reading = study.reading;
    let (Some(mean), Some(z), Some(crowd), Some(trend)) =
        (a.mean[i], a.z[i], a.crowd[i], a.trend[i])
    else {
        return format!(
            "Lecture {} : historique insuffisant pour évaluer un signal (il faut au moins {} relevés).",
            reading.label(),
            study.windows.ratio
        );
    };
    let figures = format!(
        "ratio {} contre {} en moyenne sur {RATIO_DAYS} jours, soit {} écarts-types",
        fr(study.points[i].ratio, 4),
        fr(mean, 4),
        fr_signed(z, 1)
    );
    let position = match crowd {
        Crowd::Long => "les top traders sont bien plus long que d'habitude",
        Crowd::Short => "les top traders sont bien plus short que d'habitude",
        Crowd::Normal => "les top traders sont positionnés comme d'habitude",
    };
    let price = match trend {
        Trend::Up => {
            format!("le cours est au-dessus de sa moyenne des {TREND_DAYS} derniers jours")
        }
        Trend::Down => format!("le cours est sous sa moyenne des {TREND_DAYS} derniers jours"),
    };
    let verdict = match (signal::signal_for(reading, crowd, trend), reading) {
        (Some(Action::Sell), Reading::Contrarian) => {
            "les positions longues sont à risque (liquidations si le cours continue de baisser) : VENTE"
        }
        (Some(Action::Buy), Reading::Contrarian) => {
            "les positions short sont à risque (rachats forcés si le cours continue de monter) : ACHAT"
        }
        (Some(Action::Buy), Reading::SmartMoney) => "ils achètent la baisse : ACHAT",
        (Some(Action::Sell), Reading::SmartMoney) => "ils vendent la hausse : VENTE",
        (None, _) if crowd == Crowd::Normal => "pas de positionnement extrême : NEUTRE",
        (None, _) => "le cours confirme le positionnement, aucun signal : NEUTRE",
    };
    format!(
        "Lecture {} : {position} ({figures}) ; {price} → {verdict}.",
        reading.label()
    )
}

/// Dernier signal de la période affichée, avec l'évolution du cours depuis.
fn last_signal(study: &Study) -> String {
    let Some((i, action)) = study.analysis.last_event() else {
        return "Aucun signal sur la période affichée.".into();
    };
    let at = study.prices[i];
    let since = (study.current_price() / at - 1.0) * 100.0;
    format!(
        "Dernier signal : {} le {} UTC, au cours de {} (depuis : {} %).",
        action.label(),
        time(study.points[i].ts, "%d/%m/%Y %H:%M"),
        fmt_price(at),
        fr_signed(since, 1)
    )
}

/// Courbe du ratio sur l'axe vertical `yaxis` (`y` seul, `y2` sous le cours) ; l'axe des dates est
/// toujours le même pour tous les panneaux.
fn ratio_trace(points: &[TopTraderPoint], x: &[String], yaxis: &str) -> Value {
    json!({
        "type": "scatter", "mode": "lines", "name": "Ratio top traders", "x": x,
        "y": points.iter().map(|p| p.ratio).collect::<Vec<_>>(),
        "customdata": points.iter().map(|p| [p.long * 100.0, p.short * 100.0]).collect::<Vec<_>>(),
        "hovertemplate": "Ratio : %{y:.4f}<br>Comptes long : %{customdata[0]:.1f} %<br>Comptes short : %{customdata[1]:.1f} %<extra></extra>",
        "line": {"color": "#26A69A", "width": 2}, "yaxis": yaxis
    })
}

/// Panneau du haut : bougies, moyenne du cours et marqueurs d'achat et de vente.
fn price_traces(fig: &mut Figure, study: &Study, x: &[String]) {
    let k = study.klines;
    let col = |f: fn(&Kline) -> f64| k.iter().map(f).collect::<Vec<f64>>();
    fig.trace(json!({
        "type": "candlestick", "name": "Cours",
        "x": k.iter().map(|k| time(k.ts, AXIS_TIME)).collect::<Vec<_>>(),
        "open": col(|k| k.open), "high": col(|k| k.high), "low": col(|k| k.low), "close": col(|k| k.close),
        "increasing": {"line": {"color": "#00FF88"}}, "decreasing": {"line": {"color": "#FF3333"}}
    }));
    fig.trace(json!({
        "type": "scatter", "mode": "lines", "name": format!("Moyenne cours {TREND_DAYS} j"),
        "x": x, "y": series(&study.analysis.price_ma),
        "line": {"color": "#FFA726", "width": 1.5, "dash": "dot"}
    }));
    for (action, shape, color, name) in [
        (Action::Buy, "triangle-up", "#00E676", "Achat"),
        (Action::Sell, "triangle-down", "#FF1744", "Vente"),
    ] {
        let at: Vec<usize> = study
            .analysis
            .events
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(i, _)| *i)
            .collect();
        if at.is_empty() {
            continue;
        }
        fig.trace(json!({
            "type": "scatter", "mode": "markers", "name": name,
            "x": at.iter().map(|i| x[*i].clone()).collect::<Vec<_>>(),
            "y": at.iter().map(|i| study.prices[*i]).collect::<Vec<_>>(),
            "marker": {"symbol": shape, "size": 15, "color": color, "line": {"color": "white", "width": 1}},
            "hovertemplate": format!("{} (lecture {})<extra></extra>", action.label(), study.reading.label())
        }));
    }
}

/// Panneau du bas : moyenne du ratio et bandes au-delà desquelles le positionnement est extrême.
fn band_traces(fig: &mut Figure, analysis: &Analysis, x: &[String]) {
    let edge = |sign: f64| -> Vec<Option<f64>> {
        analysis
            .mean
            .iter()
            .zip(&analysis.band)
            .map(|(m, b)| Some((*m)? + sign * (*b)?))
            .collect()
    };
    fig.trace(json!({
        "type": "scatter", "mode": "lines", "name": format!("Moyenne ratio {RATIO_DAYS} j"),
        "x": x, "y": series(&analysis.mean), "yaxis": "y2", "hoverinfo": "skip",
        "line": {"color": "rgba(255,255,255,0.5)", "width": 1, "dash": "dot"}
    }));
    for (sign, legend) in [(1.0, true), (-1.0, false)] {
        fig.trace(json!({
            "type": "scatter", "mode": "lines", "name": format!("Seuils (±{} σ)", fr(THRESHOLD, 1)),
            "legendgroup": "bands", "showlegend": legend,
            "x": x, "y": series(&edge(sign)), "yaxis": "y2", "hoverinfo": "skip",
            "line": {"color": "rgba(255,214,0,0.6)", "width": 1, "dash": "dash"}
        }));
    }
}

/// Figure : le cours et ses signaux au-dessus du ratio quand `study` existe, le ratio seul sinon.
pub(crate) fn build_figure(
    points: &[TopTraderPoint],
    study: Option<&Study>,
    symbol: &str,
    period: &str,
) -> Figure {
    // Les indices de l'analyse (marqueurs, bandes) sont ceux des relevés de l'étude.
    let points = study.map_or(points, |s| s.points);
    let x: Vec<String> = points.iter().map(|p| time(p.ts, AXIS_TIME)).collect();
    // Le ratio est le panneau du bas (`y2`) sous le cours, ou le seul graphique (`y`).
    let ratio_axis = if study.is_some() { "y2" } else { "y" };
    let mut layout = match study {
        Some(_) => {
            // Un seul axe des dates pour les deux panneaux : l'infobulle unifiée les réunit.
            let price_title = format!("Cours {symbol} et signaux");
            let mut layout = two_rows_shared_x(
                [0.68, 0.32],
                0.08,
                Some([&price_title, "Ratio Long/Short des comptes des top traders"]),
            );
            merge(
                &mut layout,
                json!({
                    "title": {"text": format!("{symbol} - Cours et top traders Binance ({period})")},
                    "height": 900,
                    "legend": {"orientation": "v", "yanchor": "top", "y": 1, "xanchor": "left", "x": 1.02},
                    "margin": {"r": 170},
                    // Noms complets dans l'infobulle (« Moyenne cours 2 j » est coupé sinon).
                    "hoverlabel": {"namelength": -1},
                    "xaxis": {"title": {"text": "Date (UTC)"}, "rangeslider": {"visible": false}, "hoverformat": HOVER_TIME},
                    // 6 chiffres significatifs : 80 523,1 comme 0,3521 (Plotly écrirait « 80.52k » sinon).
                    "yaxis": {"title": {"text": "Cours (USDT)"}, "hoverformat": PRICE_HOVER},
                    "yaxis2": {"title": {"text": "Comptes long / comptes short"}}
                }),
            );
            layout
        }
        None => json!({
            "title": {"text": format!("{symbol} - Top traders Binance : ratio Long/Short des comptes ({period})")},
            "height": 600,
            "xaxis": {"title": {"text": "Date (UTC)"}, "hoverformat": HOVER_TIME},
            "yaxis": {"title": {"text": "Comptes long / comptes short"}},
            "margin": {"t": 80, "b": 60}
        }),
    };
    merge(&mut layout, json!({"hovermode": "x unified"}));

    let mut fig = Figure::new(layout);
    if let Some(study) = study {
        price_traces(&mut fig, study, &x);
    }
    fig.trace(ratio_trace(points, &x, ratio_axis));
    if let Some(study) = study {
        band_traces(&mut fig, &study.analysis, &x);
    }
    fig.hline(1.0, "white", "dash", ratio_axis, "paper");
    fig.annotation(json!({"xref": "paper", "yref": ratio_axis, "x": 1, "y": 1.0, "xanchor": "right", "yanchor": "bottom", "text": "Équilibre", "showarrow": false}));
    fig
}

pub async fn render(data: &DataProvider, params: &Value) -> Result<IndicatorOutput, String> {
    let period = param_str(params, "period", "4h");
    let (Some(period_label), Some(period_ms)) =
        (binance::period_label(period), binance::period_ms(period))
    else {
        return Err(format!("Période inconnue : {period}"));
    };
    let reading_param = param_str(params, "reading", "contrarian");
    let reading = Reading::from_param(reading_param)
        .ok_or_else(|| format!("Lecture inconnue : {reading_param}"))?;
    let symbol = binance::symbol(param_str(params, "coin", "BTC"))?;

    let points = data.binance_top_traders(&symbol, period).await?;
    if points.is_empty() {
        return Err(format!("Aucune donnée Binance pour {symbol}."));
    }

    // Le cours est la moitié de l'indicateur, mais s'il manque, le ratio reste affiché sans signal.
    let mut warnings: Vec<String> = Vec::new();
    let klines = match data.binance_klines(&symbol, period).await {
        Ok(klines) => Some(klines),
        Err(e) => {
            warnings.push(format!(
                "Cours indisponible, seul le ratio est affiché (sans signal) : {e}"
            ));
            None
        }
    };
    let study = klines
        .as_deref()
        .and_then(|k| Study::new(&points, k, period_ms, reading));
    if klines.is_some() && study.is_none() {
        warnings.push(
            "Aucune bougie Binance ne précède les relevés du ratio : seul le ratio est affiché, sans signal."
                .into(),
        );
    }

    let shown: &[TopTraderPoint] = study.as_ref().map_or(&points[..], |s| s.points);
    let (first, last) = (&shown[0], &shown[shown.len() - 1]);
    let mut out = IndicatorOutput::from(build_figure(shown, study.as_ref(), &symbol, period_label));

    if let Some(study) = &study {
        out.metrics.push(Metric {
            label: format!("Cours {symbol}"),
            value: fmt_price(study.current_price()),
        });
    }
    out.metrics.push(Metric {
        label: "Ratio Long/Short actuel".into(),
        value: fr(last.ratio, 4),
    });
    out.metrics.push(Metric {
        label: "Comptes long".into(),
        value: percent(last.long),
    });
    out.metrics.push(Metric {
        label: "Comptes short".into(),
        value: percent(last.short),
    });
    if let Some(study) = &study {
        out.metrics.push(Metric {
            label: "Signal".into(),
            value: signal_tile(study),
        });
    }

    out = out.with_notice(
        Level::Info,
        format!(
            "Binance ne publie que les {} derniers jours pour ce ratio : {} relevés (période de {period_label}), du {} au {} UTC.",
            binance::HISTORY_DAYS,
            shown.len(),
            time(first.ts, "%d/%m/%Y %H:%M"),
            time(last.ts, "%d/%m/%Y %H:%M"),
        ),
    );
    for warning in warnings {
        out = out.with_notice(Level::Warning, warning);
    }
    if let Some(study) = &study {
        out = out
            .with_notice(
                Level::Info,
                format!("{}\n{}", describe(study), last_signal(study)),
            )
            .with_notice(
                Level::Warning,
                format!(
                    "Signal indicatif, non validé : Binance ne publie que {} jours de ratio, trop peu pour mesurer la fiabilité d'une règle. Ce n'est pas un conseil en investissement.",
                    binance::HISTORY_DAYS
                ),
            );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::top_traders_signal::MIN_DEVIATION;
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

    /// 60 relevés toutes les 4 h : ratio autour de 1,5, puis 2,4 au dernier ; cours qui monte
    /// puis chute sur le dernier relevé. Les bougies commencent 3 jours avant le premier relevé.
    fn scenario() -> (Vec<TopTraderPoint>, Vec<Kline>) {
        let n = 60;
        let points: Vec<TopTraderPoint> = (0..n)
            .map(|i| {
                let ratio = if i == n - 1 {
                    2.4
                } else if i % 2 == 0 {
                    1.48
                } else {
                    1.52
                };
                point(T0 + i * PERIOD, ratio)
            })
            .collect();
        // Cours : monte de 1 par période, sauf la dernière bougie close (avant le dernier relevé).
        let klines: Vec<Kline> = (-18..n + 1)
            .map(|i| {
                let close = if i == n - 2 { 90.0 } else { 100.0 + i as f64 };
                kline(T0 + i * PERIOD, close)
            })
            .collect();
        (points, klines)
    }

    #[test]
    fn percent_and_prices_use_french_formats() {
        assert_eq!(percent(0.7042), "70,4 %");
        assert_eq!(percent(0.5), "50,0 %");
        assert_eq!(fr(2.375, 4), "2,3750");
        assert_eq!(fr_signed(2.14, 1), "+2,1");
        assert_eq!(fr_signed(-0.46, 1), "-0,5");
        assert_eq!(fmt_price(80_523.1), "80\u{202F}523,1");
        assert_eq!(fmt_price(1_234_567.0), "1\u{202F}234\u{202F}567,0");
        assert_eq!(fmt_price(250.5), "250,50");
        assert_eq!(fmt_price(2.5), "2,500");
        assert_eq!(fmt_price(0.3521), "0,35210");
        assert_eq!(fmt_price(0.000_123_4), "0,0001234");
    }

    #[test]
    fn timestamps_are_shown_in_utc() {
        assert_eq!(time(T0, "%Y-%m-%d %H:%M"), "2026-10-27 08:00");
        assert_eq!(time(T0, "%d/%m/%Y %H:%M"), "27/10/2026 08:00");
    }

    #[test]
    fn the_price_known_at_a_point_is_the_last_closed_candle_never_the_running_one() {
        // Bougies de 4 h ouvertes à 0, 4, 8 et 16 h (celle de 12 h manque) ; clôtures 10, 20, 30, 50.
        let klines = [
            kline(0, 10.0),
            kline(PERIOD, 20.0),
            kline(2 * PERIOD, 30.0),
            kline(4 * PERIOD, 50.0),
        ];
        let points = [
            point(PERIOD, 1.0),     // 4 h : la bougie de 0 h vient de se clore
            point(2 * PERIOD, 1.0), // 8 h : celle de 4 h
            point(3 * PERIOD, 1.0), // 12 h : celle de 8 h
            point(4 * PERIOD, 1.0), // 16 h : celle de 12 h manque -> la dernière connue, 8 h
            point(5 * PERIOD, 1.0), // 20 h : celle de 16 h
        ];
        assert_eq!(
            known_prices(&points, &klines, PERIOD),
            [Some(10.0), Some(20.0), Some(30.0), Some(30.0), Some(50.0)]
        );
        // Avant toute bougie close : pas de cours.
        assert_eq!(known_prices(&[point(0, 1.0)], &klines, PERIOD), [None]);
    }

    #[test]
    fn leading_points_without_a_price_are_left_out_of_the_study() {
        let klines = [kline(PERIOD, 20.0), kline(2 * PERIOD, 30.0)];
        let points = [
            point(0, 1.0),          // aucune bougie avant : écarté
            point(PERIOD, 1.0),     // la bougie de 0 h manque : écarté
            point(2 * PERIOD, 1.0), // bougie de 4 h : cours 20
            point(3 * PERIOD, 1.0),
        ];
        let study = Study::new(&points, &klines, PERIOD, Reading::Contrarian).unwrap();
        assert_eq!(study.points.len(), 2);
        assert_eq!(study.prices, [20.0, 30.0]);
        // La figure suit les relevés de l'étude même si on lui donne tous les relevés : les indices
        // des marqueurs et des bandes sont ceux de l'étude.
        let fig = serde_json::to_value(build_figure(&points, Some(&study), "BTCUSDT", "4 heures"))
            .unwrap();
        let ratio = fig["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "Ratio top traders")
            .unwrap();
        assert_eq!(ratio["x"].as_array().unwrap().len(), 2);
        assert!(Study::new(&points[..2], &klines, PERIOD, Reading::Contrarian).is_none());
    }

    #[test]
    fn the_study_signals_a_crowded_long_against_a_falling_price() {
        let (points, klines) = scenario();
        let study = Study::new(&points, &klines, PERIOD, Reading::Contrarian).unwrap();
        let last = points.len() - 1;
        // Au dernier relevé : foule long (2,4 contre ~1,5), cours connu 90 sous sa moyenne.
        assert_eq!(study.prices[last], 90.0);
        assert_eq!(study.analysis.crowd[last], Some(Crowd::Long));
        assert_eq!(study.analysis.trend[last], Some(Trend::Down));
        assert_eq!(study.analysis.events, [(last, Action::Sell)]);
        assert_eq!(signal_tile(&study), "🔴 VENTE");

        let text = describe(&study);
        assert!(text.starts_with("Lecture contrarienne :"), "{text}");
        assert!(text.contains("bien plus long que d'habitude"), "{text}");
        assert!(text.contains("ratio 2,4000 contre"), "{text}");
        assert!(
            text.contains("sous sa moyenne des 2 derniers jours"),
            "{text}"
        );
        assert!(text.ends_with("VENTE."), "{text}");

        // Cours actuel = clôture de la bougie en cours (T0 + 60 périodes : 100 + 60) ; le signal
        // a été donné au cours connu 90.
        let line = last_signal(&study);
        assert!(line.starts_with("Dernier signal : VENTE le "), "{line}");
        assert!(
            line.contains("au cours de 90,000 (depuis : +77,8 %)"),
            "{line}"
        );
    }

    #[test]
    fn the_other_reading_turns_the_same_situation_into_a_buy() {
        let (points, klines) = scenario();
        let study = Study::new(&points, &klines, PERIOD, Reading::SmartMoney).unwrap();
        assert_eq!(study.analysis.events, [(points.len() - 1, Action::Buy)]);
        assert_eq!(signal_tile(&study), "🟢 ACHAT");
        let text = describe(&study);
        assert!(text.starts_with("Lecture smart money :"), "{text}");
        assert!(text.ends_with("ils achètent la baisse : ACHAT."), "{text}");
    }

    #[test]
    fn a_calm_market_is_neutral_and_a_short_history_is_undetermined() {
        let (mut points, klines) = scenario();
        let last = points.len() - 1;
        points[last] = point(points[last].ts, 1.5);
        let study = Study::new(&points, &klines, PERIOD, Reading::Contrarian).unwrap();
        assert_eq!(signal_tile(&study), "⚪ NEUTRE");
        assert!(study.analysis.events.is_empty());
        assert!(describe(&study).ends_with("pas de positionnement extrême : NEUTRE."));
        assert_eq!(last_signal(&study), "Aucun signal sur la période affichée.");

        // Moins de relevés que la fenêtre du ratio (42 en 4 h) : indéterminé.
        let short = &points[..20];
        let study = Study::new(short, &klines, PERIOD, Reading::Contrarian).unwrap();
        assert_eq!(signal_tile(&study), "⚪ INDÉTERMINÉ");
        assert!(
            describe(&study).contains("historique insuffisant"),
            "{}",
            describe(&study)
        );
    }

    #[test]
    fn a_price_that_confirms_the_crowd_gives_no_signal() {
        let (points, mut klines) = scenario();
        // Le cours continue de monter jusqu'au dernier relevé : la foule long a raison.
        for k in &mut klines {
            *k = kline(k.ts, 100.0 + ((k.ts - T0) / PERIOD) as f64);
        }
        let study = Study::new(&points, &klines, PERIOD, Reading::Contrarian).unwrap();
        assert_eq!(study.analysis.crowd[points.len() - 1], Some(Crowd::Long));
        assert!(study.analysis.events.is_empty());
        assert!(describe(&study)
            .ends_with("le cours confirme le positionnement, aucun signal : NEUTRE."));
    }

    #[test]
    fn figure_without_a_price_is_one_ratio_line_around_the_balance_line() {
        let points = [point(T0, 3.0), point(T0 + PERIOD, 0.8)];
        let fig = serde_json::to_value(build_figure(&points, None, "ETHUSDT", "4 heures")).unwrap();

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
    fn figure_with_a_price_puts_the_candles_above_the_ratio_and_marks_the_signals() {
        let (points, klines) = scenario();
        let study = Study::new(&points, &klines, PERIOD, Reading::Contrarian).unwrap();
        let fig = serde_json::to_value(build_figure(&points, Some(&study), "BTCUSDT", "4 heures"))
            .unwrap();
        let layout = &fig["layout"];

        // Deux panneaux sur un seul axe des dates (sans cela, l'infobulle unifiée ne réunit pas le
        // cours et le ratio), le cours en haut.
        assert!(layout.get("xaxis2").is_none());
        assert_eq!(layout["yaxis"]["anchor"], "x");
        assert_eq!(layout["yaxis2"]["anchor"], "x");
        assert_eq!(layout["xaxis"]["rangeslider"]["visible"], false);
        assert_eq!(layout["xaxis"]["hoverformat"], HOVER_TIME);
        assert_eq!(layout["xaxis"]["title"]["text"], "Date (UTC)");
        assert!(layout["yaxis"]["domain"][0].as_f64() > layout["yaxis2"]["domain"][1].as_f64());
        assert_eq!(layout["yaxis"]["title"]["text"], "Cours (USDT)");
        assert_eq!(layout["yaxis"]["hoverformat"], PRICE_HOVER);
        assert_eq!(layout["hoverlabel"]["namelength"], -1);
        assert_eq!(
            layout["annotations"][0]["text"], "Cours BTCUSDT et signaux",
            "titre du panneau du haut"
        );
        assert_eq!(layout["hovermode"], "x unified");
        assert_eq!(layout["hoversubplots"], "axis");

        let traces = fig["data"].as_array().unwrap();
        let named = |name: &str| traces.iter().find(|t| t["name"] == name).unwrap();

        // Bougies du premier relevé à maintenant (les 3 jours de marge ne sont pas affichées).
        let candles = named("Cours");
        assert_eq!(candles["type"], "candlestick");
        assert_eq!(candles["x"][0], "2026-10-27 08:00");
        assert_eq!(candles["x"].as_array().unwrap().len(), 61);

        // Marqueur de vente au dernier relevé, au cours connu à cet instant ; aucun d'achat.
        let sell = named("Vente");
        assert_eq!(sell["marker"]["symbol"], "triangle-down");
        assert_eq!(sell["x"], json!([time(points[59].ts, AXIS_TIME)]));
        assert_eq!(sell["y"], json!([90.0]));
        assert!(traces.iter().all(|t| t["name"] != "Achat"));

        // Le ratio est dans le panneau du bas, avec sa moyenne et ses bandes.
        let ratio = named("Ratio top traders");
        assert_eq!(ratio["yaxis"], "y2");
        for name in ["Moyenne ratio 7 j", "Seuils (±1,5 σ)"] {
            let t = named(name);
            assert_eq!(t["yaxis"], "y2", "{name}");
            assert_eq!(t["hoverinfo"], "skip");
        }
        // Aucune trace n'a son propre axe des dates : tous les panneaux partagent `x`.
        assert!(traces.iter().all(|t| t.get("xaxis").is_none()));
        // Bandes : vides tant que la fenêtre n'est pas pleine, puis moyenne ± demi-largeur.
        let bands: Vec<&Value> = traces
            .iter()
            .filter(|t| t["legendgroup"] == "bands")
            .collect();
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0]["showlegend"], true);
        assert_eq!(bands[1]["showlegend"], false);
        assert_eq!(bands[0]["y"][0], Value::Null);
        let (upper, lower) = (
            bands[0]["y"][59].as_f64().unwrap(),
            bands[1]["y"][59].as_f64().unwrap(),
        );
        let mean = study.analysis.mean[59].unwrap();
        let band = study.analysis.band[59].unwrap();
        assert!((upper - (mean + band)).abs() < 1e-12 && (lower - (mean - band)).abs() < 1e-12);
        assert!(MIN_DEVIATION > 0.0 && band >= MIN_DEVIATION);

        // La ligne d'équilibre suit le ratio, donc l'axe du bas.
        assert_eq!(layout["shapes"][0]["yref"], "y2");
        assert_eq!(
            layout["annotations"].as_array().unwrap().last().unwrap()["yref"],
            "y2"
        );
    }
}

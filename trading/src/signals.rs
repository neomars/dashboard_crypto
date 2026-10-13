//! État du module tendance à la dernière clôture : ce que le moteur ferait
//! maintenant. Alimente la page « Signaux de tendance » du tableau de bord.

use crate::chart::{signals_chart, Chart, SignalSeries};
use crate::feed::MarketFeed;
use crate::market::{fmt_time, Bar, Instrument, Interval, Market};
use crate::risk::{target_exposure, ATR_LOOKBACK, VOL_WINDOW_DAYS};
use crate::strategy::{trend_signal, TrendVariant};
use crate::ta::{atr, realized_vol};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Volatilité annualisée visée par défaut (25 %).
pub const DEFAULT_TARGET_VOL: f64 = 0.25;
/// Distance indicative du stop suiveur, en ATR.
pub const DEFAULT_STOP_ATR: f64 = 3.0;

/// Levier maximal par défaut : 1 en spot, 2 en perpétuel (plafond du trading réel).
pub fn default_max_leverage(market: Market) -> f64 {
    match market {
        Market::Spot => 1.0,
        Market::Perp => 2.0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalParams {
    pub base: String,
    pub market: Market,
    pub interval: Interval,
    pub variant: TrendVariant,
}

impl SignalParams {
    pub fn instrument(&self) -> Result<Instrument, String> {
        Instrument::new(&self.base, self.market)
    }

    /// Historique à charger : 3 ans en journalier, 1 an en 4 heures.
    pub fn data_start(&self, today: NaiveDate) -> NaiveDate {
        today
            - Duration::days(match self.interval {
                Interval::D1 => 3 * 365,
                Interval::H4 => 365,
                Interval::H1 => 90,
                Interval::M15 => 30,
                Interval::M1 => 3,
            })
    }

    /// Bougies affichées : 2 ans en journalier, 180 jours en 4 heures.
    pub fn display_bars(&self) -> usize {
        self.interval.bars_for_days(match self.interval {
            Interval::D1 => 730,
            Interval::H4 => 180,
            Interval::H1 => 30,
            Interval::M15 => 7,
            Interval::M1 => 1,
        })
    }
}

/// État d'un signal de cassure (variante Donchian).
#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub lookback: usize,
    pub state: Option<i8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalReport {
    pub symbol: String,
    pub label: String,
    /// Clôture sur laquelle le signal est calculé.
    pub last_date: String,
    pub close: f64,
    pub score: Option<f64>,
    /// Exposition que le moteur viserait à l'ouverture suivante (fraction du capital).
    pub target_exposure: f64,
    pub max_leverage: f64,
    pub realized_vol_pct: Option<f64>,
    pub atr: Option<f64>,
    /// Stop indicatif à 3 ATR de la clôture, du côté de la position visée.
    pub stop_hint: Option<f64>,
    pub components: Vec<Component>,
    pub chart: Chart,
}

/// Charge les bougies Poloniex nécessaires puis calcule le signal à la dernière clôture.
pub async fn report_with_feed(
    feed: &MarketFeed,
    params: &SignalParams,
    today: NaiveDate,
) -> Result<SignalReport, String> {
    let inst = params.instrument()?;
    let bars = feed
        .bars(&inst, params.interval, params.data_start(today))
        .await?;
    signal_report(&bars, params)
}

pub fn signal_report(bars: &[Bar], params: &SignalParams) -> Result<SignalReport, String> {
    let inst = params.instrument()?;
    let last = bars
        .len()
        .checked_sub(1)
        .ok_or_else(|| format!("Aucune bougie pour {}.", inst.symbol()))?;
    let interval = params.interval;
    let allow_short = inst.market.allows_short();
    let max_leverage = default_max_leverage(inst.market);

    let signal = trend_signal(bars, params.variant, allow_short);
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let vol = realized_vol(
        &closes,
        interval.bars_for_days(VOL_WINDOW_DAYS),
        interval.bars_per_year(),
    );
    let atr_v = atr(bars, ATR_LOOKBACK);
    let exposure: Vec<f64> = (0..bars.len())
        .map(|i| match (signal.score[i], vol[i]) {
            (Some(s), Some(v)) => {
                target_exposure(s, v, DEFAULT_TARGET_VOL, max_leverage, allow_short)
            }
            _ => 0.0,
        })
        .collect();

    let target = exposure[last];
    let close = bars[last].close;
    let stop_hint = atr_v[last]
        .filter(|_| target != 0.0)
        .map(|a| close - target.signum() * DEFAULT_STOP_ATR * a);
    let from = bars.len().saturating_sub(params.display_bars());
    let chart = signals_chart(&SignalSeries {
        bars,
        from,
        interval,
        variant: params.variant,
        score: &signal.score,
        exposure: &exposure,
        title: format!(
            "{} — {} ({})",
            inst.label(),
            params.variant.label(),
            interval.label()
        ),
    });

    Ok(SignalReport {
        symbol: inst.symbol(),
        label: inst.label(),
        last_date: fmt_time(bars[last].time, interval),
        close,
        score: signal.score[last],
        target_exposure: target,
        max_leverage,
        realized_vol_pct: vol[last].map(|v| v * 100.0),
        atr: atr_v[last],
        stop_hint,
        components: signal
            .components
            .iter()
            .map(|(lookback, states)| Component {
                lookback: *lookback,
                state: states[last],
            })
            .collect(),
        chart,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::DAY_MS;

    fn bars(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: i as i64 * DAY_MS,
                open: c,
                high: c * 1.001,
                low: c * 0.999,
                close: c,
                volume: 1.0,
            })
            .collect()
    }

    fn params(market: Market, variant: TrendVariant) -> SignalParams {
        SignalParams {
            base: "BTC".into(),
            market,
            interval: Interval::D1,
            variant,
        }
    }

    #[test]
    fn uptrend_report_targets_a_long_position() {
        let closes: Vec<f64> = (0..400)
            .map(|i| 100.0 * 1.002f64.powi(i) * (1.0 + 0.01 * ((i % 4) as f64 - 1.5)))
            .collect();
        let r = signal_report(
            &bars(&closes),
            &params(Market::Spot, TrendVariant::Donchian),
        )
        .unwrap();
        assert_eq!(r.score, Some(1.0));
        assert!(r.target_exposure > 0.0 && r.target_exposure <= 1.0);
        assert!(r.stop_hint.unwrap() < r.close);
        assert_eq!(r.components.len(), 4);
        assert_eq!(r.last_date, "1971-02-04");
        // 730 bougies affichées au plus.
        let x = r.chart.data[0]["x"].as_array().unwrap().len();
        assert_eq!(x, 400);
        assert_eq!(r.chart.data.len(), 5, "cours, canal (2), score, exposition");
    }

    #[test]
    fn ema_variant_has_no_components() {
        let closes: Vec<f64> = (0..300)
            .map(|i| 100.0 * 0.995f64.powi(i) * (1.0 + 0.01 * ((i % 4) as f64 - 1.5)))
            .collect();
        let r = signal_report(&bars(&closes), &params(Market::Perp, TrendVariant::Ema)).unwrap();
        assert_eq!(r.score, Some(-1.0));
        assert!(r.target_exposure < 0.0 && r.target_exposure >= -2.0);
        assert!(
            r.stop_hint.unwrap() > r.close,
            "stop au-dessus d'une position vendeuse"
        );
        assert!(r.components.is_empty());
        assert_eq!(
            r.chart.data.len(),
            6,
            "cours, 3 moyennes, score, exposition"
        );
    }

    #[test]
    fn empty_history_is_an_error() {
        assert!(signal_report(&[], &params(Market::Spot, TrendVariant::Donchian)).is_err());
    }

    #[test]
    fn display_window_depends_on_the_interval() {
        let mut p = params(Market::Spot, TrendVariant::Donchian);
        assert_eq!(p.display_bars(), 730);
        p.interval = Interval::H4;
        assert_eq!(p.display_bars(), 1080);
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        assert_eq!(p.data_start(today), today - Duration::days(365));
    }
}

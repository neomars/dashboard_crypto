//! Indicateurs : chacun produit une figure Plotly, éventuellement accompagnée
//! de messages (équivalents de `st.info` / `st.warning`) et de métriques.

use crate::data::DataProvider;
use crate::figure::Figure;
use serde::Serialize;
use serde_json::Value;

mod bmi;
mod bmsb;
mod cycle;
mod cycle_roi;
mod fear_greed;
mod halving;
mod institutional;
mod long_short;
mod net_realized_pnl;
mod onchain;
mod realized_cap_utxo;
mod sopr;
mod vcr;
mod volume;

pub use bmi::find_corrections;
pub use bmsb::{calculate_bmsb, Regime};
pub use net_realized_pnl::{bubble_sizes, weekly_net_realized, WeeklyPnl};
pub use vcr::annualized_vol;

pub const IDS: [&str; 14] = [
    "fear_greed",
    "halving",
    "onchain",
    "cycle",
    "sopr",
    "institutional",
    "volume",
    "vcr",
    "bmi",
    "bmsb",
    "long_short",
    "cycle_roi",
    "realized_cap_utxo",
    "net_realized_pnl",
];

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Metric {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndicatorOutput {
    pub figure: Figure,
    pub notices: Vec<Notice>,
    pub metrics: Vec<Metric>,
}

impl From<Figure> for IndicatorOutput {
    fn from(figure: Figure) -> Self {
        Self {
            figure,
            notices: Vec::new(),
            metrics: Vec::new(),
        }
    }
}

impl IndicatorOutput {
    pub fn with_notice(mut self, level: Level, text: impl Into<String>) -> Self {
        self.notices.push(Notice {
            level,
            text: text.into(),
        });
        self
    }
}

/// Paramètres saisis dans l'interface pour un indicateur (sliders, listes...).
pub(crate) fn param_u64(params: &Value, key: &str, default: u64, min: u64, max: u64) -> u64 {
    params
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(min, max)
}

pub(crate) fn param_str<'a>(params: &'a Value, key: &str, default: &'a str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or(default)
}

pub async fn render(
    id: &str,
    params: &Value,
    data: &DataProvider,
) -> Result<IndicatorOutput, String> {
    match id {
        "fear_greed" => fear_greed::render(data).await,
        "halving" => halving::render(data).await,
        "onchain" => onchain::render(data).await,
        "cycle" => cycle::render(data).await,
        "sopr" => sopr::render(data).await,
        "institutional" => institutional::render(data).await,
        "volume" => volume::render(data).await,
        "vcr" => vcr::render(data).await,
        "bmi" => bmi::render(data).await,
        "bmsb" => bmsb::render(data, params).await,
        "long_short" => long_short::render(data, params).await,
        "cycle_roi" => cycle_roi::render(data).await,
        "realized_cap_utxo" => realized_cap_utxo::render(data).await,
        "net_realized_pnl" => net_realized_pnl::render(data).await,
        other => Err(format!("Indicateur inconnu : {other}")),
    }
}

/// Clôtures BTC indexées par date, pour les jointures avec les données Dune.
pub(crate) fn close_by_date(
    candles: &[crate::series::Candle],
) -> std::collections::HashMap<chrono::NaiveDate, f64> {
    candles.iter().map(|c| (c.date, c.close)).collect()
}

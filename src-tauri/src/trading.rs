//! Commandes de la section « Trading ». Elles appellent uniquement le moteur
//! de trading (`trading-engine`), séparé des indicateurs de marché
//! (`dashboard-core`) : ni données ni calculs ne sont partagés entre les deux.

use serde::Serialize;
use std::sync::Mutex;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use trading_engine::accumulation::{self, AccumulationParams, AccumulationSummary, Purchase};
use trading_engine::backtest::{self, Backtest, BacktestParams, Fill, RiskEvent, Summary};
use trading_engine::catalog::{self, ToolMeta};
use trading_engine::chart::{accumulation_chart, backtest_chart, Chart};
use trading_engine::market::today;
use trading_engine::mvrv::MvrvInfo;
use trading_engine::signals::{self, SignalParams, SignalReport};
use trading_engine::MarketFeed;

/// État du moteur de trading : ses données Poloniex et le dernier backtest.
pub struct TradingState {
    feed: MarketFeed,
    last_backtest: Mutex<Option<Backtest>>,
}

impl Default for TradingState {
    fn default() -> Self {
        Self {
            feed: MarketFeed::new(),
            last_backtest: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn list_trading_tools() -> Vec<ToolMeta> {
    catalog::tools()
}

#[tauri::command]
pub async fn trading_signals(
    state: State<'_, TradingState>,
    params: SignalParams,
) -> Result<SignalReport, String> {
    signals::report_with_feed(&state.feed, &params, today()).await
}

#[derive(Serialize)]
pub struct AccumulationView {
    chart: Chart,
    summary: AccumulationSummary,
    purchases: Vec<Purchase>,
    mvrv: Option<MvrvInfo>,
    warnings: Vec<String>,
}

/// Simule l'accumulation de BTC (achats au comptant, jamais de vente).
#[tauri::command]
pub async fn run_accumulation(
    state: State<'_, TradingState>,
    params: AccumulationParams,
) -> Result<AccumulationView, String> {
    let acc = accumulation::run_with_feed(&state.feed, &params, today()).await?;
    Ok(AccumulationView {
        chart: accumulation_chart(&acc),
        summary: acc.summary,
        purchases: acc.purchases,
        mvrv: acc.mvrv,
        warnings: acc.warnings,
    })
}

#[derive(Serialize)]
pub struct BacktestView {
    symbol: String,
    chart: Chart,
    summary: Summary,
    fills: Vec<Fill>,
    events: Vec<RiskEvent>,
    warnings: Vec<String>,
}

#[tauri::command]
pub async fn run_backtest(
    state: State<'_, TradingState>,
    params: BacktestParams,
) -> Result<BacktestView, String> {
    let bt = backtest::run_with_feed(&state.feed, &params).await?;
    let view = BacktestView {
        symbol: bt.symbol.clone(),
        chart: backtest_chart(&bt),
        summary: bt.summary(),
        fills: bt.fills.clone(),
        events: bt.events.clone(),
        warnings: bt.warnings.clone(),
    };
    *state.last_backtest.lock().unwrap() = Some(bt);
    Ok(view)
}

/// Exporte le dernier backtest en CSV (`equity` : courbe de capital, `fills` :
/// ordres). Renvoie le chemin enregistré, ou `None` si l'utilisateur a annulé.
#[tauri::command]
pub async fn export_backtest(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
    format: String,
) -> Result<Option<String>, String> {
    let (content, name) = {
        let guard = state.last_backtest.lock().unwrap();
        let bt = guard.as_ref().ok_or("Aucun backtest à exporter.")?;
        let day = today().format("%Y%m%d");
        match format.as_str() {
            "equity" => (bt.to_csv(), format!("backtest_{}_{day}.csv", bt.symbol)),
            "fills" => (
                bt.fills_csv(),
                format!("backtest_{}_ordres_{day}.csv", bt.symbol),
            ),
            other => return Err(format!("Format inconnu : {other}")),
        }
    };
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter("CSV", &["csv"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Impossible d'écrire {} : {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

//! Commandes de la section « Trading ». Elles appellent uniquement le moteur
//! de trading (`trading-engine`), séparé des indicateurs de marché
//! (`dashboard-core`) : ni données ni calculs ne sont partagés entre les deux.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use trading_engine::accumulation::{self, AccumulationParams, AccumulationSummary, Purchase};
use trading_engine::backtest::{self, Backtest, BacktestParams, Fill, RiskEvent, Summary};
use trading_engine::candle_store::CandleStore;
use trading_engine::catalog::{self, ToolMeta};
use trading_engine::chart::{accumulation_chart, backtest_chart, Chart};
use trading_engine::contract::ContractSpec;
use trading_engine::fibo::FiboParams;
use trading_engine::fibo_data::{self, DataStatus, SyncProgress};
use trading_engine::fibo_report::{self, ExcursionReport, FiboReport, FiboRequest, PointsReport};
use trading_engine::market::today;
use trading_engine::mvrv::MvrvInfo;
use trading_engine::signals::{self, SignalParams, SignalReport};
use trading_engine::MarketFeed;

/// État du moteur de trading : ses données Poloniex, le dernier backtest et la
/// dernière étude Fibonacci à levier.
pub struct TradingState {
    feed: MarketFeed,
    last_backtest: Mutex<Option<Backtest>>,
    /// Annulation du téléchargement ou du calcul Fibonacci en cours.
    cancel: Arc<AtomicBool>,
    /// Une seule opération longue à la fois (téléchargement ou calcul).
    busy: Arc<AtomicBool>,
    last_fibo: Mutex<Option<(FiboRequest, FiboReport)>>,
}

impl Default for TradingState {
    fn default() -> Self {
        Self {
            feed: MarketFeed::new(),
            last_backtest: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
            last_fibo: Mutex::new(None),
        }
    }
}

/// Marque l'état « occupé » et le relâche à la sortie, même sur erreur.
struct BusyGuard(Arc<AtomicBool>);

impl BusyGuard {
    fn acquire(state: &TradingState) -> Result<Self, String> {
        if state.busy.swap(true, Ordering::SeqCst) {
            return Err(
                "Un téléchargement ou un calcul est déjà en cours : annulez-le ou attendez sa fin."
                    .into(),
            );
        }
        state.cancel.store(false, Ordering::SeqCst);
        Ok(Self(state.busy.clone()))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Cache disque des bougies 1 minute, dans le dossier de données de l'application.
fn candle_store(app: &tauri::AppHandle) -> Result<CandleStore, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Dossier de données introuvable : {e}"))?;
    Ok(CandleStore::new(dir.join("fibo-candles")))
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

// ---------- Fibonacci à levier ----------

#[derive(Serialize)]
pub struct FiboOverview {
    contract: Option<ContractSpec>,
    contract_error: Option<String>,
    status: DataStatus,
}

/// Paramètres du contrat (lus sur l'API) et état du cache disque.
#[tauri::command]
pub async fn fibo_overview(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
) -> Result<FiboOverview, String> {
    let store = candle_store(&app)?;
    let (contract, contract_error) = match state.feed.contract("BTC", &store).await {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e)),
    };
    Ok(FiboOverview {
        contract,
        contract_error,
        status: fibo_data::data_status(&store, "BTC")?,
    })
}

#[derive(Serialize)]
pub struct SyncOutcome {
    warnings: Vec<String>,
    cancelled: bool,
    status: DataStatus,
}

/// Télécharge les bougies 1 minute manquantes pour la période de la demande
/// (reprenable, annulable). La progression arrive par l'événement `fibo-progress`.
#[tauri::command]
pub async fn fibo_sync(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
    request: FiboRequest,
) -> Result<SyncOutcome, String> {
    let _busy = BusyGuard::acquire(&state)?;
    let store = candle_store(&app)?;
    let emit = |p: SyncProgress| {
        let _ = app.emit("fibo-progress", &p);
    };
    let want = fibo_data::DataRange {
        base: &request.base,
        from: request.data_start_ms(),
        to: request.data_end_ms(),
        use_mark: request.use_mark_price,
    };
    let warnings = fibo_data::ensure_data(&state.feed, &store, want, &emit, &state.cancel).await?;
    Ok(SyncOutcome {
        warnings,
        cancelled: state.cancel.load(Ordering::SeqCst),
        status: fibo_data::data_status(&store, &request.base)?,
    })
}

/// Demande l'arrêt du téléchargement ou du calcul en cours.
#[tauri::command]
pub fn fibo_cancel(state: State<'_, TradingState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Étude complète : périodes d'étude et de test, placebo, Monte-Carlo, verdict.
/// Utilise le cache disque (voir `fibo_sync`). Le calcul tourne hors du fil de l'interface.
#[tauri::command]
pub async fn fibo_run(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
    request: FiboRequest,
) -> Result<FiboReport, String> {
    let _busy = BusyGuard::acquire(&state)?;
    let store = candle_store(&app)?;
    let data = fibo_report::load_data(&state.feed, &store, &request).await?;
    let (req, cancel, handle) = (request.clone(), state.cancel.clone(), app.clone());
    let report = tauri::async_runtime::spawn_blocking(move || {
        let progress = |phase: &str, done: usize, total: usize| {
            let _ = handle.emit(
                "fibo-progress",
                &SyncProgress {
                    phase: phase.to_string(),
                    done,
                    total,
                    bars: 0,
                    at: String::new(),
                },
            );
        };
        fibo_report::build_report(&req, &data, &progress, &cancel)
    })
    .await
    .map_err(|e| format!("Le calcul a échoué : {e}"))??;
    *state.last_fibo.lock().unwrap() = Some((request, report.clone()));
    Ok(report)
}

/// Zoom sur un trade de la dernière étude (`period` : « study » ou « test »).
#[tauri::command]
pub async fn fibo_zoom(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
    period: String,
    index: usize,
) -> Result<Chart, String> {
    let trade = {
        let guard = state.last_fibo.lock().unwrap();
        let (_, report) = guard.as_ref().ok_or("Aucune étude à zoomer.")?;
        let trades = if period == "study" {
            &report.study.trades
        } else {
            &report.test.trades
        };
        trades.get(index).cloned().ok_or("Trade introuvable.")?
    };
    fibo_report::trade_zoom(&candle_store(&app)?, "BTC", &trade)
}

/// Panneau « Points actuels » : dernier mouvement confirmé et état de ses niveaux.
#[tauri::command]
pub async fn fibo_points(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
    params: FiboParams,
    prudent_tick: bool,
) -> Result<PointsReport, String> {
    let store = candle_store(&app)?;
    fibo_report::load_points(&state.feed, &store, "BTC", &params, prudent_tick, today()).await
}

/// Contrôle des repères du 01/05 au 12/10/2026 (part des jours où le prix
/// s'éloigne de l'ouverture d'une distance donnée).
#[tauri::command]
pub async fn fibo_excursions(state: State<'_, TradingState>) -> Result<ExcursionReport, String> {
    fibo_report::load_excursions(&state.feed).await
}

/// Exporte les trades de la dernière étude en CSV. Renvoie le chemin enregistré,
/// ou `None` si l'utilisateur a annulé.
#[tauri::command]
pub async fn fibo_export(
    app: tauri::AppHandle,
    state: State<'_, TradingState>,
) -> Result<Option<String>, String> {
    let content = {
        let guard = state.last_fibo.lock().unwrap();
        let (_, report) = guard.as_ref().ok_or("Aucune étude à exporter.")?;
        report.trades_csv()
    };
    let name = format!("fibonacci_levier_{}.csv", today().format("%Y%m%d"));
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

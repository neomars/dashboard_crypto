// Pas de console supplémentaire sous Windows en version release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dashboard_core::catalog::{self, IndicatorMeta};
use dashboard_core::config::Config;
use dashboard_core::figure::Figure;
use dashboard_core::indicators::{self, IndicatorOutput};
use dashboard_core::simulator::{self, SimParams, Simulation, Summary, Trade};
use dashboard_core::{pdf, DataProvider};
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

/// Requêtes Dune dont l'ID est modifiable depuis l'accueil (nom, valeur par défaut).
const DUNE_QUERIES: [(&str, &str); 5] = [
    ("sopr", "6764134"),
    ("institutional", "3382000"),
    ("long_short", "3089944"),
    ("realized_cap_utxo", "7611528"),
    ("net_realized_pnl", ""),
];

struct AppState {
    data: DataProvider,
    last_simulation: Mutex<Option<Simulation>>,
}

#[tauri::command]
fn list_indicators() -> Vec<IndicatorMeta> {
    catalog::indicators()
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
async fn render_indicator(
    state: State<'_, AppState>,
    id: String,
    params: Option<Value>,
) -> Result<IndicatorOutput, String> {
    indicators::render(&id, &params.unwrap_or(Value::Null), &state.data).await
}

#[derive(Serialize)]
struct SimulationView {
    figure: Figure,
    summary: Summary,
    trades: Vec<Trade>,
}

#[tauri::command]
async fn run_simulation(
    state: State<'_, AppState>,
    params: SimParams,
) -> Result<SimulationView, String> {
    params.validate()?;
    let candles = state
        .data
        .ticker_range(&params.ticker, Some(params.start), Some(params.end))
        .await?;
    let prices: Vec<_> = candles
        .iter()
        .map(|c| dashboard_core::series::PricePoint {
            date: c.date,
            close: c.close,
        })
        .collect();
    let sim = simulator::simulate(&prices, &params)?;
    let view = SimulationView {
        figure: sim.figure(),
        summary: sim.summary(),
        trades: sim.trades.clone(),
    };
    *state.last_simulation.lock().unwrap() = Some(sim);
    Ok(view)
}

/// Exporte la dernière simulation (CSV ou PDF) à l'emplacement choisi.
/// Renvoie le chemin enregistré, ou `None` si l'utilisateur a annulé.
#[tauri::command]
async fn export_simulation(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    format: String,
) -> Result<Option<String>, String> {
    let (bytes, name, filter) = {
        let guard = state.last_simulation.lock().unwrap();
        let sim = guard.as_ref().ok_or("Aucune simulation à exporter.")?;
        let ticker = sim
            .params
            .ticker
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "_");
        match format.as_str() {
            "csv" => (
                sim.to_csv().into_bytes(),
                format!(
                    "simulation_{ticker}_{}.csv",
                    chrono::Local::now().format("%Y%m%d")
                ),
                "csv",
            ),
            "pdf" => (
                pdf::simulation_report(sim),
                format!("rapport_simulation_{ticker}.pdf"),
                "pdf",
            ),
            other => return Err(format!("Format inconnu : {other}")),
        }
    };
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter(filter.to_uppercase(), &[filter])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes)
        .map_err(|e| format!("Impossible d'écrire {} : {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

#[derive(Serialize)]
struct QuerySetting {
    name: &'static str,
    value: String,
    default: &'static str,
}

#[derive(Serialize)]
struct Settings {
    config_path: String,
    dune_api_key: String,
    queries: Vec<QuerySetting>,
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    let cfg = state.data.config();
    Settings {
        config_path: cfg.path().display().to_string(),
        dune_api_key: cfg.dune_api_key(),
        queries: DUNE_QUERIES
            .iter()
            .map(|(name, default)| QuerySetting {
                name,
                value: cfg.get("DUNE_QUERIES", name),
                default,
            })
            .collect(),
    }
}

#[tauri::command]
fn save_dune_api_key(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state.data.config().save_dune_api_key(&key)?;
    state.data.clear_cache();
    Ok(())
}

#[tauri::command]
fn delete_dune_api_key(state: State<'_, AppState>) -> Result<(), String> {
    state.data.config().delete_dune_api_key()?;
    state.data.clear_cache();
    Ok(())
}

/// Enregistre (ou efface si vide) l'ID d'une requête Dune.
#[tauri::command]
fn save_query_id(state: State<'_, AppState>, name: String, value: String) -> Result<(), String> {
    if !DUNE_QUERIES.iter().any(|(n, _)| *n == name) {
        return Err(format!("Requête inconnue : {name}"));
    }
    let value = value.trim();
    if !value.is_empty() && !value.chars().all(|c| c.is_ascii_digit()) {
        return Err("L'ID d'une requête Dune est un nombre (ex. 1234567).".into());
    }
    let cfg = state.data.config();
    if value.is_empty() {
        cfg.delete("DUNE_QUERIES", &name)?
    } else {
        cfg.set("DUNE_QUERIES", &name, value)?
    }
    state.data.clear_cache();
    Ok(())
}

#[tauri::command]
fn clear_cache(state: State<'_, AppState>) {
    state.data.clear_cache();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            data: DataProvider::new(Config::default_location()),
            last_simulation: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            list_indicators,
            app_version,
            render_indicator,
            run_simulation,
            export_simulation,
            get_settings,
            save_dune_api_key,
            delete_dune_api_key,
            save_query_id,
            clear_cache
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application");
}

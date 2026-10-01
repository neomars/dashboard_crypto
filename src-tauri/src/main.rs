// Pas de console supplémentaire sous Windows en version release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dashboard_core::catalog::{self, IndicatorMeta};
use dashboard_core::config::Config;
use dashboard_core::figure::Figure;
use dashboard_core::indicators::{self, mstr_mnav, IndicatorOutput};
use dashboard_core::simulator::{self, SimParams, Simulation, Summary, Trade};
use dashboard_core::{bgeometrics, pdf, DataProvider};
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

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
    /// Nom de la valeur simulée (BTC, ETH, AAPL...), pour les textes de l'interface.
    unit: String,
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
        unit: params.unit(),
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
struct EndpointSetting {
    key: &'static str,
    label: &'static str,
    /// Nom imposé dans la configuration (vide = détection automatique).
    value: String,
    candidates: &'static [&'static str],
}

#[derive(Serialize)]
struct MstrHoldingsSetting {
    date: String,
    btc: f64,
    shares: f64,
}

#[derive(Serialize)]
struct Settings {
    config_path: String,
    endpoints: Vec<EndpointSetting>,
    /// Dernière donnée de holdings MSTR retenue (fichier intégré + config.ini).
    mstr: Option<MstrHoldingsSetting>,
    /// Contact envoyé à la SEC (vide = User-Agent par défaut).
    sec_user_agent: String,
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    let cfg = state.data.config();
    Settings {
        config_path: cfg.path().display().to_string(),
        endpoints: bgeometrics::METRICS
            .iter()
            .map(|m| EndpointSetting {
                key: m.key,
                label: m.label,
                value: cfg.bgeometrics_endpoint(m.key),
                candidates: m.candidates,
            })
            .collect(),
        mstr: mstr_mnav::latest_holdings(cfg).map(|h| MstrHoldingsSetting {
            date: h.date.format("%d/%m/%Y").to_string(),
            btc: h.btc,
            shares: h.shares,
        }),
        sec_user_agent: cfg.sec_user_agent(),
    }
}

/// Enregistre (ou, si vide, retire) le contact envoyé à la SEC.
#[tauri::command]
fn save_sec_user_agent(state: State<'_, AppState>, value: String) -> Result<(), String> {
    let value = value.trim();
    if !value.is_empty() && !value.contains('@') {
        return Err("Adresse e-mail invalide (ex. jean.dupont@exemple.fr).".into());
    }
    if value.chars().any(char::is_control) {
        return Err("Le contact ne doit pas contenir de retour à la ligne.".into());
    }
    state.data.config().set_sec_user_agent(value)?;
    state.data.clear_cache();
    Ok(())
}

/// Impose (ou, si vide, retire) le nom d'endpoint BGeometrics d'une métrique.
#[tauri::command]
fn save_endpoint(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    if bgeometrics::metric(&key).is_none() {
        return Err(format!("Métrique inconnue : {key}"));
    }
    let value = value.trim();
    if !value.is_empty() && !bgeometrics::is_valid_endpoint(value) {
        return Err("Un nom d'endpoint ne contient que des minuscules, des chiffres et des tirets (ex. sth-sopr).".into());
    }
    state.data.config().set_bgeometrics_endpoint(&key, value)?;
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
            save_endpoint,
            save_sec_user_agent,
            clear_cache
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application");
}

//! Configuration utilisateur (`config.ini`) : noms d'endpoint BGeometrics
//! imposés (sinon détectés automatiquement) et derniers réglages du simulateur.
//!
//! Emplacement : variable `DASHBOARD_CRYPTO_CONFIG`, sinon
//! `~/.config/dashboard-crypto/config.ini` sous Linux,
//! `%APPDATA%\dashboard-crypto\config.ini` sous Windows (même fichier que les versions
//! précédentes ; leurs anciennes sections Dune sont ignorées).

use crate::simulator::{ExitFrequency, SimParams};
use chrono::NaiveDate;
use ini::Ini;
use std::path::{Path, PathBuf};

/// Section des derniers réglages du simulateur d'investissement.
const SIMULATOR: &str = "SIMULATOR";
/// Valeur de `end` qui suit la date du jour au lieu de figer celle de l'enregistrement.
const END_TODAY: &str = "today";

#[derive(Debug, Clone)]
pub struct Config {
    path: PathBuf,
}

impl Config {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_location() -> Self {
        if let Some(p) = std::env::var_os("DASHBOARD_CRYPTO_CONFIG") {
            return Self::new(p);
        }
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Self::new(base.join("dashboard-crypto").join("config.ini"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Ini {
        Ini::load_from_file(&self.path).unwrap_or_default()
    }

    pub fn get(&self, section: &str, key: &str) -> String {
        let ini = self.load();
        ini.get_from(Some(section), key)
            .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
            .unwrap_or_default()
    }

    /// Toutes les entrées (clé, valeur) d'une section, dans l'ordre du fichier.
    pub fn section(&self, section: &str) -> Vec<(String, String)> {
        let ini = self.load();
        ini.section(Some(section))
            .map(|s| {
                s.iter()
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set(&self, section: &str, key: &str, value: &str) -> Result<(), String> {
        let mut ini = self.load();
        ini.with_section(Some(section)).set(key, value);
        self.save(&ini)
    }

    pub fn delete(&self, section: &str, key: &str) -> Result<(), String> {
        if !self.path.exists() {
            return Ok(());
        }
        let mut ini = self.load();
        ini.delete_from(Some(section), key);
        if ini.section(Some(section)).is_some_and(|s| s.is_empty()) {
            ini.delete(Some(section));
        }
        self.save(&ini)
    }

    fn save(&self, ini: &Ini) -> Result<(), String> {
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Impossible de créer {} : {e}", dir.display()))?;
        }
        ini.write_to_file(&self.path)
            .map_err(|e| format!("Impossible d'écrire {} : {e}", self.path.display()))
    }

    /// Contact envoyé à la SEC dans le User-Agent (vide = valeur par défaut).
    pub fn sec_user_agent(&self) -> String {
        self.get("SEC", "user_agent")
    }

    /// Enregistre (ou, si vide, retire) le contact SEC.
    pub fn set_sec_user_agent(&self, value: &str) -> Result<(), String> {
        let value = value.trim();
        if value.is_empty() {
            self.delete("SEC", "user_agent")
        } else {
            self.set("SEC", "user_agent", value)
        }
    }

    /// Derniers réglages du simulateur, section `[SIMULATOR]`. Une valeur absente ou
    /// illisible reprend celle par défaut ; des réglages incohérents (ex. début après la
    /// fin, après une édition à la main) sont abandonnés en bloc.
    pub fn simulator_params(&self) -> SimParams {
        let ini = self.load();
        let get = |key: &str| ini.get_from(Some(SIMULATOR), key).map(str::trim);
        let number = |key: &str| {
            get(key)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        let date = |key: &str| get(key).and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok());

        let default = SimParams::default();
        let params = SimParams {
            start: date("start").unwrap_or(default.start),
            end: if get("end") == Some(END_TODAY) {
                default.end
            } else {
                date("end").unwrap_or(default.end)
            },
            initial_capital: number("initial_capital").unwrap_or(default.initial_capital),
            drop_pct: number("drop_pct").unwrap_or(default.drop_pct),
            target_leverage: number("target_leverage").unwrap_or(default.target_leverage),
            exit_frequency: get("exit_frequency")
                .and_then(ExitFrequency::from_label)
                .unwrap_or(default.exit_frequency),
            exit_pct: number("exit_pct").unwrap_or(default.exit_pct),
            ticker: get("ticker")
                .filter(|t| !t.is_empty())
                .map_or(default.ticker.clone(), str::to_string),
        };
        if params.validate().is_ok() {
            params
        } else {
            default
        }
    }

    /// Mémorise les réglages du simulateur. Une date de fin qui est le jour même est
    /// enregistrée comme « aujourd'hui » : au prochain lancement elle suit le calendrier.
    pub fn set_simulator_params(&self, p: &SimParams) -> Result<(), String> {
        let end = if p.end >= SimParams::default().end {
            END_TODAY.to_string()
        } else {
            p.end.format("%Y-%m-%d").to_string()
        };
        let mut ini = self.load();
        ini.with_section(Some(SIMULATOR))
            .set("ticker", p.ticker.trim())
            .set("start", p.start.format("%Y-%m-%d").to_string())
            .set("end", end)
            .set("initial_capital", p.initial_capital.to_string())
            .set("target_leverage", p.target_leverage.to_string())
            .set("drop_pct", p.drop_pct.to_string())
            .set("exit_frequency", p.exit_frequency.label())
            .set("exit_pct", p.exit_pct.to_string());
        self.save(&ini)
    }

    /// Nom d'endpoint BGeometrics imposé pour une métrique : variable
    /// `BGEOMETRICS_<CLÉ>` > section `[BGEOMETRICS]` > vide (détection automatique).
    pub fn bgeometrics_endpoint(&self, key: &str) -> String {
        if let Ok(v) = std::env::var(format!("BGEOMETRICS_{}", key.to_uppercase())) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
        self.get("BGEOMETRICS", key)
    }

    /// Impose (ou, si vide, retire) le nom d'endpoint d'une métrique BGeometrics.
    pub fn set_bgeometrics_endpoint(&self, key: &str, endpoint: &str) -> Result<(), String> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            self.delete("BGEOMETRICS", key)
        } else {
            self.set("BGEOMETRICS", key, endpoint)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str) -> Config {
        let dir = std::env::temp_dir().join(format!(
            "dashboard-crypto-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Config::new(dir.join("sub").join("config.ini"))
    }

    #[test]
    fn section_lists_entries_in_file_order() {
        let cfg = temp_config("section");
        assert!(cfg.section("MSTR").is_empty());
        std::fs::create_dir_all(cfg.path().parent().unwrap()).unwrap();
        std::fs::write(
            cfg.path(),
            "[MSTR]\nseuils = 1.0, 1.5, 2.5\n2026-10-05 = 850000, 390000000\n",
        )
        .unwrap();
        assert_eq!(
            cfg.section("MSTR"),
            vec![
                ("seuils".to_string(), "1.0, 1.5, 2.5".to_string()),
                ("2026-10-05".to_string(), "850000, 390000000".to_string())
            ]
        );
    }

    #[test]
    fn endpoint_override_roundtrip() {
        let cfg = temp_config("endpoint");
        assert_eq!(cfg.bgeometrics_endpoint("etf"), "");
        cfg.set_bgeometrics_endpoint("etf", " etf-btc ").unwrap();
        assert_eq!(cfg.bgeometrics_endpoint("etf"), "etf-btc");
        cfg.set_bgeometrics_endpoint("etf", "").unwrap();
        assert_eq!(cfg.bgeometrics_endpoint("etf"), "");
        assert!(!std::fs::read_to_string(cfg.path())
            .unwrap()
            .contains("BGEOMETRICS"));
    }

    #[test]
    fn sec_user_agent_roundtrip() {
        let cfg = temp_config("sec");
        assert_eq!(cfg.sec_user_agent(), "");
        cfg.set_sec_user_agent(" Jean Dupont jean@exemple.fr ")
            .unwrap();
        assert_eq!(cfg.sec_user_agent(), "Jean Dupont jean@exemple.fr");
        cfg.set_sec_user_agent("").unwrap();
        assert_eq!(cfg.sec_user_agent(), "");
    }

    fn ymd(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn simulator_params_default_without_file() {
        let cfg = temp_config("sim-default");
        let p = cfg.simulator_params();
        assert_eq!(p.ticker, "BTC-USD");
        assert_eq!(p.end, SimParams::default().end);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn simulator_params_roundtrip() {
        let cfg = temp_config("sim-roundtrip");
        let saved = SimParams {
            start: ymd("2019-03-15"),
            end: ymd("2024-12-31"),
            initial_capital: 25_000.5,
            drop_pct: 7.5,
            target_leverage: 3.0,
            exit_frequency: ExitFrequency::Monthly,
            exit_pct: 20.0,
            ticker: " ETH-USD ".into(),
        };
        cfg.set_simulator_params(&saved).unwrap();
        let loaded = cfg.simulator_params();
        assert_eq!(loaded.ticker, "ETH-USD");
        assert_eq!(loaded.start, saved.start);
        assert_eq!(loaded.end, saved.end);
        assert_eq!(loaded.initial_capital, 25_000.5);
        assert_eq!(loaded.drop_pct, 7.5);
        assert_eq!(loaded.target_leverage, 3.0);
        assert_eq!(loaded.exit_frequency, ExitFrequency::Monthly);
        assert_eq!(loaded.exit_pct, 20.0);
    }

    #[test]
    fn simulator_end_today_follows_the_calendar() {
        let cfg = temp_config("sim-today");
        cfg.set_simulator_params(&SimParams::default()).unwrap();
        let file = std::fs::read_to_string(cfg.path()).unwrap();
        assert!(file.contains("end=today"), "{file}");
        // Un fichier écrit un autre jour garde « today » : la fin est la date du jour.
        assert_eq!(cfg.simulator_params().end, SimParams::default().end);
    }

    #[test]
    fn simulator_params_keep_other_sections() {
        let cfg = temp_config("sim-other");
        cfg.set_sec_user_agent("jean@exemple.fr").unwrap();
        cfg.set_simulator_params(&SimParams::default()).unwrap();
        assert_eq!(cfg.sec_user_agent(), "jean@exemple.fr");
    }

    #[test]
    fn simulator_params_ignore_unreadable_values() {
        let cfg = temp_config("sim-bad");
        std::fs::create_dir_all(cfg.path().parent().unwrap()).unwrap();
        std::fs::write(
            cfg.path(),
            "[SIMULATOR]\nticker = SOL-USD\nstart = pas une date\ndrop_pct = NaN\ntarget_leverage = 4\nexit_frequency = Jamais\n",
        )
        .unwrap();
        let p = cfg.simulator_params();
        let default = SimParams::default();
        assert_eq!(p.ticker, "SOL-USD");
        assert_eq!(p.target_leverage, 4.0);
        assert_eq!(p.start, default.start);
        assert_eq!(p.drop_pct, default.drop_pct);
        assert_eq!(p.exit_frequency, default.exit_frequency);
    }

    #[test]
    fn simulator_params_inconsistent_file_falls_back_to_default() {
        let cfg = temp_config("sim-inconsistent");
        std::fs::create_dir_all(cfg.path().parent().unwrap()).unwrap();
        std::fs::write(
            cfg.path(),
            "[SIMULATOR]\nticker = SOL-USD\nstart = 2025-01-01\nend = 2020-01-01\n",
        )
        .unwrap();
        assert_eq!(cfg.simulator_params().ticker, SimParams::default().ticker);
    }

    #[test]
    fn reads_quoted_values_and_ignores_old_sections() {
        let cfg = temp_config("quoted");
        std::fs::create_dir_all(cfg.path().parent().unwrap()).unwrap();
        std::fs::write(
            cfg.path(),
            "[DUNE]\napi_key = xyz\n\n[BGEOMETRICS]\nnrpl = \"nrpl-usd\"\n",
        )
        .unwrap();
        assert_eq!(cfg.bgeometrics_endpoint("nrpl"), "nrpl-usd");
        assert_eq!(cfg.bgeometrics_endpoint("sth_sopr"), "");
    }
}

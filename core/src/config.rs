//! Configuration utilisateur (`config.ini`) : noms d'endpoint BGeometrics
//! imposés (sinon détectés automatiquement).
//!
//! Emplacement : variable `DASHBOARD_CRYPTO_CONFIG`, sinon
//! `~/.config/dashboard-crypto/config.ini` (même fichier que les versions
//! précédentes ; leurs anciennes sections Dune sont ignorées).

use ini::Ini;
use std::path::{Path, PathBuf};

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

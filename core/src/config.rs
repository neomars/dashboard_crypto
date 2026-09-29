//! Configuration utilisateur (`config.ini`) : clé API Dune et IDs de requêtes.
//!
//! Emplacement : variable `DASHBOARD_CRYPTO_CONFIG`, sinon
//! `~/.config/dashboard-crypto/config.ini` (même fichier que la version Python
//! installée par paquet, donc une clé déjà enregistrée est reprise).

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

    pub fn dune_api_key(&self) -> String {
        self.get("DUNE", "api_key")
    }

    pub fn save_dune_api_key(&self, key: &str) -> Result<(), String> {
        self.set("DUNE", "api_key", key.trim())
    }

    pub fn delete_dune_api_key(&self) -> Result<(), String> {
        self.delete("DUNE", "api_key")
    }

    /// ID d'une requête Dune : variable `DUNE_QUERY_<NOM>` > section
    /// `[DUNE_QUERIES]` > valeur par défaut (vide = aucune).
    pub fn dune_query_id(&self, name: &str, default: &str) -> String {
        if let Ok(v) = std::env::var(format!("DUNE_QUERY_{}", name.to_uppercase())) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
        let v = self.get("DUNE_QUERIES", name);
        if v.is_empty() {
            default.to_string()
        } else {
            v
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
    fn save_read_and_delete_key() {
        let cfg = temp_config("key");
        assert_eq!(cfg.dune_api_key(), "");
        cfg.save_dune_api_key("  abc123 ").unwrap();
        assert_eq!(cfg.dune_api_key(), "abc123");
        cfg.delete_dune_api_key().unwrap();
        assert_eq!(cfg.dune_api_key(), "");
        assert!(!std::fs::read_to_string(cfg.path())
            .unwrap()
            .contains("DUNE"));
    }

    #[test]
    fn reads_python_style_quoted_values() {
        let cfg = temp_config("quoted");
        std::fs::create_dir_all(cfg.path().parent().unwrap()).unwrap();
        std::fs::write(
            cfg.path(),
            "[DUNE]\napi_key = \"xyz\"\n\n[DUNE_QUERIES]\nsopr = 42\n",
        )
        .unwrap();
        assert_eq!(cfg.dune_api_key(), "xyz");
        assert_eq!(cfg.dune_query_id("sopr", "1"), "42");
        assert_eq!(cfg.dune_query_id("institutional", "3382000"), "3382000");
        assert_eq!(cfg.dune_query_id("net_realized_pnl", ""), "");
    }
}

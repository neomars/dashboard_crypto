//! Pages de la section « Trading » de la barre latérale (`tools.json`),
//! distinctes de la liste des indicateurs de marché (`core/indicators.json`).

use serde::{Deserialize, Serialize};

/// Identifiants des pages servies par le moteur.
pub const IDS: [&str; 4] = [
    "btc_accumulation",
    "fibo_leverage",
    "trend_signals",
    "trend_backtest",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMeta {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub default_scale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
}

pub fn tools() -> Vec<ToolMeta> {
    serde_json::from_str(include_str!("../tools.json")).expect("tools.json valide")
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_tool_is_served() {
        let tools = super::tools();
        assert_eq!(tools.len(), super::IDS.len());
        for t in tools {
            assert!(
                super::IDS.contains(&t.id.as_str()),
                "page inconnue : {}",
                t.id
            );
        }
    }
}

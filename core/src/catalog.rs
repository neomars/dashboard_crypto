//! Liste des indicateurs affichés dans la barre latérale (`indicators.json`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorMeta {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub default_scale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
    #[serde(default)]
    pub is_special: bool,
}

pub fn indicators() -> Vec<IndicatorMeta> {
    serde_json::from_str(include_str!("../indicators.json")).expect("indicators.json valide")
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_indicator_has_a_renderer() {
        for ind in super::indicators() {
            assert!(
                ind.is_special || crate::indicators::IDS.contains(&ind.id.as_str()),
                "pas de rendu pour {}",
                ind.id
            );
        }
    }
}

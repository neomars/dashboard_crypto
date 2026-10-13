//! Paramètres du contrat perpétuel (levier maximal, taille de contrat, marge de
//! maintenance), frais, taille de position en contrats entiers et prix de
//! liquidation en marge isolée.
//!
//! Les paramètres du contrat sont lus sur l'API publique de Poloniex
//! (`/v3/market/allInstruments`, et `/v3/market/riskLimit` pour les paliers de
//! marge de maintenance) : rien n'est codé en dur, sauf les frais, qui ne sont
//! pas publics (grille de frais propre au compte) et restent des défauts
//! modifiables signalés « à vérifier ».

use crate::market::Instrument;
use crate::poloniex::{num, v3_data};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Sens d'une position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Long,
    Short,
}

impl Side {
    /// +1 pour un achat, −1 pour une vente.
    pub fn sign(self) -> f64 {
        match self {
            Side::Long => 1.0,
            Side::Short => -1.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Long => "Achat",
            Side::Short => "Vente",
        }
    }

    pub fn opposite(self) -> Side {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
        }
    }
}

/// Palier de marge de maintenance : `rate` s'applique jusqu'à `up_to_contracts`
/// contrats (`None` : sans limite).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceTier {
    pub up_to_contracts: Option<f64>,
    pub rate: f64,
}

/// Fiche du contrat perpétuel, telle que lue sur l'API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractSpec {
    pub symbol: String,
    /// Levier maximal autorisé par la plateforme.
    pub max_leverage: f64,
    /// Valeur d'un contrat, en actif de base (`ctVal`, 0,001 BTC pour BTC_USDT_PERP).
    pub contract_value: f64,
    /// Pas de quantité, en contrats (`lotSz`).
    pub lot_size: f64,
    /// Quantité minimale, en contrats (`minSz`).
    pub min_size: f64,
    /// Pas de prix (`tSz`).
    pub tick_size: f64,
    /// Marge de maintenance par palier, croissants (au moins un palier).
    pub tiers: Vec<MaintenanceTier>,
    /// Vrai si les paliers viennent de `/v3/market/riskLimit` ; faux si le taux
    /// unique de la fiche (`mM`) en tient lieu.
    pub tiers_from_risk_limit: bool,
    /// Avertissements à afficher (champs absents, format non reconnu...).
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl ContractSpec {
    /// Refuse tout levier au-delà du maximum de la plateforme.
    pub fn check_leverage(&self, leverage: f64) -> Result<(), String> {
        if !leverage.is_finite() || leverage < 1.0 {
            return Err("Le levier doit être au moins 1.".into());
        }
        if leverage > self.max_leverage + 1e-9 {
            return Err(format!(
                "Levier x{leverage} refusé : le maximum de {} est x{}.",
                self.symbol, self.max_leverage
            ));
        }
        Ok(())
    }

    /// Taux de marge de maintenance d'une position de `contracts` contrats.
    pub fn maintenance_rate(&self, contracts: f64) -> f64 {
        self.tiers
            .iter()
            .find(|t| t.up_to_contracts.is_none_or(|limit| contracts <= limit))
            .or(self.tiers.last())
            .map_or(0.0, |t| t.rate)
    }

    /// Nombre entier de contrats pour une marge isolée `margin` (USDT) au levier
    /// `leverage`, au prix `price` : montant engagé = marge × levier, arrondi
    /// vers le bas au pas de quantité. `None` si c'est moins que la quantité
    /// minimale (un contrat).
    pub fn contracts_for(&self, margin: f64, leverage: f64, price: f64) -> Option<f64> {
        if !(margin > 0.0 && leverage >= 1.0 && price > 0.0 && self.contract_value > 0.0) {
            return None;
        }
        let raw = margin * leverage / (price * self.contract_value);
        // Tolérance : 12,000000000001 contrats doivent valoir 12.
        let n = ((raw / self.lot_size) + 1e-9).floor() * self.lot_size;
        (n >= self.min_size && n > 0.0).then_some(n)
    }

    /// Quantité d'actif de base pour `contracts` contrats.
    pub fn quantity(&self, contracts: f64) -> f64 {
        contracts * self.contract_value
    }

    /// Arrondit un prix au pas de prix le plus proche.
    pub fn round_price(&self, price: f64) -> f64 {
        if self.tick_size > 0.0 {
            (price / self.tick_size).round() * self.tick_size
        } else {
            price
        }
    }
}

/// Prix de liquidation en marge isolée, d'après la formule générique
/// `entrée × (1 − 1/L + m)` pour un achat et `entrée × (1 + 1/L − m)` pour une
/// vente, où `m` est le taux de marge de maintenance. La formule exacte de
/// Poloniex n'est pas publiée : elle n'a pas pu être vérifiée.
///
/// `None` si `1/L ≤ m` : la position serait liquidée dès l'ouverture.
pub fn liquidation_price(
    side: Side,
    entry: f64,
    leverage: f64,
    maintenance_rate: f64,
) -> Option<f64> {
    let gap = 1.0 / leverage - maintenance_rate;
    (gap > 0.0 && entry > 0.0).then(|| entry * (1.0 - side.sign() * gap))
}

/// Frais par ordre, en % du montant échangé.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fees {
    /// Ordre limite (maker).
    pub maker_pct: f64,
    /// Ordre au marché (taker).
    pub taker_pct: f64,
}

/// Défauts : maker 0,02 %, taker 0,05 %. La grille des perpétuels n'est pas
/// publique par API (elle dépend du compte) : **à vérifier**. Des sources
/// publiques indiquent selon la date 0,01 %/0,04 % (communiqué de 2022) ou
/// 0,01 %/0,075 % (guides tiers).
pub const DEFAULT_FEES: Fees = Fees {
    maker_pct: 0.02,
    taker_pct: 0.05,
};

fn text(v: &Value) -> Option<f64> {
    num(v).filter(|x| x.is_finite())
}

/// Fiche du contrat `symbol` dans la réponse de `/v3/market/allInstruments`
/// (`{code, msg, data: [{symbol, ctVal, maxLever, lotSz, minSz, tSz, mM, ...}]}`).
pub fn parse_instrument(body: &Value, symbol: &str) -> Result<ContractSpec, String> {
    let entry = v3_data(body)?
        .iter()
        .find(|e| e["symbol"].as_str() == Some(symbol))
        .ok_or_else(|| format!("Contrat {symbol} absent de la réponse de Poloniex."))?;
    let field = |name: &str| {
        text(&entry[name])
            .ok_or_else(|| format!("Champ « {name} » absent de la fiche de {symbol}."))
    };
    let max_leverage = field("maxLever")?;
    let contract_value = field("ctVal")?;
    let maintenance = field("mM")?;
    if !(max_leverage >= 1.0 && contract_value > 0.0 && maintenance >= 0.0) {
        return Err(format!("Fiche de {symbol} incohérente."));
    }
    let mut warnings = Vec::new();
    let mut or_default = |name: &str, default: f64| {
        text(&entry[name]).filter(|x| *x > 0.0).unwrap_or_else(|| {
            warnings.push(format!(
                "Champ « {name} » absent de la fiche de {symbol} : {default} supposé."
            ));
            default
        })
    };
    let lot_size = or_default("lotSz", 1.0);
    let min_size = or_default("minSz", 1.0);
    let tick_size = or_default("tSz", 0.1);
    Ok(ContractSpec {
        symbol: symbol.to_string(),
        max_leverage,
        contract_value,
        lot_size,
        min_size,
        tick_size,
        tiers: vec![MaintenanceTier {
            up_to_contracts: None,
            rate: maintenance,
        }],
        tiers_from_risk_limit: false,
        warnings,
    })
}

/// Paliers de marge de maintenance de la réponse de `/v3/market/riskLimit`.
///
/// Le format de cette réponse n'a pas pu être vérifié : la lecture est
/// tolérante (un tableau `data` de lignes ; taux dans `mMR`, `mmr`, `mM` ou
/// `maintMarginRate` ; plafond du palier, en contrats, dans `maxSz`, `maxQty`
/// ou `mR`). Sans taux lisible, renvoie une erreur et l'appelant garde le taux
/// unique de la fiche du contrat.
pub fn parse_risk_limit(body: &Value) -> Result<Vec<MaintenanceTier>, String> {
    const RATE: [&str; 4] = ["mMR", "mmr", "mM", "maintMarginRate"];
    const SIZE: [&str; 3] = ["maxSz", "maxQty", "mR"];
    let mut tiers: Vec<MaintenanceTier> = v3_data(body)?
        .iter()
        .filter_map(|row| {
            let rate = RATE.iter().find_map(|k| text(&row[*k]))?;
            let limit = SIZE
                .iter()
                .find_map(|k| text(&row[*k]))
                .filter(|x| *x > 0.0);
            (rate > 0.0).then_some(MaintenanceTier {
                up_to_contracts: limit,
                rate,
            })
        })
        .collect();
    if tiers.is_empty() {
        return Err("Aucun palier de marge de maintenance lisible dans la réponse.".into());
    }
    tiers.sort_by(|a, b| {
        a.up_to_contracts
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.up_to_contracts.unwrap_or(f64::INFINITY))
    });
    // Le dernier palier couvre toutes les positions plus grandes.
    if let Some(last) = tiers.last_mut() {
        last.up_to_contracts = None;
    }
    Ok(tiers)
}

/// Instrument du contrat étudié.
pub fn perp_instrument(base: &str) -> Result<Instrument, String> {
    Instrument::new(base, crate::market::Market::Perp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Extrait de la documentation de la bibliothèque `ccxt` (implémentation de
    /// référence pour Poloniex) : fiche de BNB_USDT_PERP. L'API n'a pas pu être
    /// interrogée directement pour BTC_USDT_PERP.
    fn bnb_response() -> Value {
        json!({"code": "200", "msg": "Success", "data": [{
            "symbol": "BNB_USDT_PERP", "bAsset": ".PBNBUSDT", "bCcy": "BNB", "qCcy": "USDT",
            "visibleStartTime": "1620390600000", "tradableStartTime": "1620390600000",
            "sCcy": "USDT", "tSz": "0.001", "pxScale": "0.001,0.01,0.1,1,10", "lotSz": "1",
            "minSz": "1", "ctVal": "0.1", "status": "OPEN", "oDate": "1620287590000",
            "maxPx": "1000000", "minPx": "0.001", "maxQty": "1000000", "minQty": "1",
            "maxLever": "50", "lever": "10", "ctType": "LINEAR", "alias": "", "iM": "0.02",
            "mM": "0.0115", "mR": "2000", "buyLmt": "", "sellLmt": "", "ordPxRange": "0.05",
            "marketMaxQty": "2800", "limitMaxQty": "1000000"}]})
    }

    /// Fiche de BTC_USDT_PERP : mêmes champs, avec les valeurs relevées par
    /// l'utilisateur (levier maximal 75, contrat de 0,001 BTC).
    fn btc_spec() -> ContractSpec {
        let mut body = bnb_response();
        let e = &mut body["data"][0];
        e["symbol"] = json!("BTC_USDT_PERP");
        e["ctVal"] = json!("0.001");
        e["maxLever"] = json!("75");
        e["tSz"] = json!("0.1");
        e["mM"] = json!("0.005");
        parse_instrument(&body, "BTC_USDT_PERP").unwrap()
    }

    #[test]
    fn instrument_fields_are_read_from_the_api() {
        let s = parse_instrument(&bnb_response(), "BNB_USDT_PERP").unwrap();
        assert_eq!(s.max_leverage, 50.0);
        assert_eq!(s.contract_value, 0.1);
        assert_eq!((s.lot_size, s.min_size, s.tick_size), (1.0, 1.0, 0.001));
        assert_eq!(s.tiers.len(), 1);
        assert_eq!(s.tiers[0].rate, 0.0115);
        assert!(!s.tiers_from_risk_limit);
        assert!(s.warnings.is_empty());
        let btc = btc_spec();
        assert_eq!((btc.max_leverage, btc.contract_value), (75.0, 0.001));
    }

    #[test]
    fn missing_or_wrong_contract_is_an_error() {
        let err = parse_instrument(&bnb_response(), "BTC_USDT_PERP").unwrap_err();
        assert!(err.contains("absent"), "{err}");
        let mut body = bnb_response();
        body["data"][0].as_object_mut().unwrap().remove("ctVal");
        assert!(parse_instrument(&body, "BNB_USDT_PERP")
            .unwrap_err()
            .contains("ctVal"));
        let err = parse_instrument(&json!({"code": 400, "msg": "Param error"}), "X").unwrap_err();
        assert!(err.contains("Param error"));
        // Champs secondaires absents : valeurs par défaut et avertissements.
        let mut body = bnb_response();
        body["data"][0].as_object_mut().unwrap().remove("lotSz");
        let s = parse_instrument(&body, "BNB_USDT_PERP").unwrap();
        assert_eq!(s.lot_size, 1.0);
        assert!(s.warnings[0].contains("lotSz"));
    }

    #[test]
    fn leverage_above_the_maximum_is_refused() {
        let s = btc_spec();
        assert!(s.check_leverage(75.0).is_ok());
        assert!(s.check_leverage(1.0).is_ok());
        let err = s.check_leverage(76.0).unwrap_err();
        assert!(err.contains("x75"), "{err}");
        assert!(s.check_leverage(0.5).is_err());
        assert!(s.check_leverage(f64::NAN).is_err());
    }

    #[test]
    fn contracts_are_whole_numbers_rounded_down() {
        let s = btc_spec();
        // 10 USDT × 75 = 750 USDT ; un contrat vaut 0,001 × 60 000 = 60 USDT : 12,5 → 12.
        assert_eq!(s.contracts_for(10.0, 75.0, 60_000.0), Some(12.0));
        assert_eq!(s.quantity(12.0), 0.012);
        // 10 USDT × 20 = 200 USDT : 3,33 → 3 contrats.
        assert_eq!(s.contracts_for(10.0, 20.0, 60_000.0), Some(3.0));
        // Moins d'un contrat : refusé.
        assert_eq!(s.contracts_for(10.0, 5.0, 60_000.0), None);
        assert_eq!(s.contracts_for(0.0, 75.0, 60_000.0), None);
        // Exactement un nombre entier (erreur d'arrondi binaire).
        assert_eq!(s.contracts_for(6.0, 10.0, 60_000.0), Some(1.0));
        // Pas de quantité de 5 contrats.
        let mut lots = s.clone();
        lots.lot_size = 5.0;
        lots.min_size = 5.0;
        assert_eq!(lots.contracts_for(10.0, 75.0, 60_000.0), Some(10.0));
        assert_eq!(lots.contracts_for(3.0, 75.0, 60_000.0), None);
    }

    #[test]
    fn liquidation_price_in_isolated_margin() {
        // Achat à x20, maintenance 0,5 % : −(5 % − 0,5 %) = −4,5 %.
        let long = liquidation_price(Side::Long, 60_000.0, 20.0, 0.005).unwrap();
        assert!((long - 57_300.0).abs() < 1e-6, "{long}");
        let short = liquidation_price(Side::Short, 60_000.0, 20.0, 0.005).unwrap();
        assert!((short - 62_700.0).abs() < 1e-6, "{short}");
        // x75 : 1/75 − 0,5 % = 0,833 %.
        let l75 = liquidation_price(Side::Long, 60_000.0, 75.0, 0.005).unwrap();
        assert!(((60_000.0 - l75) / 60_000.0 - (1.0 / 75.0 - 0.005)).abs() < 1e-12);
        // Levier trop élevé pour la marge de maintenance : liquidation immédiate.
        assert_eq!(liquidation_price(Side::Long, 60_000.0, 200.0, 0.005), None);
    }

    #[test]
    fn risk_limit_tiers_are_read_tolerantly() {
        let body = json!({"code": 200, "msg": "Success", "data": [
            {"symbol": "BTC_USDT_PERP", "maxSz": "5000", "mMR": "0.01"},
            {"symbol": "BTC_USDT_PERP", "maxSz": "100", "mMR": "0.005"},
            {"symbol": "BTC_USDT_PERP", "maxSz": "20000", "mMR": "0.02"},
        ]});
        let tiers = parse_risk_limit(&body).unwrap();
        assert_eq!(tiers.len(), 3);
        assert_eq!(tiers[0].rate, 0.005, "triés par taille");
        assert_eq!(
            tiers[2].up_to_contracts, None,
            "le dernier palier est sans limite"
        );
        let mut s = btc_spec();
        s.tiers = tiers;
        assert_eq!(s.maintenance_rate(12.0), 0.005);
        assert_eq!(s.maintenance_rate(101.0), 0.01);
        assert_eq!(s.maintenance_rate(1e6), 0.02);
        assert!(parse_risk_limit(&json!({"code": 200, "data": [{"foo": 1}]})).is_err());
    }

    #[test]
    fn prices_round_to_the_tick() {
        let s = btc_spec();
        assert!((s.round_price(60_000.04) - 60_000.0).abs() < 1e-9);
        assert!((s.round_price(60_000.06) - 60_000.1).abs() < 1e-9);
    }

    #[test]
    fn contract_spec_round_trips_through_json() {
        let s = btc_spec();
        let back: ContractSpec = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        assert_eq!(Side::Long.opposite(), Side::Short);
        assert_eq!(DEFAULT_FEES.maker_pct, 0.02);
    }
}

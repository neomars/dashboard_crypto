//! Règles de risque du moteur : taille de position par cible de volatilité,
//! plafond de levier, rééquilibrage par seuil, stop suiveur en ATR et
//! coupe-circuits. Ces règles s'appliquent au backtest et s'appliqueront de
//! la même façon à l'exécution réelle.

/// Fenêtre de la volatilité réalisée, en jours.
pub const VOL_WINDOW_DAYS: usize = 30;
/// Période de l'ATR du stop suiveur, en bougies.
pub const ATR_LOOKBACK: usize = 20;
/// Coupe-circuit : perte maximale sur une journée UTC (3 %).
pub const DAILY_LOSS_LIMIT: f64 = 0.03;
/// Coupe-circuit : baisse maximale depuis le plus haut du capital (10 %).
pub const MAX_DRAWDOWN_LIMIT: f64 = 0.10;
/// Marge de maintenance du perpétuel (fraction de la position) : en dessous, liquidation.
pub const MAINTENANCE_MARGIN: f64 = 0.005;
/// Levier au-delà duquel le moteur avertit (plafond du document de recherche).
pub const RECOMMENDED_MAX_LEVERAGE: f64 = 2.0;
/// Levier maximal accepté par le moteur, même en backtest.
pub const HARD_MAX_LEVERAGE: f64 = 5.0;

/// Exposition cible, en fraction signée du capital (1,0 = 100 % du capital en
/// position acheteuse) : score × volatilité cible / volatilité réalisée,
/// plafonnée par le levier maximal, et jamais vendeuse si `allow_short` est faux.
pub fn target_exposure(
    score: f64,
    realized_vol: f64,
    target_vol: f64,
    max_leverage: f64,
    allow_short: bool,
) -> f64 {
    if !score.is_finite() || score == 0.0 || !realized_vol.is_finite() || realized_vol <= 0.0 {
        return 0.0;
    }
    let floor = if allow_short { -max_leverage } else { 0.0 };
    (score * target_vol / realized_vol).clamp(floor, max_leverage)
}

/// Rééquilibrer seulement si la position s'ouvre, se ferme, change de sens,
/// dépasse le levier maximal (les cours l'ont fait grossir), ou si l'exposition
/// varie de plus de `threshold` (fraction relative) : on évite de payer des
/// frais pour de petits ajustements.
pub fn needs_rebalance(current: f64, target: f64, threshold: f64, max_leverage: f64) -> bool {
    if current == target {
        return false;
    }
    if current == 0.0 || target == 0.0 || current.signum() != target.signum() {
        return true;
    }
    if current.abs() > max_leverage + 1e-9 {
        return true;
    }
    (target - current).abs() > threshold * current.abs().max(target.abs())
}

/// Niveau du stop suiveur : `mult` ATR sous le meilleur cours depuis l'entrée
/// (position acheteuse, `side` > 0) ou au-dessus (position vendeuse).
pub fn trailing_stop(side: f64, best: f64, atr: f64, mult: f64) -> f64 {
    if side > 0.0 {
        best - mult * atr
    } else {
        best + mult * atr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_scales_with_volatility_and_is_capped() {
        // Volatilité réalisée 50 %, cible 25 % : demi-position.
        assert_eq!(target_exposure(1.0, 0.5, 0.25, 1.0, false), 0.5);
        // Volatilité très basse : plafonnée au levier maximal.
        assert_eq!(target_exposure(1.0, 0.05, 0.25, 2.0, true), 2.0);
        // Score partiel.
        assert_eq!(target_exposure(0.5, 0.5, 0.25, 1.0, false), 0.25);
    }

    #[test]
    fn spot_never_goes_short() {
        assert_eq!(target_exposure(-1.0, 0.5, 0.25, 1.0, false), 0.0);
        assert_eq!(target_exposure(-1.0, 0.5, 0.25, 2.0, true), -0.5);
    }

    #[test]
    fn missing_volatility_means_no_position() {
        assert_eq!(target_exposure(1.0, 0.0, 0.25, 1.0, false), 0.0);
        assert_eq!(target_exposure(1.0, f64::NAN, 0.25, 1.0, false), 0.0);
        assert_eq!(target_exposure(f64::NAN, 0.5, 0.25, 1.0, false), 0.0);
    }

    #[test]
    fn rebalance_threshold() {
        assert!(needs_rebalance(0.0, 0.3, 0.2, 1.0), "ouverture");
        assert!(needs_rebalance(0.5, 0.0, 0.2, 1.0), "fermeture");
        assert!(needs_rebalance(0.5, -0.5, 0.2, 1.0), "changement de sens");
        assert!(
            !needs_rebalance(0.5, 0.55, 0.2, 1.0),
            "écart de 9 % : on garde"
        );
        assert!(
            needs_rebalance(0.5, 0.7, 0.2, 1.0),
            "écart de 29 % : on ajuste"
        );
        assert!(!needs_rebalance(0.0, 0.0, 0.2, 1.0));
    }

    #[test]
    fn leverage_cap_overrides_the_threshold() {
        // Une baisse a porté une position de 2x à 2,2x : sous le seuil de 20 %,
        // mais au-dessus du plafond, donc réduite.
        assert!(needs_rebalance(2.2, 2.0, 0.2, 2.0));
        assert!(!needs_rebalance(1.9, 2.0, 0.2, 2.0));
    }

    #[test]
    fn trailing_stop_sits_on_the_losing_side() {
        assert_eq!(trailing_stop(1.0, 100.0, 2.0, 3.0), 94.0);
        assert_eq!(trailing_stop(-1.0, 100.0, 2.0, 3.0), 106.0);
    }
}

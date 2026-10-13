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
/// Tolérance du plafond de levier : on ne rééquilibre que si l'exposition
/// dépasse le plafond de plus de 10 % en relatif (à 2x, au-delà de 2,2x). Sans
/// elle, chaque petite baisse des cours, qui fait grossir la position, la ferait
/// revendre dans le creux.
pub const LEVERAGE_TOLERANCE: f64 = 0.10;
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
/// dépasse le levier maximal de plus de [`LEVERAGE_TOLERANCE`] (les cours l'ont
/// fait grossir), ou si l'exposition varie de plus de `threshold` (fraction relative) : on évite de payer des
/// frais pour de petits ajustements.
pub fn needs_rebalance(current: f64, target: f64, threshold: f64, max_leverage: f64) -> bool {
    if current == target {
        return false;
    }
    if current == 0.0 || target == 0.0 || current.signum() != target.signum() {
        return true;
    }
    if current.abs() > max_leverage * (1.0 + LEVERAGE_TOLERANCE) {
        return true;
    }
    (target - current).abs() > threshold * current.abs().max(target.abs())
}

/// Coupe-circuit « baisse depuis le plus haut du capital » : il se déclenche une
/// fois par épisode et ne se réarme qu'après un nouveau plus haut du capital.
/// Sans cela, il se redéclenchait chaque fois que la baisse repassait sous le
/// seuil après un rebond partiel.
#[derive(Debug, Clone)]
pub struct DrawdownGuard {
    peak: f64,
    tripped: bool,
}

impl DrawdownGuard {
    pub fn new(initial: f64) -> Self {
        Self {
            peak: initial,
            tripped: false,
        }
    }

    /// Plus haut du capital.
    pub fn peak(&self) -> f64 {
        self.peak
    }

    /// Intègre le capital `equity` d'une clôture. Renvoie la baisse depuis le
    /// plus haut (fraction, négative ou nulle) et `true` si cette clôture ouvre
    /// un nouvel épisode au-delà de [`MAX_DRAWDOWN_LIMIT`].
    pub fn update(&mut self, equity: f64) -> (f64, bool) {
        if equity > self.peak {
            self.peak = equity;
            self.tripped = false;
        }
        let drawdown = if self.peak > 0.0 {
            equity / self.peak - 1.0
        } else {
            0.0
        };
        let fired = !self.tripped && drawdown <= -MAX_DRAWDOWN_LIMIT;
        if fired {
            self.tripped = true;
        }
        (drawdown, fired)
    }
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
        // Une baisse a porté une position de 2x à 2,3x : sous le seuil de 20 %,
        // mais au-delà du plafond et de sa tolérance de 10 % (2,2x), donc réduite.
        assert!(needs_rebalance(2.3, 2.0, 0.2, 2.0));
        assert!(!needs_rebalance(1.9, 2.0, 0.2, 2.0));
    }

    #[test]
    fn a_small_drop_at_the_leverage_cap_does_not_sell() {
        // Non-régression : à 2x, une baisse de 5 % des cours porte l'exposition
        // à 2,1x. Elle déclenchait une vente à chaque creux (seuil : plafond
        // + 1e-9) ; elle est maintenant tolérée jusqu'à 2,2x.
        assert!(!needs_rebalance(2.1, 2.0, 0.2, 2.0));
        assert!(!needs_rebalance(2.2, 2.0, 0.2, 2.0), "limite incluse");
        assert!(needs_rebalance(2.25, 2.0, 0.2, 2.0));
        // Même chose en vente à découvert.
        assert!(!needs_rebalance(-2.1, -2.0, 0.2, 2.0));
        assert!(needs_rebalance(-2.25, -2.0, 0.2, 2.0));
    }

    #[test]
    fn drawdown_guard_fires_once_per_episode() {
        let mut g = DrawdownGuard::new(100.0);
        let (dd, fired) = g.update(95.0);
        assert!((dd + 0.05).abs() < 1e-12 && !fired);
        assert!(g.update(89.0).1, "−11 % : premier déclenchement");
        // Rebond partiel puis nouvelle baisse sous −10 % : même épisode.
        assert!(!g.update(95.0).1);
        assert!(!g.update(85.0).1);
        assert!(!g.update(99.9).1, "pas encore de nouveau plus haut");
        assert!(!g.update(88.0).1, "toujours le même épisode");
        // Égalité avec le plus haut : pas un nouveau plus haut.
        assert!(!g.update(100.0).1);
        assert!(!g.update(89.5).1);
        // Nouveau plus haut : réarmé.
        assert_eq!(g.update(110.0), (0.0, false));
        assert_eq!(g.peak(), 110.0);
        assert!(g.update(98.0).1, "nouvel épisode");
    }

    #[test]
    fn trailing_stop_sits_on_the_losing_side() {
        assert_eq!(trailing_stop(1.0, 100.0, 2.0, 3.0), 94.0);
        assert_eq!(trailing_stop(-1.0, 100.0, 2.0, 3.0), 106.0);
    }
}

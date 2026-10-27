//! Signal d'achat ou de vente tiré du cours et du ratio long/short des top traders.
//!
//! À chaque relevé, avec le seul passé (aucune anticipation) :
//! - **positionnement** : le ratio s'écarte-t-il de sa moyenne des 7 derniers jours de plus de
//!   [`THRESHOLD`] écarts-types (et d'au moins [`MIN_DEVIATION`]) ? Les top traders sont alors
//!   nettement plus long (ou plus short) que d'habitude ;
//! - **tendance** : le cours est-il au-dessus ou au-dessous de sa moyenne des 2 derniers jours ?
//!
//! Un positionnement extrême qui va contre le cours (très long alors que le cours est sous sa
//! moyenne, ou très short alors qu'il est au-dessus) donne un signal. Cette situation se lit de
//! deux façons opposées, au choix ([`Reading`]) :
//! - **contrarienne** : la foule est piégée, ses positions sont à risque (liquidations ou squeeze) ;
//! - **smart money** : les top traders ont raison, ils achètent la baisse ou vendent la hausse.
//!
//! Aucune des deux lectures n'est validée : Binance ne publie que 30 jours de ratio, trop peu
//! pour mesurer la fiabilité d'une règle.

use crate::series::{rolling_mean, rolling_std};

/// Écart (en écarts-types) à partir duquel le ratio est « extrême ».
pub const THRESHOLD: f64 = 1.5;
/// Écart minimal à la moyenne, en unités de ratio, pour parler d'extrême : sans lui, le
/// moindre bruit d'un ratio presque constant (contrat peu actif) passerait pour un extrême.
pub const MIN_DEVIATION: f64 = 0.05;
/// Fenêtre de référence du ratio (positionnement habituel).
pub const RATIO_DAYS: i64 = 7;
/// Fenêtre de la moyenne du cours (tendance).
pub const TREND_DAYS: i64 = 2;
const DAY_MS: i64 = 86_400_000;
/// En dessous, l'écart-type est considéré nul (ratio constant sur la fenêtre).
const MIN_STD: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Contrarian,
    SmartMoney,
}

impl Reading {
    /// Valeur du paramètre de l'interface (`contrarian` ou `smart_money`).
    pub fn from_param(param: &str) -> Option<Self> {
        match param {
            "contrarian" => Some(Self::Contrarian),
            "smart_money" => Some(Self::SmartMoney),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Contrarian => "contrarienne",
            Self::SmartMoney => "smart money",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Buy,
    Sell,
}

impl Action {
    fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Buy => "ACHAT",
            Self::Sell => "VENTE",
        }
    }
}

/// Positionnement des top traders par rapport à leur habitude (ratio vs sa moyenne).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crowd {
    Long,
    Short,
    Normal,
}

/// Position du cours par rapport à sa moyenne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
}

/// Fenêtres de calcul, en nombre de relevés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    /// Moyenne et écart-type du ratio.
    pub ratio: usize,
    /// Moyenne du cours ; sert aussi de délai de carence entre deux signaux de même sens.
    pub trend: usize,
}

impl Windows {
    /// Fenêtres de [`RATIO_DAYS`] et [`TREND_DAYS`] jours pour une période de `period_ms`
    /// (au moins 5 et 2 relevés).
    pub fn for_period(period_ms: i64) -> Self {
        let points = |days: i64, min: usize| ((days * DAY_MS / period_ms) as usize).max(min);
        Self {
            ratio: points(RATIO_DAYS, 5),
            trend: points(TREND_DAYS, 2),
        }
    }
}

/// Résultat de l'analyse, un élément par relevé. `None` : historique insuffisant
/// (les fenêtres ne sont pas encore pleines).
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Moyenne mobile du ratio.
    pub mean: Vec<Option<f64>>,
    /// Demi-largeur des bandes d'extrême : [`THRESHOLD`] écarts-types, au moins [`MIN_DEVIATION`].
    pub band: Vec<Option<f64>>,
    /// Écart du ratio à sa moyenne, en écarts-types (pour l'affichage : le positionnement se
    /// décide d'après `band`).
    pub z: Vec<Option<f64>>,
    /// Moyenne mobile du cours.
    pub price_ma: Vec<Option<f64>>,
    pub crowd: Vec<Option<Crowd>>,
    pub trend: Vec<Option<Trend>>,
    /// Signal en vigueur à chaque relevé (`None` : neutre ou historique insuffisant).
    pub state: Vec<Option<Action>>,
    /// Relevés où un signal apparaît : premier relevé de chaque série de même sens, hors délai de carence.
    pub events: Vec<(usize, Action)>,
}

impl Analysis {
    pub fn last_event(&self) -> Option<(usize, Action)> {
        self.events.last().copied()
    }
}

/// Signal d'une situation : positionnement extrême qui va contre le cours.
pub fn signal_for(reading: Reading, crowd: Crowd, trend: Trend) -> Option<Action> {
    let contrarian = match (crowd, trend) {
        // Foule long alors que le cours décroche : les longs sont à risque.
        (Crowd::Long, Trend::Down) => Some(Action::Sell),
        // Foule short alors que le cours remonte : les shorts sont à risque.
        (Crowd::Short, Trend::Up) => Some(Action::Buy),
        _ => None,
    };
    match reading {
        Reading::Contrarian => contrarian,
        Reading::SmartMoney => contrarian.map(Action::opposite),
    }
}

/// Relevés où un signal apparaît : le premier de chaque série de même sens, sauf si un signal
/// de même sens a déjà eu lieu moins de `cooldown` relevés plus tôt.
fn events_from(state: &[Option<Action>], cooldown: usize) -> Vec<(usize, Action)> {
    let mut events: Vec<(usize, Action)> = Vec::new();
    for (t, current) in state.iter().enumerate() {
        let Some(action) = *current else { continue };
        let starts = t == 0 || state[t - 1] != Some(action);
        let cooling = events
            .iter()
            .rev()
            .find(|(_, a)| *a == action)
            .is_some_and(|(i, _)| t - i < cooldown);
        if starts && !cooling {
            events.push((t, action));
        }
    }
    events
}

/// Analyse le ratio et le cours (un cours connu à chaque relevé, de même longueur).
/// Chaque élément ne dépend que du relevé correspondant et de ceux qui le précèdent.
pub fn analyze(ratios: &[f64], prices: &[f64], reading: Reading, windows: Windows) -> Analysis {
    debug_assert_eq!(ratios.len(), prices.len());
    let n = ratios.len().min(prices.len());
    let (ratios, prices) = (&ratios[..n], &prices[..n]);

    let mean = rolling_mean(ratios, windows.ratio);
    let std = rolling_std(ratios, windows.ratio);
    let price_ma = rolling_mean(prices, windows.trend);

    let z: Vec<Option<f64>> = (0..n)
        .map(|t| match (mean[t], std[t]) {
            (Some(m), Some(s)) if s > MIN_STD => Some((ratios[t] - m) / s),
            (Some(_), Some(_)) => Some(0.0),
            _ => None,
        })
        .collect();
    let band: Vec<Option<f64>> = std
        .iter()
        .map(|s| s.map(|s| (THRESHOLD * s).max(MIN_DEVIATION)))
        .collect();
    let crowd: Vec<Option<Crowd>> = (0..n)
        .map(|t| {
            let (m, b) = (mean[t]?, band[t]?);
            let deviation = ratios[t] - m;
            Some(if deviation >= b {
                Crowd::Long
            } else if deviation <= -b {
                Crowd::Short
            } else {
                Crowd::Normal
            })
        })
        .collect();
    let trend: Vec<Option<Trend>> = (0..n)
        .map(|t| {
            price_ma[t].map(|ma| {
                if prices[t] > ma {
                    Trend::Up
                } else {
                    Trend::Down
                }
            })
        })
        .collect();
    let state: Vec<Option<Action>> = crowd
        .iter()
        .zip(&trend)
        .map(|(c, t)| match (c, t) {
            (Some(c), Some(t)) => signal_for(reading, *c, *t),
            _ => None,
        })
        .collect();

    Analysis {
        events: events_from(&state, windows.trend),
        band,
        mean,
        z,
        price_ma,
        crowd,
        trend,
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Windows = Windows {
        ratio: 10,
        trend: 3,
    };

    /// Ratio qui oscille autour de 1,5 pendant 30 relevés, puis `last` au 31e.
    fn ratios_ending_with(last: f64) -> Vec<f64> {
        let mut r: Vec<f64> = (0..30)
            .map(|i| if i % 2 == 0 { 1.48 } else { 1.52 })
            .collect();
        r.push(last);
        r
    }

    /// Cours qui monte pendant 30 relevés (de 100 à 129), puis `last` au 31e.
    fn prices_ending_with(last: f64) -> Vec<f64> {
        let mut p: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        p.push(last);
        p
    }

    #[test]
    fn contrarian_sells_a_long_crowd_while_the_price_drops() {
        let a = analyze(
            &ratios_ending_with(2.4),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        assert_eq!(a.crowd[30], Some(Crowd::Long));
        assert_eq!(a.trend[30], Some(Trend::Down));
        assert_eq!(a.state[30], Some(Action::Sell));
        assert_eq!(a.state[29], None, "la veille, rien");
        assert_eq!(a.events, [(30, Action::Sell)]);
        assert_eq!(a.last_event(), Some((30, Action::Sell)));
        assert!(a.z[30].unwrap() > THRESHOLD);
    }

    #[test]
    fn contrarian_buys_a_short_crowd_while_the_price_rises() {
        let a = analyze(
            &ratios_ending_with(0.8),
            &prices_ending_with(140.0),
            Reading::Contrarian,
            W,
        );
        assert_eq!(a.crowd[30], Some(Crowd::Short));
        assert_eq!(a.trend[30], Some(Trend::Up));
        assert_eq!(a.events, [(30, Action::Buy)]);
    }

    #[test]
    fn no_signal_when_the_price_confirms_the_crowd() {
        // Foule long ET cours au-dessus de sa moyenne : rien, quelle que soit la lecture.
        for reading in [Reading::Contrarian, Reading::SmartMoney] {
            let a = analyze(
                &ratios_ending_with(2.4),
                &prices_ending_with(140.0),
                reading,
                W,
            );
            assert_eq!(a.crowd[30], Some(Crowd::Long));
            assert_eq!(a.trend[30], Some(Trend::Up));
            assert!(a.state.iter().all(Option::is_none) && a.events.is_empty());
        }
        // Foule short ET cours sous sa moyenne : rien non plus.
        let a = analyze(
            &ratios_ending_with(0.8),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        assert_eq!(a.crowd[30], Some(Crowd::Short));
        assert!(a.events.is_empty());
    }

    #[test]
    fn smart_money_reads_the_same_situations_the_other_way() {
        let down = analyze(
            &ratios_ending_with(2.4),
            &prices_ending_with(100.0),
            Reading::SmartMoney,
            W,
        );
        assert_eq!(down.events, [(30, Action::Buy)], "ils achètent la baisse");
        let up = analyze(
            &ratios_ending_with(0.8),
            &prices_ending_with(140.0),
            Reading::SmartMoney,
            W,
        );
        assert_eq!(up.events, [(30, Action::Sell)], "ils vendent la hausse");
    }

    #[test]
    fn a_normal_ratio_gives_no_signal_whatever_the_price() {
        // 1,51 reste dans le bruit habituel (entre 1,48 et 1,52).
        let a = analyze(
            &ratios_ending_with(1.51),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        assert_eq!(a.crowd[30], Some(Crowd::Normal));
        assert!(a.events.is_empty());
    }

    #[test]
    fn the_bands_are_at_least_the_minimum_deviation_wide() {
        let a = analyze(
            &ratios_ending_with(1.51),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        // Bruit de ±0,02 : 1,5 écart-type vaut ~0,03, moins que le plancher.
        assert_eq!(a.band[30], Some(MIN_DEVIATION));
        // Un vrai mouvement élargit les bandes au-delà du plancher.
        let wide = analyze(
            &ratios_ending_with(2.4),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        assert!(wide.band[30].unwrap() > MIN_DEVIATION);
    }

    #[test]
    fn noise_on_an_almost_constant_ratio_is_not_an_extreme() {
        // Ratio à 1,000 pendant des jours, puis un bruit de 0,02 : énorme en écarts-types
        // (l'écart-type vaut presque zéro), négligeable en réalité.
        let mut ratios = vec![1.0; 30];
        ratios.push(1.02);
        let prices: Vec<f64> = (0..31).map(|i| 130.0 - i as f64).collect(); // le cours baisse
        let a = analyze(&ratios, &prices, Reading::Contrarian, W);
        assert!(
            a.z[30].unwrap() > THRESHOLD,
            "le z-score seul y verrait un extrême"
        );
        assert_eq!(a.crowd[30], Some(Crowd::Normal));
        assert!(a.events.is_empty());
        // Un écart qui compte (0,10) en est bien un.
        ratios[30] = 1.10;
        let a = analyze(&ratios, &prices, Reading::Contrarian, W);
        assert_eq!(a.crowd[30], Some(Crowd::Long));
        assert_eq!(a.events, [(30, Action::Sell)]);
    }

    #[test]
    fn a_constant_ratio_is_never_extreme() {
        let ratios = vec![1.5; 40];
        let prices: Vec<f64> = (0..40).map(|i| 100.0 - i as f64).collect();
        let a = analyze(&ratios, &prices, Reading::Contrarian, W);
        assert_eq!(a.z[39], Some(0.0));
        assert_eq!(a.crowd[39], Some(Crowd::Normal));
        assert!(a.events.is_empty());
    }

    #[test]
    fn nothing_is_decided_before_the_windows_are_full() {
        let a = analyze(
            &ratios_ending_with(2.4),
            &prices_ending_with(100.0),
            Reading::Contrarian,
            W,
        );
        // Ratio : 10 relevés pour la moyenne et l'écart-type ; cours : 3 relevés.
        assert!(a.z[..9].iter().all(Option::is_none));
        assert!(a.z[9].is_some());
        assert!(a.crowd[..9].iter().all(Option::is_none));
        assert!(a.trend[..2].iter().all(Option::is_none));
        assert!(a.trend[2].is_some());
        assert!(a.state[..9].iter().all(Option::is_none));
    }

    #[test]
    fn a_run_of_signals_is_one_event_and_the_cooldown_spaces_the_next() {
        use Action::{Buy, Sell};
        let state = [
            None,
            Some(Sell), // 1 : début de série -> événement
            Some(Sell),
            Some(Sell),
            None,
            Some(Sell), // 5 : nouvelle série, 4 relevés après -> permis (carence de 3)
            None,
            Some(Sell), // 7 : nouvelle série, 2 relevés après -> bloquée
            None,
            Some(Buy),  // 9 : l'autre sens n'est pas bloqué par la carence des ventes
            Some(Sell), // 10 : changement direct de sens, 5 relevés après la dernière vente -> permis
        ];
        assert_eq!(
            events_from(&state, 3),
            [(1, Sell), (5, Sell), (9, Buy), (10, Sell)]
        );
        // Sans délai de carence, chaque nouvelle série compte (1, 5, 7, 9 et 10).
        assert_eq!(events_from(&state, 0).len(), 5);
    }

    #[test]
    fn windows_follow_the_period() {
        let h = 3_600_000;
        assert_eq!(
            Windows::for_period(4 * h),
            Windows {
                ratio: 42,
                trend: 12
            }
        );
        assert_eq!(Windows::for_period(24 * h), Windows { ratio: 7, trend: 2 });
        assert_eq!(
            Windows::for_period(12 * h),
            Windows {
                ratio: 14,
                trend: 4
            }
        );
        assert_eq!(
            Windows::for_period(5 * 60_000),
            Windows {
                ratio: 2016,
                trend: 576
            }
        );
    }

    #[test]
    fn readings_come_from_the_interface_parameter() {
        assert_eq!(Reading::from_param("contrarian"), Some(Reading::Contrarian));
        assert_eq!(
            Reading::from_param("smart_money"),
            Some(Reading::SmartMoney)
        );
        assert_eq!(Reading::from_param("autre"), None);
        assert_eq!(Action::Buy.label(), "ACHAT");
        assert_eq!(Action::Sell.label(), "VENTE");
    }

    /// Le signal d'un relevé ne dépend que du passé : analyser un historique tronqué donne
    /// exactement le même début que l'analyser en entier (aucune anticipation).
    #[test]
    fn the_signal_never_looks_ahead() {
        let n = 240;
        let ratios: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64;
                1.6 + 0.5 * (t / 11.0).sin() + 0.3 * (t / 3.7).sin() + 0.002 * t
            })
            .collect();
        let prices: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64;
                100.0 + 8.0 * (t / 13.0).sin() + 3.0 * (t / 2.9).cos()
            })
            .collect();
        let windows = Windows {
            ratio: 20,
            trend: 5,
        };
        for reading in [Reading::Contrarian, Reading::SmartMoney] {
            let full = analyze(&ratios, &prices, reading, windows);
            assert!(
                !full.events.is_empty(),
                "le jeu d'essai doit produire des signaux"
            );
            for k in [25, 40, 77, 120, 199, 239] {
                let cut = analyze(&ratios[..k], &prices[..k], reading, windows);
                assert_eq!(cut.mean, full.mean[..k], "moyenne, k={k}");
                assert_eq!(cut.band, full.band[..k], "bandes, k={k}");
                assert_eq!(cut.z, full.z[..k], "z, k={k}");
                assert_eq!(cut.price_ma, full.price_ma[..k], "moyenne du cours, k={k}");
                assert_eq!(cut.crowd, full.crowd[..k], "positionnement, k={k}");
                assert_eq!(cut.trend, full.trend[..k], "tendance, k={k}");
                assert_eq!(cut.state, full.state[..k], "signal, k={k}");
                let early: Vec<_> = full
                    .events
                    .iter()
                    .copied()
                    .filter(|(i, _)| *i < k)
                    .collect();
                assert_eq!(cut.events, early, "événements, k={k}");
            }
        }
    }
}

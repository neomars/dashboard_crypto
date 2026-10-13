//! Module tendance (module A du document de recherche) : un score de −1 à +1
//! calculé à la clôture de chaque bougie, à partir de cette clôture et des
//! bougies précédentes uniquement.
//!
//! - Variante « Donchian » : moyenne de quatre signaux de cassure (20, 55, 100
//!   et 200 bougies). Un signal passe à +1 quand la clôture dépasse le plus
//!   haut des L bougies précédentes et le reste tant que la clôture tient
//!   au-dessus du plus bas des L/2 bougies précédentes (symétrique à −1 en
//!   vente à découvert, sur le perpétuel seulement).
//! - Variante « EMA » : +1 si EMA 20 > EMA 100 et clôture > SMA 200 ; −1 dans
//!   le cas inverse (perpétuel seulement) ; 0 sinon.
//! - Variante « Cassure » : acheteuse uniquement, même sur le perpétuel. Entrée
//!   quand la clôture égale ou dépasse la plus haute clôture des 55 bougies
//!   précédentes et dépasse la SMA 200 ; sortie dès que la clôture passe sous la
//!   SMA 200. Score 1 ou 0.

use crate::market::Bar;
use crate::ta::{ema, highest, lowest, previous, sma};
use serde::{Deserialize, Serialize};

/// Périodes des quatre signaux de cassure.
pub const DONCHIAN_LOOKBACKS: [usize; 4] = [20, 55, 100, 200];
pub const EMA_FAST: usize = 20;
pub const EMA_SLOW: usize = 100;
pub const SMA_FILTER: usize = 200;
/// Cassure : nombre de clôtures précédentes dont on dépasse la plus haute.
pub const BREAKOUT_LOOKBACK: usize = 55;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrendVariant {
    Donchian,
    Ema,
    /// Cassure 55 bougies, sortie sous la moyenne 200 (acheteuse seulement).
    Breakout,
}

impl TrendVariant {
    pub fn label(self) -> &'static str {
        match self {
            TrendVariant::Donchian => "Ensemble Donchian (20, 55, 100, 200)",
            TrendVariant::Ema => "Croisement EMA 20/100 + filtre SMA 200",
            TrendVariant::Breakout => "Cassure 55 j, sortie sous moyenne 200 j",
        }
    }

    /// Bougies nécessaires avant le premier score.
    pub fn warmup_bars(self) -> usize {
        match self {
            TrendVariant::Donchian => DONCHIAN_LOOKBACKS[3] + 1,
            TrendVariant::Ema => SMA_FILTER,
            TrendVariant::Breakout => SMA_FILTER + 1,
        }
    }
}

/// Score de tendance et détail de ses composantes.
#[derive(Debug, Clone)]
pub struct TrendSignal {
    /// Score à la clôture de chaque bougie, de −1 (vendeur) à +1 (acheteur).
    pub score: Vec<Option<f64>>,
    /// État (−1, 0, +1) de chaque signal de cassure, par période (vide pour la variante EMA).
    pub components: Vec<(usize, Vec<Option<i8>>)>,
}

/// État d'un signal de cassure de Donchian de période `lookback`.
pub fn donchian_states(bars: &[Bar], lookback: usize, allow_short: bool) -> Vec<Option<i8>> {
    let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let exit_len = (lookback / 2).max(1);
    // Canaux des bougies précédentes : la clôture courante n'y entre pas.
    let entry_high = previous(&highest(&highs, lookback));
    let entry_low = previous(&lowest(&lows, lookback));
    let exit_low = previous(&lowest(&lows, exit_len));
    let exit_high = previous(&highest(&highs, exit_len));

    let mut state: i8 = 0;
    bars.iter()
        .enumerate()
        .map(|(i, bar)| {
            let (Some(up), Some(down), Some(stop_long), Some(stop_short)) =
                (entry_high[i], entry_low[i], exit_low[i], exit_high[i])
            else {
                return None;
            };
            let close = bar.close;
            if (state == 1 && close < stop_long) || (state == -1 && close > stop_short) {
                state = 0;
            }
            if state == 0 {
                if close > up {
                    state = 1;
                } else if allow_short && close < down {
                    state = -1;
                }
            }
            Some(state)
        })
        .collect()
}

/// Score 1 ou 0 de la variante « cassure » : la position est ouverte tant que la
/// clôture reste au moins à la SMA 200. Pas de score avant `SMA_FILTER + 1`
/// bougies.
pub fn breakout_states(bars: &[Bar]) -> Vec<Option<f64>> {
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    // Plus haute clôture des bougies précédentes : la clôture courante n'y entre pas.
    let high = previous(&highest(&closes, BREAKOUT_LOOKBACK));
    let filter = sma(&closes, SMA_FILTER);
    let mut long = false;
    (0..closes.len())
        .map(|i| {
            let (Some(h), Some(m)) = (high[i], filter[i]) else {
                return None;
            };
            if i < SMA_FILTER {
                return None;
            }
            let close = closes[i];
            if long {
                long = close >= m;
            } else {
                long = close >= h && close > m;
            }
            Some(if long { 1.0 } else { 0.0 })
        })
        .collect()
}

/// Score de tendance de la variante choisie.
pub fn trend_signal(bars: &[Bar], variant: TrendVariant, allow_short: bool) -> TrendSignal {
    match variant {
        TrendVariant::Donchian => {
            let components: Vec<(usize, Vec<Option<i8>>)> = DONCHIAN_LOOKBACKS
                .iter()
                .map(|&l| (l, donchian_states(bars, l, allow_short)))
                .collect();
            let score = (0..bars.len())
                .map(|i| {
                    let states: Option<Vec<i8>> = components.iter().map(|(_, s)| s[i]).collect();
                    states.map(|s| s.iter().map(|&x| f64::from(x)).sum::<f64>() / s.len() as f64)
                })
                .collect();
            TrendSignal { score, components }
        }
        TrendVariant::Breakout => TrendSignal {
            score: breakout_states(bars),
            components: Vec::new(),
        },
        TrendVariant::Ema => {
            let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
            let (fast, slow, filter) = (
                ema(&closes, EMA_FAST),
                ema(&closes, EMA_SLOW),
                sma(&closes, SMA_FILTER),
            );
            let score = (0..bars.len())
                .map(|i| match (fast[i], slow[i], filter[i]) {
                    (Some(f), Some(s), Some(m)) => Some(if f > s && closes[i] > m {
                        1.0
                    } else if allow_short && f < s && closes[i] < m {
                        -1.0
                    } else {
                        0.0
                    }),
                    _ => None,
                })
                .collect();
            TrendSignal {
                score,
                components: Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bougies sans mèche autour des clôtures données.
    fn bars(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: i as i64,
                open: c,
                high: c,
                low: c,
                close: c,
                volume: 0.0,
            })
            .collect()
    }

    #[test]
    fn breakout_enters_and_exit_channel_closes() {
        // Période 4 : sortie sous le plus bas des 2 bougies précédentes.
        let b = bars(&[10.0, 10.0, 10.0, 10.0, 11.0, 12.0, 11.5, 11.8, 11.0]);
        let s = donchian_states(&b, 4, false);
        assert_eq!(s[..4], [None, None, None, None]);
        assert_eq!(
            s[4],
            Some(1),
            "11 > plus haut des 4 bougies précédentes (10)"
        );
        assert_eq!(s[6], Some(1), "11,5 tient au-dessus de min(11, 12)");
        assert_eq!(s[7], Some(1));
        assert_eq!(s[8], Some(0), "11 < min(11,5 ; 11,8)");
    }

    #[test]
    fn shorts_only_when_allowed() {
        let b = bars(&[10.0, 10.0, 10.0, 10.0, 9.0, 8.5]);
        assert_eq!(donchian_states(&b, 4, false)[5], Some(0));
        let s = donchian_states(&b, 4, true);
        assert_eq!(s[4], Some(-1));
        assert_eq!(s[5], Some(-1));
    }

    #[test]
    fn a_long_exit_can_flip_short_on_the_same_close() {
        let b = bars(&[10.0, 10.0, 10.0, 10.0, 11.0, 7.0]);
        let s = donchian_states(&b, 4, true);
        assert_eq!(s[4], Some(1));
        assert_eq!(
            s[5],
            Some(-1),
            "7 casse la sortie et le plus bas des 4 bougies"
        );
    }

    #[test]
    fn ensemble_needs_the_longest_lookback() {
        let closes: Vec<f64> = (0..260).map(|i| 100.0 + i as f64).collect();
        let sig = trend_signal(&bars(&closes), TrendVariant::Donchian, false);
        assert!(sig.score[199].is_none());
        assert_eq!(
            sig.score[200],
            Some(1.0),
            "hausse régulière : les 4 signaux à +1"
        );
        assert_eq!(sig.components.len(), 4);
        assert_eq!(TrendVariant::Donchian.warmup_bars(), 201);
    }

    #[test]
    fn ensemble_averages_components() {
        // Hausse longue puis repli de 10 bougies : les signaux 20 et 55 sortent
        // (bougies 303 et 307), les signaux 100 et 200 tiennent encore.
        let mut closes: Vec<f64> = (0..300).map(|i| 100.0 + i as f64).collect();
        closes.extend((0..10).map(|i| 399.0 - 3.0 * i as f64));
        let sig = trend_signal(&bars(&closes), TrendVariant::Donchian, false);
        let last = |l: usize| sig.components.iter().find(|c| c.0 == l).unwrap().1[309];
        assert_eq!(
            (last(20), last(55), last(100), last(200)),
            (Some(0), Some(0), Some(1), Some(1))
        );
        assert_eq!(sig.score[309], Some(0.5));
    }

    #[test]
    fn ema_variant_follows_the_trend() {
        let up: Vec<f64> = (0..260).map(|i| 100.0 * 1.01f64.powi(i)).collect();
        let sig = trend_signal(&bars(&up), TrendVariant::Ema, true);
        assert!(sig.score[198].is_none());
        assert_eq!(*sig.score.last().unwrap(), Some(1.0));

        let down: Vec<f64> = (0..260).map(|i| 100.0 * 0.99f64.powi(i)).collect();
        let long_only = trend_signal(&bars(&down), TrendVariant::Ema, false);
        assert_eq!(*long_only.score.last().unwrap(), Some(0.0));
        let with_short = trend_signal(&bars(&down), TrendVariant::Ema, true);
        assert_eq!(*with_short.score.last().unwrap(), Some(-1.0));
    }

    /// 230 clôtures plates à 100, puis `tail`.
    fn flat_then(tail: &[f64]) -> Vec<Bar> {
        let mut closes = vec![100.0; 230];
        closes.extend_from_slice(tail);
        bars(&closes)
    }

    #[test]
    fn breakout_enters_holds_and_exits_below_the_200_average() {
        let sig = trend_signal(
            &flat_then(&[101.0, 103.0, 100.6, 99.0, 99.5, 103.0]),
            TrendVariant::Breakout,
            false,
        );
        let s = &sig.score;
        assert!(s[199].is_none(), "201 bougies de préchauffage");
        assert_eq!(s[200], Some(0.0));
        assert_eq!(
            s[229],
            Some(0.0),
            "clôture égale au plus haut précédent mais pas au-dessus de la moyenne 200"
        );
        assert_eq!(
            s[230],
            Some(1.0),
            "101 > plus haute clôture (100) et > SMA 200"
        );
        assert_eq!(s[231], Some(1.0));
        assert_eq!(
            s[232],
            Some(1.0),
            "100,6 est sous la cassure mais au-dessus de la moyenne : on garde"
        );
        assert_eq!(s[233], Some(0.0), "99 < moyenne 200 : sortie");
        assert_eq!(s[234], Some(0.0), "pas de cassure : reste à plat");
        assert_eq!(
            s[235],
            Some(1.0),
            "103 égale la plus haute clôture des 55 précédentes (≥) et dépasse la moyenne"
        );
    }

    #[test]
    fn breakout_does_not_enter_below_the_200_average() {
        // 200 clôtures à 200, effondrement à 100 pendant 60 bougies, puis un
        // sommet à 101 : plus haute clôture des 55 précédentes, mais très loin
        // sous la moyenne 200 (environ 170).
        let mut closes = vec![200.0; 200];
        closes.extend(vec![100.0; 60]);
        closes.push(101.0);
        let sig = trend_signal(&bars(&closes), TrendVariant::Breakout, false);
        assert_eq!(sig.score[260], Some(0.0));
        assert!(sig.score.iter().flatten().all(|&x| x == 0.0 || x == 1.0));
        assert_eq!(sig.score[200..].iter().flatten().sum::<f64>(), 0.0);
    }

    #[test]
    fn breakout_is_long_only_even_on_the_perpetual() {
        let down: Vec<f64> = (0..300).map(|i| 200.0 * 0.995f64.powi(i)).collect();
        let sig = trend_signal(&bars(&down), TrendVariant::Breakout, true);
        assert!(sig.score.iter().flatten().all(|&x| x == 0.0));
        assert!(sig.components.is_empty());
        assert_eq!(TrendVariant::Breakout.warmup_bars(), 201);
        assert_eq!(
            TrendVariant::Breakout.label(),
            "Cassure 55 j, sortie sous moyenne 200 j"
        );
        assert_eq!(
            serde_json::to_string(&TrendVariant::Breakout).unwrap(),
            "\"breakout\""
        );
    }
}

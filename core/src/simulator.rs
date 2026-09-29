//! Simulateur d'investissement : stratégie de levier dynamique (x1 → xL
//! lors d'une baisse de X % depuis le plus haut), puis sortie progressive
//! du levier avec règle « no-loss » (pas de vente sous le prix d'achat).

use crate::data::halving_dates;
use crate::figure::{dates, merge, two_rows, Figure};
use crate::series::{fmt_date, PricePoint};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitFrequency {
    #[serde(rename = "Journalière")]
    Daily,
    #[serde(rename = "Hebdomadaire")]
    Weekly,
    #[serde(rename = "Mensuelle")]
    Monthly,
}

impl ExitFrequency {
    fn days_per_step(self) -> i64 {
        match self {
            Self::Daily => 1,
            Self::Weekly => 7,
            Self::Monthly => 30,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Journalière",
            Self::Weekly => "Hebdomadaire",
            Self::Monthly => "Mensuelle",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimParams {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub initial_capital: f64,
    pub drop_pct: f64,
    pub target_leverage: f64,
    pub exit_frequency: ExitFrequency,
    pub exit_pct: f64,
    pub ticker: String,
}

impl SimParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.start >= self.end {
            return Err("La date de début doit être antérieure à la date de fin.".into());
        }
        if self.initial_capital.is_nan() || self.initial_capital <= 0.0 {
            return Err("L'investissement initial doit être positif.".into());
        }
        if !(1.0..).contains(&self.target_leverage) {
            return Err("L'effet de levier cible doit être au moins de 1.".into());
        }
        if self.drop_pct.is_nan() || self.drop_pct <= 0.0 || self.drop_pct >= 100.0 {
            return Err("La baisse déclencheur doit être comprise entre 0 et 100 %.".into());
        }
        if !(1.0..=100.0).contains(&self.exit_pct) {
            return Err("La sortie par étape doit être comprise entre 1 et 100 %.".into());
        }
        if self.ticker.trim().is_empty() {
            return Err("Le ticker Yahoo Finance est vide.".into());
        }
        Ok(())
    }

    /// Paramètres tels qu'affichés dans le rapport PDF.
    pub fn display_rows(&self) -> Vec<(String, String)> {
        vec![
            ("Ticker".into(), self.ticker.clone()),
            ("Date de début".into(), fmt_date(self.start)),
            ("Date de fin".into(), fmt_date(self.end)),
            (
                "Capital Initial".into(),
                format!("{}", self.initial_capital),
            ),
            ("Levier Cible".into(), format!("{}", self.target_leverage)),
            ("Baisse Déclencheur".into(), format!("{}%", self.drop_pct)),
            (
                "Fréquence Sortie".into(),
                self.exit_frequency.label().into(),
            ),
            ("Sortie par étape".into(), format!("{}%", self.exit_pct)),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Mode {
    X1,
    XL,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryRow {
    pub date: NaiveDate,
    pub btc_price: f64,
    pub portfolio_value: f64,
    pub mode: Mode,
    pub btc_units: f64,
    pub drawdown: f64,
    pub buy_hold: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub date: NaiveDate,
    pub action: String,
    pub price: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub final_equity: f64,
    pub performance_pct: f64,
    pub final_units: f64,
    pub initial_units: f64,
    pub buy_hold: f64,
    pub buy_hold_pct: f64,
    pub max_drawdown: f64,
    pub liquidated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Simulation {
    pub params: SimParams,
    pub history: Vec<HistoryRow>,
    pub trades: Vec<Trade>,
}

/// Montant au format `$1,234.56`.
pub fn money(v: f64) -> String {
    let neg = v < 0.0;
    let s = format!("{:.2}", v.abs());
    let (int, dec) = s.split_once('.').unwrap();
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{}${grouped}.{dec}", if neg { "-" } else { "" })
}

pub fn simulate(prices: &[PricePoint], params: &SimParams) -> Result<Simulation, String> {
    params.validate()?;
    let first = prices.first().ok_or("Pas de données disponibles.")?;
    let unit = params
        .ticker
        .split('-')
        .next()
        .unwrap_or(&params.ticker)
        .to_string();
    let drop_threshold = -params.drop_pct / 100.0;
    let lev = params.target_leverage;

    let start_price = first.close;
    let mut portfolio_value = params.initial_capital;
    let mut mode = Mode::X1;
    let mut ath = start_price;
    let mut waiting_for_recovery = false;
    let mut units = portfolio_value / start_price;
    let mut debt = 0.0;

    let mut trades = vec![Trade {
        date: first.date,
        action: "Achat Initial (Mode X1)".into(),
        price: money(start_price),
        details: format!(
            "Investissement de {} pour {units:.4} {unit}",
            money(portfolio_value)
        ),
    }];

    let mut closing_start = first.date;
    let mut units_to_close = 0.0;
    let mut purchase_price_xl = 0.0;
    let mut steps_passed: i64 = 0;
    let mut last_logged_skip: i64 = -1;
    let days_per_step = params.exit_frequency.days_per_step();
    let total_steps = (100.0 / params.exit_pct) as i64;

    let mut history = Vec::with_capacity(prices.len());
    let mut liquidated = false;
    let mut portfolio_ath = portfolio_value;

    for p in prices {
        let (date, price) = (p.date, p.close);
        match mode {
            Mode::X1 => {
                if price > ath {
                    ath = price;
                    waiting_for_recovery = false;
                }
                portfolio_value = units * price;
                if (price - ath) / ath <= drop_threshold && !waiting_for_recovery {
                    mode = Mode::XL;
                    closing_start = date;
                    steps_passed = 0;
                    let old_units = units;
                    let new_units = portfolio_value * lev / price;
                    debt = (new_units - units) * price;
                    units = new_units;
                    units_to_close = units;
                    purchase_price_xl = price;
                    trades.push(Trade {
                        date,
                        action: format!("Passage en Levier x{lev:.1}"),
                        price: money(price),
                        details: format!(
                            "Achat de {:.4} {unit} via dette ({})",
                            new_units - old_units,
                            money(debt)
                        ),
                    });
                }
            }
            Mode::XL => {
                portfolio_value = units * price - debt;
                let expected_steps = (date - closing_start).num_days() / days_per_step;
                if expected_steps > steps_passed {
                    if price >= purchase_price_xl {
                        // Rattrape toutes les étapes en attente, sans dépasser le total prévu.
                        let pending =
                            (expected_steps - steps_passed).min(total_steps - steps_passed);
                        let to_sell = (units_to_close * (params.exit_pct / 100.0) * pending as f64)
                            .min(units);
                        let mut proceeds = to_sell * price;
                        units -= to_sell;
                        let mut debt_paid = 0.0;
                        if debt > 0.0 {
                            debt_paid = proceeds.min(debt);
                            debt -= debt_paid;
                            proceeds -= debt_paid;
                        }
                        if proceeds > 0.0 {
                            units += proceeds / price;
                        }
                        trades.push(Trade {
                            date,
                            action: format!("Sortie progressive ({} {expected_steps}/{total_steps})", params.exit_frequency.label()),
                            price: money(price),
                            details: format!("Vente rattrapée ({pending} étape(s)) : {to_sell:.4} {unit}. Dette payée: {}.", money(debt_paid)),
                        });
                        steps_passed = expected_steps;
                    } else if expected_steps > last_logged_skip {
                        trades.push(Trade {
                            date,
                            action: "⚠️ Vente reportée".into(),
                            price: money(price),
                            details: format!(
                                "Prix actuel ({}) < Prix achat ({}). Report au mois suivant.",
                                money(price),
                                money(purchase_price_xl)
                            ),
                        });
                        last_logged_skip = expected_steps;
                    }

                    if steps_passed >= total_steps {
                        mode = Mode::X1;
                        portfolio_value = units * price - debt;
                        units = portfolio_value / price;
                        debt = 0.0;
                        trades.push(Trade {
                            date,
                            action: "Fin de phase Levier".into(),
                            price: money(price),
                            details: format!("Retour au mode X1 (100% investi en {unit})"),
                        });
                        // Encore sous le seuil : attendre de repasser au-dessus avant un nouveau déclenchement.
                        if (price - ath) / ath <= drop_threshold {
                            waiting_for_recovery = true;
                        }
                    }
                }
            }
        }

        if portfolio_value <= 0.0 && !liquidated {
            liquidated = true;
            trades.push(Trade {
                date,
                action: "💀 LIQUIDATION 💀".into(),
                price: money(price),
                details: "Le capital net est tombé à zéro.".into(),
            });
            portfolio_value = 0.0;
            units = 0.0;
            debt = 0.0;
        }

        portfolio_ath = portfolio_ath.max(portfolio_value);
        history.push(HistoryRow {
            date,
            btc_price: price,
            portfolio_value,
            mode,
            btc_units: units,
            drawdown: (portfolio_value - portfolio_ath) / portfolio_ath * 100.0,
            buy_hold: params.initial_capital * price / start_price,
        });
    }
    Ok(Simulation {
        params: params.clone(),
        history,
        trades,
    })
}

impl Simulation {
    pub fn summary(&self) -> Summary {
        let first = &self.history[0];
        let last = self.history.last().unwrap();
        let cap = self.params.initial_capital;
        Summary {
            final_equity: last.portfolio_value,
            performance_pct: (last.portfolio_value / cap - 1.0) * 100.0,
            final_units: last.btc_units,
            initial_units: cap / first.btc_price,
            buy_hold: last.buy_hold,
            buy_hold_pct: (last.buy_hold / cap - 1.0) * 100.0,
            max_drawdown: self.history.iter().map(|h| h.drawdown).fold(0.0, f64::min),
            liquidated: self.trades.iter().any(|t| t.action.contains("LIQUIDATION")),
        }
    }

    pub fn to_csv(&self) -> String {
        let mut out =
            String::from("Date,BTC_Price,Portfolio_Value,Mode,BTC_Units,Drawdown,Buy_Hold\n");
        for h in &self.history {
            let _ = writeln!(
                out,
                "{},{},{},{:?},{},{},{}",
                fmt_date(h.date),
                h.btc_price,
                h.portfolio_value,
                h.mode,
                h.btc_units,
                h.drawdown,
                h.buy_hold
            );
        }
        out
    }

    pub fn figure(&self) -> Figure {
        let h = &self.history;
        let x = dates(h.iter().map(|r| r.date));
        let col = |f: fn(&HistoryRow) -> f64| h.iter().map(f).collect::<Vec<f64>>();
        let (first, last) = (h[0].date, h[h.len() - 1].date);

        let mut layout = two_rows([0.8, 0.2], 0.05, None);
        merge(
            &mut layout,
            json!({
                "title": {"text": "Simulation d'Investissement BTC - Stratégie de Levier Dynamique"},
                "xaxis": {"range": [fmt_date(first), fmt_date(last)]},
                "xaxis2": {"title": {"text": "Date"}},
                "yaxis": {"title": {"text": "Capital (USD)"}},
                "yaxis2": {"title": {"text": "Drawdown (%)"}, "side": "left", "showgrid": true, "gridcolor": "rgba(255,255,255,0.05)"},
                "yaxis3": {"title": {"text": "Prix BTC (USD)"}, "overlaying": "y", "side": "right", "showgrid": false, "anchor": "x"},
                "height": 800, "hovermode": "x unified",
                "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1}
            }),
        );
        let mut fig = Figure::new(layout);
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Valeur Portefeuille (Stratégie)", "x": x, "y": col(|r| r.portfolio_value), "line": {"color": "#00FFAA", "width": 2.5}}));
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Buy & Hold BTC", "x": x, "y": col(|r| r.buy_hold), "line": {"color": "rgba(255, 165, 0, 0.6)", "width": 1.5, "dash": "dash"}}));
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Prix BTC", "x": x, "y": col(|r| r.btc_price), "yaxis": "y3", "line": {"color": "rgba(255, 255, 255, 0.1)", "width": 1}}));
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": "Drawdown (%)", "x": x, "y": col(|r| r.drawdown), "xaxis": "x2", "yaxis": "y2",
            "line": {"color": "#FF5555", "width": 1}, "fill": "tozeroy", "fillcolor": "rgba(255, 85, 85, 0.2)"}));

        for d in halving_dates()
            .into_iter()
            .filter(|d| *d >= first && *d <= last)
        {
            fig.vline(&fmt_date(d), "rgba(255, 0, 0, 0.3)", 1.0, "dot", "paper");
        }

        let value_on = |d: NaiveDate| h.iter().find(|r| r.date == d).map(|r| r.portfolio_value);
        let markers = |needle: &str| {
            let pts: Vec<(String, f64)> = self
                .trades
                .iter()
                .filter(|t| t.action.contains(needle))
                .filter_map(|t| Some((fmt_date(t.date), value_on(t.date)?)))
                .collect();
            (
                pts.iter().map(|p| p.0.clone()).collect::<Vec<_>>(),
                pts.iter().map(|p| p.1).collect::<Vec<_>>(),
            )
        };
        let (ex, ey) = markers("Passage en Levier");
        fig.trace(json!({"type": "scatter", "mode": "markers", "name": "Entrée Levier", "x": ex, "y": ey, "hoverinfo": "skip",
            "marker": {"color": "lime", "size": 12, "symbol": "triangle-up", "line": {"color": "white", "width": 1}}}));
        let (sx, sy) = markers("Fin de phase Levier");
        fig.trace(json!({"type": "scatter", "mode": "markers", "name": "Sortie Levier", "x": sx, "y": sy, "hoverinfo": "skip",
            "marker": {"color": "red", "size": 10, "symbol": "circle", "line": {"color": "white", "width": 1}}}));
        let (lx, ly) = markers("LIQUIDATION");
        if !lx.is_empty() {
            fig.trace(json!({"type": "scatter", "mode": "markers+text", "name": "Liquidation", "x": lx, "y": ly, "text": "💀",
                "textposition": "top center", "hoverinfo": "skip", "marker": {"color": "orange", "size": 15, "symbol": "x"}}));
        }
        fig
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd;
    use chrono::Duration;

    fn prices(values: &[f64]) -> Vec<PricePoint> {
        let start = parse_ymd("2020-01-01");
        values
            .iter()
            .enumerate()
            .map(|(i, &close)| PricePoint {
                date: start + Duration::days(i as i64),
                close,
            })
            .collect()
    }

    fn params(freq: ExitFrequency) -> SimParams {
        SimParams {
            start: parse_ymd("2020-01-01"),
            end: parse_ymd("2021-01-01"),
            initial_capital: 10_000.0,
            drop_pct: 10.0,
            target_leverage: 2.0,
            exit_frequency: freq,
            exit_pct: 10.0,
            ticker: "BTC-USD".into(),
        }
    }

    #[test]
    fn flat_price_stays_in_x1_mode() {
        let sim = simulate(&prices(&[100.0; 30]), &params(ExitFrequency::Weekly)).unwrap();
        assert!(sim.history.iter().all(|h| h.mode == Mode::X1));
        assert_eq!(sim.history.last().unwrap().portfolio_value, 10_000.0);
    }

    #[test]
    fn drop_triggers_leverage_switch() {
        let sim = simulate(
            &prices(&[100.0, 100.0, 100.0, 100.0, 100.0, 95.0, 90.0, 85.0, 80.0]),
            &params(ExitFrequency::Weekly),
        )
        .unwrap();
        assert!(sim.history.iter().any(|h| h.mode == Mode::XL));
        assert!(sim
            .trades
            .iter()
            .any(|t| t.action.contains("Passage en Levier")));
    }

    #[test]
    fn no_loss_rule_postpones_exit_below_purchase_price() {
        let mut p = vec![100.0; 3];
        p.push(80.0);
        p.extend([70.0; 30]);
        let sim = simulate(&prices(&p), &params(ExitFrequency::Daily)).unwrap();
        assert!(sim.trades.iter().any(|t| t.action.contains("reportée")));
        assert_eq!(sim.history.last().unwrap().mode, Mode::XL);
    }

    #[test]
    fn liquidation_zeroes_out_portfolio() {
        let mut p = vec![100.0; 3];
        p.push(80.0);
        p.extend([30.0; 5]);
        let sim = simulate(&prices(&p), &params(ExitFrequency::Weekly)).unwrap();
        assert!(sim.trades.iter().any(|t| t.action.contains("LIQUIDATION")));
        assert_eq!(sim.history.last().unwrap().portfolio_value, 0.0);
        assert!(sim.summary().liquidated);
    }

    #[test]
    fn full_exit_returns_to_x1_without_debt() {
        // Levier à 80, remontée à 120 : 10 étapes journalières puis retour en X1.
        let mut p = vec![100.0, 80.0];
        p.extend([120.0; 15]);
        let sim = simulate(&prices(&p), &params(ExitFrequency::Daily)).unwrap();
        assert!(sim.trades.iter().any(|t| t.action == "Fin de phase Levier"));
        let last = sim.history.last().unwrap();
        assert_eq!(last.mode, Mode::X1);
        assert!((last.portfolio_value - last.btc_units * 120.0).abs() < 1e-6);
        assert!(
            last.portfolio_value > last.buy_hold,
            "le levier sur la remontée surperforme"
        );
    }

    #[test]
    fn invalid_dates_are_rejected() {
        let mut p = params(ExitFrequency::Weekly);
        p.end = p.start;
        assert!(simulate(&prices(&[1.0]), &p).is_err());
    }

    #[test]
    fn nan_and_out_of_range_params_are_rejected() {
        let base = params(ExitFrequency::Weekly);
        let bad = [
            SimParams {
                initial_capital: f64::NAN,
                ..base.clone()
            },
            SimParams {
                target_leverage: 0.5,
                ..base.clone()
            },
            SimParams {
                drop_pct: f64::NAN,
                ..base.clone()
            },
            SimParams {
                exit_pct: 0.5,
                ..base.clone()
            },
            SimParams {
                ticker: " ".into(),
                ..base.clone()
            },
        ];
        for p in bad {
            assert!(p.validate().is_err(), "{p:?}");
        }
        assert!(base.validate().is_ok());
    }

    #[test]
    fn money_format() {
        assert_eq!(money(1234567.891), "$1,234,567.89");
        assert_eq!(money(-5.0), "-$5.00");
        assert_eq!(money(999.999), "$1,000.00");
    }

    #[test]
    fn csv_has_header_and_one_line_per_day() {
        let sim = simulate(&prices(&[100.0; 3]), &params(ExitFrequency::Weekly)).unwrap();
        let csv = sim.to_csv();
        assert_eq!(csv.lines().count(), 4);
        assert!(csv.starts_with("Date,BTC_Price"));
        assert!(csv
            .lines()
            .nth(1)
            .unwrap()
            .starts_with("2020-01-01,100,10000,X1,"));
    }
}

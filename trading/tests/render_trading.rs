//! Pages Trading rendues de bout en bout à partir d'un marché synthétique mis
//! en cache (sans réseau). Avec `TRADING_SAMPLES_HTML=chemin.html`, écrit
//! aussi une page affichant les figures avec Plotly.js, pour contrôle visuel.

use chrono::{NaiveDate, Utc};
use serde_json::Value;
use trading_engine::backtest::{run_with_feed, BacktestParams};
use trading_engine::market::{
    date_ms, Bar, FundingRate, Instrument, Interval, Market, DAY_MS, FUNDING_PERIOD_MS,
};
use trading_engine::signals::{report_with_feed, SignalParams};
use trading_engine::strategy::TrendVariant;
use trading_engine::MarketFeed;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Marché journalier déterministe : cycles de 4 ans (hausse puis baisse),
/// volatilité d'environ 2 % par jour, mèches aléatoires.
fn synthetic(start: &str, price0: f64, seed: u64) -> Vec<Bar> {
    let t0 = date_ms(d(start));
    let end = Utc::now().timestamp_millis() - DAY_MS;
    let mut state = seed;
    let mut uniform = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as f64 / u64::MAX as f64 - 0.5
    };
    let mut price = price0;
    let mut bars = Vec::new();
    let mut i = 0i64;
    while t0 + i * DAY_MS < end {
        let years = i as f64 / 365.25;
        let drift = 0.0025 * (years * std::f64::consts::TAU / 4.0).sin();
        let noise = (uniform() + uniform() + uniform()) * 0.04;
        let open = price;
        let close = open * (drift + noise).exp();
        bars.push(Bar {
            time: t0 + i * DAY_MS,
            open,
            high: open.max(close) * (1.0 + 0.01 * (uniform() + 0.5)),
            low: open.min(close) * (1.0 - 0.01 * (uniform() + 0.5)),
            close,
            volume: 1e8,
        });
        price = close;
        i += 1;
    }
    bars
}

fn backtest_params(market: Market, variant: TrendVariant, start: &str) -> BacktestParams {
    BacktestParams {
        base: "BTC".into(),
        market,
        interval: Interval::D1,
        start: d(start),
        end: Utc::now().date_naive(),
        initial_capital: 10_000.0,
        variant,
        target_vol_pct: 25.0,
        max_leverage: if market == Market::Perp { 2.0 } else { 1.0 },
        stop_atr_mult: 3.0,
        fee_pct: 0.075,
        slippage_pct: 0.05,
        rebalance_pct: 20.0,
        funding_pct_8h: 0.01,
        halt_on_breach: false,
    }
}

#[tokio::test]
async fn trading_pages_render_from_cached_data() {
    let today = Utc::now().date_naive();
    let feed = MarketFeed::new();
    let spot = Instrument::new("BTC", Market::Spot).unwrap();
    let perp = Instrument::new("BTC", Market::Perp).unwrap();
    let spot_bars = synthetic("2015-01-01", 300.0, 0x9E37_79B9_7F4A_7C15);
    // Le perpétuel n'a d'historique Poloniex que depuis 2023.
    let perp_start = date_ms(d("2023-01-01"));
    let perp_bars: Vec<Bar> = spot_bars
        .iter()
        .copied()
        .filter(|b| b.time >= perp_start)
        .collect();
    // Funding des 180 derniers jours seulement, comme l'API.
    let now = Utc::now().timestamp_millis();
    let first_period = (now - 180 * DAY_MS) / FUNDING_PERIOD_MS + 1;
    let funding: Vec<FundingRate> = (first_period..=now / FUNDING_PERIOD_MS)
        .map(|k| FundingRate {
            time: k * FUNDING_PERIOD_MS,
            rate: 0.0001,
        })
        .collect();
    feed.seed_funding(&perp, funding);

    let cases = [
        (&spot, &spot_bars, "2018-01-01"),
        (&perp, &perp_bars, "2023-09-01"),
    ];
    let mut figures: Vec<(String, Value)> = Vec::new();
    for (inst, bars, start) in cases {
        for variant in [TrendVariant::Donchian, TrendVariant::Ema] {
            let sp = SignalParams {
                base: inst.base.clone(),
                market: inst.market,
                interval: Interval::D1,
                variant,
            };
            let bp = backtest_params(inst.market, variant, start);
            feed.seed_bars(inst, Interval::D1, sp.data_start(today), bars.clone());
            feed.seed_bars(inst, Interval::D1, bp.data_start(), bars.clone());
            let tag = format!("{}-{variant:?}", inst.symbol());

            let report = report_with_feed(&feed, &sp, today)
                .await
                .unwrap_or_else(|e| panic!("{tag} : {e}"));
            assert!(report.score.is_some(), "{tag}");
            assert!(
                report.target_exposure.abs() <= report.max_leverage + 1e-9,
                "{tag}"
            );
            if inst.market == Market::Spot {
                assert!(report.target_exposure >= 0.0, "{tag}");
            }
            assert!(!report.chart.data.is_empty());
            figures.push((
                format!("Signaux {tag}"),
                serde_json::to_value(&report.chart).unwrap(),
            ));

            let bt = run_with_feed(&feed, &bp)
                .await
                .unwrap_or_else(|e| panic!("{tag} : {e}"));
            assert!(
                bt.points.first().unwrap().time >= date_ms(bp.start),
                "{tag}"
            );
            assert!(
                bt.points
                    .iter()
                    .all(|p| p.equity.is_finite() && p.equity > 0.0),
                "{tag}"
            );
            let cap = bp.leverage_cap();
            // L'exposition dérive avec les cours entre deux rééquilibrages : marge de 50 %.
            assert!(
                bt.points.iter().all(|p| p.exposure.abs() <= cap * 1.5),
                "{tag}"
            );
            if inst.market == Market::Spot {
                assert!(
                    bt.points
                        .iter()
                        .all(|p| p.exposure >= 0.0 && p.exposure <= 1.0 + 1e-9),
                    "{tag}"
                );
            } else {
                assert!(
                    bt.funding_real_periods + bt.funding_estimated_periods > 0,
                    "{tag}"
                );
            }
            assert!(!bt.fills.is_empty(), "{tag} : aucun ordre");
            let s = bt.summary();
            assert!(s.strategy.max_drawdown_pct <= 0.0 && s.strategy.max_drawdown_pct > -100.0);
            assert!(s.strategy.sharpe.is_some_and(f64::is_finite), "{tag}");
            assert!(s.exposure_pct > 0.0 && s.exposure_pct <= 100.0);
            let last = bt.points.last().unwrap();
            assert!((s.buy_hold.final_equity - last.buy_hold).abs() < 1e-6);
            figures.push((
                format!(
                    "Backtest {tag} : CAGR {:.1} % (B&H {:.1} %), DD max {:.1} % (B&H {:.1} %), {} ordres, frais {:.0}, funding {:.0}",
                    s.strategy.cagr_pct.unwrap_or(f64::NAN),
                    s.buy_hold.cagr_pct.unwrap_or(f64::NAN),
                    s.strategy.max_drawdown_pct,
                    s.buy_hold.max_drawdown_pct,
                    s.fills,
                    s.fees,
                    s.funding
                ),
                serde_json::to_value(trading_engine::chart::backtest_chart(&bt)).unwrap(),
            ));
        }
    }

    if let Ok(path) = std::env::var("TRADING_SAMPLES_HTML") {
        let plotly = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/vendor/plotly.min.js");
        let mut html = format!("<!doctype html><meta charset=utf-8><body style='background:#0e1117;color:#eee;font-family:sans-serif'><script src='file://{plotly}'></script>");
        for (k, (title, fig)) in figures.iter().enumerate() {
            html.push_str(&format!("<h3>{title}</h3><div id='f{k}'></div><script>(function(f){{f.layout.template={{layout:{{paper_bgcolor:'#111',plot_bgcolor:'#111',font:{{color:'#f2f5fa'}},xaxis:{{gridcolor:'#283442'}},yaxis:{{gridcolor:'#283442'}}}}}};f.layout.width=1200;Plotly.newPlot('f{k}',f.data,f.layout)}})({fig});</script>"));
        }
        std::fs::write(path, html).unwrap();
    }
}

#[tokio::test]
async fn backtest_reports_missing_history() {
    let feed = MarketFeed::new();
    let perp = Instrument::new("ETH", Market::Perp).unwrap();
    let bp = BacktestParams {
        base: "ETH".into(),
        ..backtest_params(Market::Perp, TrendVariant::Donchian, "2026-02-01")
    };
    feed.seed_bars(
        &perp,
        Interval::D1,
        bp.data_start(),
        synthetic("2026-01-01", 2000.0, 7),
    );
    feed.seed_funding(&perp, Vec::new());
    // Historique depuis 2026 seulement : le préchauffage (201 bougies) repousse le début, ou l'empêche.
    match run_with_feed(&feed, &bp).await {
        Ok(bt) => assert!(bt.warnings.iter().any(|w| w.contains("Début décalé"))),
        Err(e) => assert!(e.contains("Historique insuffisant"), "{e}"),
    }
}

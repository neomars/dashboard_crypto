//! Pages Trading rendues de bout en bout à partir d'un marché synthétique mis
//! en cache (sans réseau). Avec `TRADING_SAMPLES_HTML=chemin.html`, écrit
//! aussi une page affichant les figures avec Plotly.js, pour contrôle visuel.

use chrono::{Duration, NaiveDate, Utc};
use serde_json::Value;
use trading_engine::accumulation::{
    self, default_tier_bounds, default_tier_multipliers, AccumulationParams, Frequency, Weighting,
};
use trading_engine::backtest::{run_with_feed, BacktestParams};
use trading_engine::chart::accumulation_chart;
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
        for variant in [
            TrendVariant::Donchian,
            TrendVariant::Ema,
            TrendVariant::Breakout,
        ] {
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
            // Résultats en BTC : la référence « garder » vaut ×1,00 par construction.
            assert!(s.btc.start > 0.0 && s.btc.end > 0.0, "{tag}");
            assert!((s.btc.multiplier - s.btc.end / s.btc.start).abs() < 1e-12);
            assert!(s.btc.max_drawdown_pct <= 0.0 && s.btc.max_drawdown_pct > -100.0);
            assert!((0.0..=100.0).contains(&s.btc.time_out_pct));
            assert!(bt.to_csv().lines().next().unwrap().ends_with(",Equity_BTC"));
            let figure = serde_json::to_value(trading_engine::chart::backtest_chart(&bt)).unwrap();
            let names: Vec<&str> = figure["data"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|t| t["name"].as_str())
                .collect();
            assert_eq!(
                &names[..2],
                ["Nombre de BTC", "Garder ses BTC (×1,00)"],
                "{tag}"
            );
            if let (Ok(path), "BTC_USDT-Breakout") =
                (std::env::var("TRADING_BACKTEST_JSON"), tag.as_str())
            {
                let view = serde_json::json!({
                    "symbol": bt.symbol, "chart": trading_engine::chart::backtest_chart(&bt),
                    "summary": s, "fills": bt.fills, "events": bt.events, "warnings": bt.warnings,
                });
                std::fs::write(path, view.to_string()).unwrap();
            }
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

    write_samples("", &figures);
}

/// Avec `TRADING_SAMPLES_HTML=chemin.html`, écrit une page affichant les
/// figures avec Plotly.js (`suffixe` s'ajoute au nom du fichier).
fn write_samples(suffix: &str, figures: &[(String, Value)]) {
    if let Ok(path) = std::env::var("TRADING_SAMPLES_HTML") {
        let path = path.replace(".html", &format!("{suffix}.html"));
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
    // Un actif autre que BTC est aussi compté en BTC : il faut les cours de BTC_USDT.
    let btc = Instrument::new("BTC", Market::Spot).unwrap();
    feed.seed_bars(
        &btc,
        Interval::D1,
        bp.data_start(),
        synthetic("2020-01-01", 8000.0, 9),
    );
    // Historique depuis 2026 seulement : le préchauffage (201 bougies) repousse le début, ou l'empêche.
    match run_with_feed(&feed, &bp).await {
        Ok(bt) => assert!(bt.warnings.iter().any(|w| w.contains("Début décalé"))),
        Err(e) => assert!(e.contains("Historique insuffisant"), "{e}"),
    }
}

fn accumulation_params(weighting: Weighting, start: &str) -> AccumulationParams {
    AccumulationParams {
        start: d(start),
        end: Utc::now().date_naive(),
        base_amount: 100.0,
        frequency: Frequency::Weekly,
        weekday: 1,
        month_day: 1,
        weighting,
        tier_bounds: default_tier_bounds(),
        tier_multipliers: default_tier_multipliers(),
        max_multiplier: 3.0,
        fee_pct: 0.075,
        slippage_pct: 0.05,
        initial_btc: 0.0,
    }
}

#[tokio::test]
async fn accumulation_page_renders_from_cached_data() {
    let today = Utc::now().date_naive();
    let feed = MarketFeed::new();
    let btc = Instrument::new("BTC", Market::Spot).unwrap();
    let bars = synthetic("2017-01-01", 1000.0, 0x1234_5678_9ABC_DEF1);
    let first = accumulation_params(Weighting::MvrvTiers, "2018-01-01");
    feed.seed_bars(&btc, Interval::D1, first.data_start(), bars.clone());
    // MVRV synthétique : cycle de 4 ans entre 0,8 et 3,5. L'historique s'arrête
    // 150 jours avant aujourd'hui, les données récentes 8 jours avant (retard
    // du plan gratuit BGeometrics) : les derniers achats passent en secours.
    let mvrv_at = |day: NaiveDate| {
        let years = (day - d("2017-01-01")).num_days() as f64 / 365.25;
        2.15 + 1.35 * (years * std::f64::consts::TAU / 4.0).sin()
    };
    let series = |from: NaiveDate, to: NaiveDate| -> Vec<(NaiveDate, f64)> {
        (0..=(to - from).num_days())
            .map(|k| from + Duration::days(k))
            .map(|day| (day, mvrv_at(day)))
            .collect()
    };
    feed.seed_mvrv(
        series(d("2017-01-01"), today - Duration::days(150)),
        series(today - Duration::days(180), today - Duration::days(8)),
    );

    let mut figures: Vec<(String, Value)> = Vec::new();
    for weighting in [
        Weighting::Fixed,
        Weighting::MvrvTiers,
        Weighting::MvrvInverse,
        Weighting::PriceMa200,
    ] {
        let params = accumulation_params(weighting, "2018-01-01");
        let acc = accumulation::run_with_feed(&feed, &params, today)
            .await
            .unwrap_or_else(|e| panic!("{weighting:?} : {e}"));
        let s = &acc.summary;
        assert!(s.purchases > 400, "{weighting:?} : {} achats", s.purchases);
        assert!(s.btc_bought > 0.0 && s.spent > 0.0);
        assert!(acc.purchases.iter().all(|p| p.weight <= 3.0 + 1e-12));
        assert!(
            acc.points.windows(2).all(|w| w[1].btc >= w[0].btc),
            "le stack ne diminue jamais"
        );
        // Chronologie : le MVRV d'un achat est daté de la veille au plus tard.
        for p in &acc.purchases {
            if let Some(m) = p.mvrv {
                let day = d(&p.date) - Duration::days(1);
                assert!((m - mvrv_at(day)).abs() < 3.0, "{}", p.date);
            }
        }
        if weighting == Weighting::Fixed {
            assert!(s.equal_budget.extra_btc.abs() < 1e-9);
        }
        if weighting.uses_mvrv() {
            // Dernier MVRV : il y a 8 jours (> 3) : avertissement et secours.
            assert!(
                acc.warnings.iter().any(|w| w.contains("plus de 3")),
                "{weighting:?} : {:?}",
                acc.warnings
            );
            assert!(
                s.fallback_purchases >= 1 && s.fallback_purchases <= 2,
                "{}",
                s.fallback_purchases
            );
            assert!(acc.mvrv.as_ref().unwrap().overlap_days > 0);
        } else {
            assert!(acc.mvrv.is_none());
        }
        let chart = accumulation_chart(&acc);
        assert_eq!(chart.layout["yaxis"]["type"], "log");
        let n_traces = chart.data.len();
        assert_eq!(n_traces, if weighting.uses_mvrv() { 7 } else { 6 });
        assert_eq!(
            chart.data[1]["marker"]["size"].as_array().unwrap().len(),
            s.purchases
        );
        if weighting == Weighting::MvrvTiers {
            // Même forme que la commande Tauri `run_accumulation`, pour essayer l'interface.
            if let Ok(path) = std::env::var("TRADING_VIEWS_JSON") {
                let view = serde_json::json!({
                    "chart": chart, "summary": acc.summary, "purchases": acc.purchases,
                    "mvrv": acc.mvrv, "warnings": acc.warnings,
                });
                std::fs::write(path, view.to_string()).unwrap();
            }
        }
        figures.push((
            format!(
                "Accumulation {weighting:?} : {} achats, {:.0} USDT, {:.4} BTC ; à budget égal {:+.4} BTC ({:+.1} %)",
                s.purchases,
                s.spent,
                s.btc_bought,
                s.equal_budget.extra_btc,
                s.equal_budget.extra_pct.unwrap_or(f64::NAN)
            ),
            serde_json::to_value(&chart).unwrap(),
        ));
    }
    write_samples("-accumulation", &figures);
}

#[tokio::test]
async fn accumulation_without_any_mvrv_falls_back_to_the_average() {
    let today = Utc::now().date_naive();
    let feed = MarketFeed::new();
    let btc = Instrument::new("BTC", Market::Spot).unwrap();
    let params = accumulation_params(Weighting::MvrvInverse, "2019-01-01");
    feed.seed_bars(
        &btc,
        Interval::D1,
        params.data_start(),
        synthetic("2017-01-01", 1000.0, 42),
    );
    // Aucune donnée MVRV (les deux sources ont échoué) : secours sur tout l'historique.
    feed.seed_mvrv(Vec::new(), Vec::new());
    let acc = accumulation::run_with_feed(&feed, &params, today)
        .await
        .unwrap();
    assert_eq!(acc.summary.fallback_purchases, acc.summary.purchases);
    assert!(acc.warnings.iter().any(|w| w.contains("faute de MVRV")));
    assert!(acc
        .warnings
        .iter()
        .any(|w| w.contains("plus de 3") || w.contains("Aucune donnée MVRV")));
}

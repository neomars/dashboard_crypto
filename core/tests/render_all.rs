//! Rendu de tous les indicateurs à partir de données synthétiques (sans
//! réseau). Avec `RENDER_SAMPLES_HTML=chemin.html`, écrit aussi une page HTML
//! affichant toutes les figures avec Plotly.js, pour contrôle visuel.

use chrono::{Duration, NaiveDate};
use dashboard_core::config::Config;
use dashboard_core::dune::Row;
use dashboard_core::indicators::{self, IDS};
use dashboard_core::series::{Candle, PricePoint};
use dashboard_core::DataProvider;
use serde_json::{json, Value};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Trajectoire déterministe : croissance exponentielle + cycles de 4 ans.
fn price_at(day: i64, scale: f64) -> f64 {
    let t = day as f64 / 365.25;
    scale
        * (0.9 * t
            + 1.2 * (t * std::f64::consts::TAU / 4.0).sin()
            + 0.08 * (day as f64 / 9.0).sin())
        .exp()
}

fn candles(start: &str, scale: f64) -> Vec<Candle> {
    let start = d(start);
    let end = chrono::Utc::now().date_naive();
    let origin = d("2010-07-01");
    (0..=(end - start).num_days())
        .map(|i| {
            let date = start + Duration::days(i);
            let day = (date - origin).num_days();
            let (open, close) = (price_at(day - 1, scale), price_at(day, scale));
            Candle {
                date,
                open,
                high: open.max(close) * 1.02,
                low: open.min(close) * 0.98,
                close,
                volume: 1e9 + (i % 50) as f64 * 1e7,
            }
        })
        .collect()
}

fn rows(values: Vec<Value>) -> Vec<Row> {
    values
        .into_iter()
        .map(|v| v.as_object().unwrap().clone())
        .collect()
}

fn days(from: &str, step: i64) -> impl Iterator<Item = NaiveDate> {
    let end = chrono::Utc::now().date_naive();
    let start = d(from);
    (0..)
        .map(move |i| start + Duration::days(i * step))
        .take_while(move |x| *x <= end)
}

fn provider() -> DataProvider {
    let dir = std::env::temp_dir().join(format!("dashboard-crypto-render-{}", std::process::id()));
    let config = Config::new(dir.join("config.ini"));
    config.save_dune_api_key("test").unwrap();
    config
        .set("DUNE_QUERIES", "net_realized_pnl", "999")
        .unwrap();
    let data = DataProvider::new(config);

    let btc = candles("2014-09-17", 0.05);
    data.seed_combined_btc(
        candles("2010-07-18", 0.05)
            .iter()
            .map(|c| PricePoint {
                date: c.date,
                close: c.close,
            })
            .collect(),
    );
    data.seed_ticker_history("BTC-USD", btc);
    data.seed_ticker_history("ETH-USD", candles("2017-11-09", 0.004));
    data.seed_fear_greed(
        days("2018-02-01", 1)
            .enumerate()
            .map(|(i, x)| (x, (50.0 + 45.0 * (i as f64 / 40.0).sin()).round()))
            .collect(),
    );

    let ts = |x: NaiveDate| format!("{x} 00:00:00.000 UTC");
    data.seed_dune_results("6764134", rows(days("2020-01-01", 1).enumerate().map(|(i, x)| json!({
        "day": ts(x), "sth_sopr": 1.0 + 0.05 * (i as f64 / 20.0).sin(), "lth_sopr": 1.5 + 0.8 * (i as f64 / 200.0).sin()
    })).collect()));
    data.seed_dune_results("3382000", rows(days("2024-01-11", 7).enumerate().flat_map(|(i, x)| {
        [("IBIT", 30_000.0), ("FBTC", 12_000.0), ("GBTC", 8_000.0)].map(|(t, k)| json!({"time": ts(x), "etf_ticker": t, "tvl": k * (1.0 + i as f64 / 20.0)}))
    }).collect()));
    data.seed_dune_results("3089944", rows(days("2023-06-01", 1).enumerate().flat_map(|(i, x)| {
        ["BTC/USD [BTC-USDC]", "ETH/USD [ETH-USDC]"].map(|m| json!({"date": ts(x), "market_symbol": m,
            "long_oi_usd": 5e7 + 2e7 * (i as f64 / 15.0).sin(), "short_oi_usd": 4e7 + 1e7 * (i as f64 / 11.0).cos()}))
    }).collect()));
    data.seed_dune_results(
        "7611528",
        rows(
            days("2016-01-01", 30)
                .map(|x| {
                    let mut r = json!({"month": ts(x)});
                    for b in [
                        "0d-1d", "1d-1w", "1w-1m", "1m-3m", "3m-6m", "6m-12m", "12m-18m", "18m-2y",
                        "2y-3y", "3y-5y", "5y-7y", "7y-10y", "10y+",
                    ] {
                        r[b] = json!(100.0 / 13.0);
                    }
                    r
                })
                .collect(),
        ),
    );
    data.seed_dune_results("999", rows(days("2023-01-02", 7).enumerate().map(|(i, x)| {
        let s = (i as f64 / 8.0).sin();
        json!({"week": ts(x), "realized_profit_usd": 1e9 * (1.0 + s.max(0.0) * 3.0), "realized_loss_usd": 1e9 * (1.0 + (-s).max(0.0) * 3.0)})
    }).collect()));
    data
}

#[tokio::test]
async fn every_indicator_renders_from_cached_data() {
    let data = provider();
    let mut figures = Vec::new();
    for id in IDS {
        let params = match id {
            "long_short" => json!({"pair": "ETH", "mode": "Ratio Long/Short"}),
            "bmsb" => json!({"sma": 20, "ema": 21}),
            _ => Value::Null,
        };
        let out = indicators::render(id, &params, &data)
            .await
            .unwrap_or_else(|e| panic!("{id} : {e}"));
        assert!(!out.figure.data.is_empty(), "{id} : figure vide");
        figures.push((id, serde_json::to_value(&out.figure).unwrap()));
    }

    let btc: Vec<PricePoint> = data
        .ticker_range("BTC-USD", Some(d("2017-01-01")), None)
        .await
        .unwrap()
        .iter()
        .map(|c| PricePoint {
            date: c.date,
            close: c.close,
        })
        .collect();
    let params = dashboard_core::simulator::SimParams {
        start: d("2017-01-01"),
        end: chrono::Utc::now().date_naive(),
        initial_capital: 10_000.0,
        drop_pct: 10.0,
        target_leverage: 2.0,
        exit_frequency: dashboard_core::simulator::ExitFrequency::Weekly,
        exit_pct: 10.0,
        ticker: "BTC-USD".into(),
    };
    let sim = dashboard_core::simulator::simulate(&btc, &params).unwrap();
    assert!(sim.trades.iter().any(|t| t.action.contains("Levier")));
    figures.push(("simulator", serde_json::to_value(sim.figure()).unwrap()));
    if let Ok(path) = std::env::var("RENDER_SAMPLES_PDF") {
        std::fs::write(path, dashboard_core::pdf::simulation_report(&sim)).unwrap();
    }

    if let Ok(path) = std::env::var("RENDER_SAMPLES_HTML") {
        let plotly = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/vendor/plotly.min.js");
        let mut html = format!("<!doctype html><meta charset=utf-8><body style='background:#0e1117;color:#eee;font-family:sans-serif'><script src='file://{plotly}'></script>");
        for (id, fig) in &figures {
            html.push_str(&format!("<h2>{id}</h2><div id='{id}'></div><script>(function(f){{f.layout.template={{layout:{{paper_bgcolor:'#111',plot_bgcolor:'#111',font:{{color:'#f2f5fa'}},xaxis:{{gridcolor:'#283442'}},yaxis:{{gridcolor:'#283442'}}}}}};Plotly.newPlot('{id}',f.data,f.layout)}})({fig});</script>"));
        }
        std::fs::write(path, html).unwrap();
    }
}

#[tokio::test]
async fn long_short_modes_and_bmsb_regime() {
    let data = provider();
    for mode in ["Long vs Short", "Ratio Long/Short", "Open Interest Cumulé"] {
        let out = indicators::render("long_short", &json!({"pair": "BTC", "mode": mode}), &data)
            .await
            .unwrap();
        assert!(out.figure.layout["title"]["text"]
            .as_str()
            .unwrap()
            .starts_with("BTC"));
    }
    let err = indicators::render("long_short", &json!({"pair": "SOL"}), &data)
        .await
        .unwrap_err();
    assert!(err.contains("SOL"), "{err}");

    let out = indicators::render("bmsb", &json!({"sma": 30, "ema": 40}), &data)
        .await
        .unwrap();
    assert_eq!(out.metrics[0].label, "Régime actuel");
    assert!(out.figure.data[1]["name"]
        .as_str()
        .unwrap()
        .starts_with("30-week"));
}

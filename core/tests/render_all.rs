//! Rendu de tous les indicateurs à partir de données synthétiques (sans
//! réseau). Avec `RENDER_SAMPLES_HTML=chemin.html`, écrit aussi une page HTML
//! affichant toutes les figures avec Plotly.js, pour contrôle visuel.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use dashboard_core::config::Config;
use dashboard_core::indicators::{self, mstr_mnav, IDS};
use dashboard_core::series::{Candle, PricePoint};
use dashboard_core::table::Row;
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

/// mNAV cible de la série MSTR synthétique (entre ~0,7 et ~2,5).
fn target_mnav(date: NaiveDate) -> f64 {
    let t = (date - d("2020-08-01")).num_days() as f64;
    1.6 + 0.9 * (t / 240.0).sin()
}

/// Cours MSTR synthétique (jours ouvrés seulement) donnant exactement
/// `target_mnav` avec les holdings du fichier embarqué et le BTC synthétique.
fn mstr_candles(btc: &[Candle]) -> Vec<Candle> {
    let holdings = mstr_mnav::forward_fill(&mstr_mnav::embedded_points());
    btc.iter()
        .filter(|c| !matches!(c.date.weekday(), Weekday::Sat | Weekday::Sun))
        .filter_map(|c| {
            let h = mstr_mnav::holdings_at(&holdings, c.date)?;
            let close = target_mnav(c.date) * h.btc * c.close / h.shares;
            Some(Candle {
                date: c.date,
                open: close,
                high: close,
                low: close,
                close,
                volume: 1e6,
            })
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
    let data =
        DataProvider::new(Config::new(dir.join("config.ini"))).with_cache_dir(dir.join("cache"));

    data.seed_combined_btc(
        candles("2010-07-18", 0.05)
            .iter()
            .map(|c| PricePoint {
                date: c.date,
                close: c.close,
            })
            .collect(),
    );
    data.seed_ticker_history("BTC-USD", candles("2014-09-17", 0.05));
    data.seed_ticker_history("ETH-USD", candles("2017-11-09", 0.004));
    data.seed_ticker_history("MSTR", mstr_candles(&candles("2014-09-17", 0.05)));
    data.seed_fear_greed(
        days("2018-02-01", 1)
            .enumerate()
            .map(|(i, x)| (x, (50.0 + 45.0 * (i as f64 / 40.0).sin()).round()))
            .collect(),
    );

    // Format BGeometrics : [{"d": "AAAA-MM-JJ", "unixTs": ..., "<métrique>": ...}]
    let bg = |from: &str, step: i64, value: &dyn Fn(usize) -> Value| -> Vec<Row> {
        rows(
            days(from, step)
                .enumerate()
                .map(|(i, x)| {
                    let mut r = value(i);
                    r["d"] = json!(x.to_string());
                    r["unixTs"] = json!(x.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
                    r
                })
                .collect(),
        )
    };
    data.seed_bgeometrics(
        "sth_sopr",
        bg(
            "2022-10-01",
            1,
            &|i| json!({"sthSopr": 1.0 + 0.05 * (i as f64 / 20.0).sin()}),
        ),
    );
    data.seed_bgeometrics(
        "lth_sopr",
        bg(
            "2022-10-01",
            1,
            &|i| json!({"lthSopr": 1.5 + 0.8 * (i as f64 / 200.0).sin()}),
        ),
    );
    data.seed_bgeometrics("etf", bg("2024-01-11", 1, &|i| {
        let k = 1.0 + i as f64 / 200.0;
        json!({"ibit": 30_000.0 * k, "fbtc": 12_000.0 * k, "gbtc": 8_000.0 * k, "etfBtcTotal": 50_000.0 * k})
    }));
    data.seed_bgeometrics(
        "realized_cap_hodl_waves",
        bg("2022-10-01", 1, &|i| {
            let mut r = json!({});
            for (k, b) in [
                "24h", "1d_1w", "1w_1m", "1m_3m", "3m_6m", "6m_12m", "1y_2y", "2y_3y", "3y_5y",
                "5y_7y", "7y_10y", "10y",
            ]
            .iter()
            .enumerate()
            {
                r[*b] = json!(1.0 + ((i + 40 * k) as f64 / 90.0).sin().abs());
            }
            r
        }),
    );
    data.seed_bgeometrics(
        "nrpl",
        bg(
            "2022-10-01",
            1,
            &|i| json!({"nrpl": 3e8 * (i as f64 / 50.0).sin() + 5e7}),
        ),
    );

    // Format OKX : un point par jour, [ratio] ou [contrats, actif de base, USD]
    for pair in ["BTC", "ETH"] {
        let pts: Vec<NaiveDate> = days("2023-10-01", 1).collect();
        data.seed_okx(
            "long-short-account-ratio-contract",
            pair,
            pts.iter()
                .enumerate()
                .map(|(i, x)| (*x, vec![1.0 + 0.6 * (i as f64 / 15.0).sin()]))
                .collect(),
        );
        data.seed_okx(
            "open-interest-history",
            pair,
            pts.iter()
                .enumerate()
                .map(|(i, x)| {
                    let usd = 3e9 + 1e9 * (i as f64 / 30.0).sin();
                    (*x, vec![usd / 1e3, usd / 6e4, usd])
                })
                .collect(),
        );
    }
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
async fn mstr_mnav_matches_known_ratio_on_trading_days() {
    let data = provider();
    let out = indicators::render("mstr_mnav", &json!({"btc": true}), &data)
        .await
        .unwrap();
    let fig = serde_json::to_value(&out.figure).unwrap();
    let mnav_trace = fig["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "mNAV")
        .unwrap();
    let xs = mnav_trace["x"].as_array().unwrap();
    let ys = mnav_trace["y"].as_array().unwrap();
    assert!(xs.len() > 1000);
    for (x, y) in xs.iter().zip(ys) {
        let date = d(x.as_str().unwrap());
        assert!(
            !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
            "{date} : jours de cotation MSTR seulement"
        );
        assert!(
            (y.as_f64().unwrap() - target_mnav(date)).abs() < 1e-9,
            "{date}"
        );
    }
    assert_eq!(out.metrics[0].label, "mNAV actuel");
    assert!(fig["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["yaxis"] == "y3"));

    let without_btc = indicators::render("mstr_mnav", &json!({"btc": false}), &data)
        .await
        .unwrap();
    assert!(!without_btc.figure.data.iter().any(|t| t["yaxis"] == "y3"));
}

#[tokio::test]
async fn long_short_modes_and_bmsb_regime() {
    let data = provider();
    for mode in ["Long vs Short", "Ratio Long/Short", "Open Interest"] {
        let out = indicators::render("long_short", &json!({"pair": "BTC", "mode": mode}), &data)
            .await
            .unwrap();
        assert!(out.figure.layout["title"]["text"]
            .as_str()
            .unwrap()
            .starts_with("BTC"));
    }
    let err = indicators::render("long_short", &json!({"pair": "XRP"}), &data)
        .await
        .unwrap_err();
    assert!(err.contains("XRP"), "{err}");

    let out = indicators::render("bmsb", &json!({"sma": 30, "ema": 40}), &data)
        .await
        .unwrap();
    assert_eq!(out.metrics[0].label, "Régime actuel");
    assert!(out.figure.data[1]["name"]
        .as_str()
        .unwrap()
        .starts_with("30-week"));
}

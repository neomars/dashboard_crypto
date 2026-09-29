use super::IndicatorOutput;
use crate::data::{halving_dates, DataProvider};
use crate::figure::Figure;
use crate::series::{fmt_date, PricePoint};
use chrono::Datelike;
use serde_json::json;

pub(crate) struct CycleRoi {
    pub name: String,
    pub days: Vec<i64>,
    pub roi: Vec<f64>,
    pub dates: Vec<String>,
}

/// Trajectoire du prix (multiplicateur) depuis chaque top de cycle.
pub(crate) fn cycle_rois(btc: &[PricePoint], halvings: &[chrono::NaiveDate]) -> Vec<CycleRoi> {
    let (tops, _) = super::halving::tops_and_bottoms(btc, halvings);
    tops.iter()
        .map(|top| {
            let after: Vec<&PricePoint> = btc.iter().filter(|p| p.date >= top.date).collect();
            CycleRoi {
                name: format!("Cycle {}", top.date.year()),
                days: after
                    .iter()
                    .map(|p| (p.date - top.date).num_days())
                    .collect(),
                roi: after.iter().map(|p| p.close / top.close).collect(),
                dates: after.iter().map(|p| fmt_date(p.date)).collect(),
            }
        })
        .collect()
}

pub async fn render(data: &DataProvider) -> Result<IndicatorOutput, String> {
    let btc = data.combined_btc_history().await?;
    let mut halvings = halving_dates();
    halvings.push(data.estimate_next_halving().await);

    let mut fig = Figure::new(json!({
        "title": {"text": "Bitcoin Market Cycle ROI (Performance depuis le Top)"},
        "xaxis": {"title": {"text": "Jours depuis le top du cycle"}},
        "yaxis": {"title": {"text": "ROI (Multiplicateur)"}},
        "height": 700, "hovermode": "x unified",
        "legend": {"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1},
        "margin": {"t": 100, "l": 80}
    }));
    let colors = ["#00FF00", "#00BFFF", "#FFD700", "#FF00FF", "#FFFFFF"];
    for (i, c) in cycle_rois(&btc, &halvings).into_iter().enumerate() {
        fig.trace(json!({"type": "scatter", "mode": "lines", "name": c.name, "x": c.days, "y": c.roi, "customdata": c.dates,
            "line": {"width": 2.5, "color": colors[i % colors.len()]},
            "hovertemplate": "<b>%{fullData.name}</b><br>Jours: %{x}<br>ROI: %{y:.2f}x<br>Date: %{customdata}<extra></extra>"}));
    }
    Ok(fig.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ymd as d;

    #[test]
    fn roi_is_relative_to_cycle_top() {
        let btc = vec![
            PricePoint {
                date: d("2020-06-01"),
                close: 50.0,
            },
            PricePoint {
                date: d("2020-07-01"),
                close: 100.0,
            },
            PricePoint {
                date: d("2020-07-11"),
                close: 25.0,
            },
        ];
        let r = cycle_rois(&btc, &[d("2020-05-11"), d("2024-04-20")]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Cycle 2020");
        assert_eq!(r[0].days, vec![0, 10]);
        assert_eq!(r[0].roi, vec![1.0, 0.25]);
    }
}

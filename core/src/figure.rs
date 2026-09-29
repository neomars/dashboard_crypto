//! Figures Plotly sérialisées en JSON (`{data, layout}`), rendues côté
//! interface par Plotly.js. Le thème sombre (équivalent de `plotly_dark`) est
//! appliqué par l'interface.

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Figure {
    pub data: Vec<Value>,
    pub layout: Value,
}

impl Figure {
    pub fn new(layout: Value) -> Self {
        Self {
            data: Vec::new(),
            layout,
        }
    }

    pub fn trace(&mut self, trace: Value) -> &mut Self {
        self.data.push(trace);
        self
    }

    pub fn shape(&mut self, shape: Value) {
        push_to(&mut self.layout, "shapes", shape);
    }

    pub fn annotation(&mut self, annotation: Value) {
        push_to(&mut self.layout, "annotations", annotation);
    }

    /// Ligne verticale sur toute la hauteur de l'axe `yref` (`"paper"` ou domaine d'un axe).
    pub fn vline(&mut self, x: &str, color: &str, width: f64, dash: &str, yref: &str) {
        self.shape(json!({
            "type": "line", "x0": x, "x1": x, "y0": 0, "y1": 1,
            "xref": "x", "yref": yref, "line": {"color": color, "width": width, "dash": dash}
        }));
    }

    /// Ligne horizontale sur toute la largeur, sur l'axe `yref` (`"y"`, `"y2"`...).
    pub fn hline(&mut self, y: f64, color: &str, dash: &str, yref: &str, xref_domain: &str) {
        self.shape(json!({
            "type": "line", "x0": 0, "x1": 1, "y0": y, "y1": y,
            "xref": xref_domain, "yref": yref, "line": {"color": color, "width": 1, "dash": dash}
        }));
    }
}

fn push_to(layout: &mut Value, key: &str, item: Value) {
    let obj = layout.as_object_mut().expect("layout objet");
    obj.entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("tableau")
        .push(item);
}

/// Fusion profonde de `extra` dans `base` (les objets sont fusionnés, le reste remplacé).
pub fn merge(base: &mut Value, extra: Value) {
    match (base, extra) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (slot, v) => *slot = v,
    }
}

/// Mise en page de deux graphiques superposés partageant l'axe X (équivalent
/// de `make_subplots(rows=2, shared_xaxes=True, ...)`). Les traces du bas
/// utilisent `xaxis: "x2", yaxis: "y2"`.
pub fn two_rows(row_heights: [f64; 2], spacing: f64, titles: Option<[&str; 2]>) -> Value {
    let usable = 1.0 - spacing;
    let total = row_heights[0] + row_heights[1];
    let bottom_top = usable * row_heights[1] / total;
    let top_bottom = bottom_top + spacing;
    let mut layout = json!({
        "xaxis": {"anchor": "y", "domain": [0, 1], "matches": "x2", "showticklabels": false},
        "xaxis2": {"anchor": "y2", "domain": [0, 1]},
        "yaxis": {"anchor": "x", "domain": [top_bottom, 1]},
        "yaxis2": {"anchor": "x2", "domain": [0, bottom_top]},
    });
    if let Some([t1, t2]) = titles {
        let title = |text: &str, y: f64| {
            json!({
                "text": text, "x": 0.5, "y": y, "xref": "paper", "yref": "paper",
                "xanchor": "center", "yanchor": "bottom", "showarrow": false, "font": {"size": 16}
            })
        };
        layout["annotations"] = json!([title(t1, 1.0), title(t2, bottom_top)]);
    }
    layout
}

pub fn dates(dates: impl IntoIterator<Item = NaiveDate>) -> Vec<String> {
    dates.into_iter().map(crate::series::fmt_date).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_is_deep() {
        let mut a = json!({"yaxis": {"title": "a", "type": "log"}, "height": 1});
        merge(&mut a, json!({"yaxis": {"title": "b"}, "width": 2}));
        assert_eq!(
            a,
            json!({"yaxis": {"title": "b", "type": "log"}, "height": 1, "width": 2})
        );
    }

    #[test]
    fn two_rows_domains_follow_heights() {
        let l = two_rows([0.7, 0.3], 0.1, Some(["A", "B"]));
        let y1 = l["yaxis"]["domain"].as_array().unwrap();
        let y2 = l["yaxis2"]["domain"].as_array().unwrap();
        assert!((y2[1].as_f64().unwrap() - 0.27).abs() < 1e-9);
        assert!((y1[0].as_f64().unwrap() - 0.37).abs() < 1e-9);
        assert_eq!(l["annotations"][1]["text"], "B");
    }
}

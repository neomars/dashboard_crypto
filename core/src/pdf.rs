//! Rapport PDF du simulateur. Générateur PDF minimal (texte Helvetica +
//! cadres) : pas de dépendance lourde, et les accents sont conservés grâce à
//! l'encodage WinAnsi. Les caractères hors Latin-1 (émojis) sont retirés.

use crate::series::fmt_date;
use crate::simulator::{money, Simulation};
use std::io::Write as _;

const PAGE_W: f64 = 595.28; // A4 en points
const PAGE_H: f64 = 841.89;
const MARGIN: f64 = 28.35; // 10 mm
const MM: f64 = 2.8346;

/// Largeurs Helvetica (1/1000 em) des caractères ASCII 32 à 126.
const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn char_width(c: char) -> f64 {
    let w = match c as u32 {
        32..=126 => HELVETICA_WIDTHS[(c as u32 - 32) as usize],
        _ => 556,
    };
    w as f64 / 1000.0
}

/// Largeur d'un texte en points.
pub fn text_width(text: &str, size: f64, bold: bool) -> f64 {
    text.chars().map(char_width).sum::<f64>() * size * if bold { 1.06 } else { 1.0 }
}

/// Encode un texte en chaîne PDF WinAnsi (Latin-1), en échappant `( ) \` et
/// en retirant les caractères non représentables.
fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in text.chars() {
        let code = match c {
            '’' => 0x92,
            '€' => 0x80,
            '–' => 0x96,
            '…' => 0x85,
            c if (c as u32) < 0x100 && (c as u32 >= 0x20) => c as u32 as u8,
            _ => continue,
        };
        if matches!(code, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(code);
    }
    out.push(b')');
    out
}

/// Supprime les émojis et espaces superflus (texte affichable en Helvetica).
pub fn clean(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| (*c as u32) < 0x100 || matches!(c, '’' | '€' | '–' | '…'))
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tronque `text` (avec « … ») pour qu'il tienne dans `width` points.
fn fit(text: &str, width: f64, size: f64) -> String {
    if text_width(text, size, false) <= width {
        return text.to_string();
    }
    let budget = width - text_width("…", size, false);
    let mut out = String::new();
    for c in text.chars() {
        if text_width(&out, size, false) + char_width(c) * size > budget {
            break;
        }
        out.push(c);
    }
    out.trim_end().to_string() + "…"
}

#[derive(Default)]
pub struct Document {
    pages: Vec<Vec<u8>>,
}

impl Document {
    pub fn new_page(&mut self) {
        self.pages.push(Vec::new());
    }

    fn current(&mut self) -> &mut Vec<u8> {
        if self.pages.is_empty() {
            self.new_page();
        }
        self.pages.last_mut().unwrap()
    }

    /// Texte dont la ligne de base est à `y` points depuis le haut de la page.
    pub fn text(&mut self, x: f64, y: f64, size: f64, bold: bool, text: &str) {
        let font = if bold { "F2" } else { "F1" };
        let page = self.current();
        let _ = write!(page, "BT /{font} {size:.1} Tf {x:.2} {:.2} Td ", PAGE_H - y);
        page.extend(pdf_string(text));
        page.extend_from_slice(b" Tj ET\n");
    }

    /// Cadre dont le coin supérieur gauche est à (`x`, `y`) depuis le haut.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let page = self.current();
        let _ = writeln!(
            page,
            "0.5 w {x:.2} {:.2} {w:.2} {h:.2} re S",
            PAGE_H - y - h
        );
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = Vec::new();
        let n_pages = self.pages.len().max(1);
        // 1 catalogue, 2 arbre des pages, 3-4 polices, puis (page, contenu) par page.
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
        objects.push(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n_pages} >>",
                kids.join(" ")
            )
            .into_bytes(),
        );
        for base in ["Helvetica", "Helvetica-Bold"] {
            objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>").into_bytes());
        }
        let empty = Vec::new();
        for i in 0..n_pages {
            let content = self.pages.get(i).unwrap_or(&empty);
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                6 + 2 * i
            ).into_bytes());
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").into_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .into_bytes(),
        );
        out
    }
}

/// Rapport de simulation : paramètres, résultats et 20 dernières opérations.
pub fn simulation_report(sim: &Simulation) -> Vec<u8> {
    let s = sim.summary();
    let mut doc = Document::default();
    doc.new_page();
    let mut y = MARGIN + 20.0;

    let title = "Rapport de Simulation d'Investissement";
    doc.text(
        (PAGE_W - text_width(title, 16.0, true)) / 2.0,
        y,
        16.0,
        true,
        title,
    );
    y += 40.0;

    doc.text(MARGIN, y, 12.0, true, "Paramètres de Simulation");
    y += 22.0;
    for (k, v) in sim.params.display_rows() {
        doc.text(MARGIN, y, 10.0, false, &format!("{k}: {v}"));
        y += 20.0;
    }
    y += 14.0;

    doc.text(MARGIN, y, 12.0, true, "Résultats");
    y += 22.0;
    for line in [
        format!(
            "Capital Final: {} ({:+.2}%)",
            money(s.final_equity),
            s.performance_pct
        ),
        format!(
            "Buy & Hold: {} ({:+.2}%)",
            money(s.buy_hold),
            s.buy_hold_pct
        ),
        format!("Drawdown Max: {:.2}%", s.max_drawdown),
    ] {
        doc.text(MARGIN, y, 10.0, false, &line);
        y += 20.0;
    }
    if s.liquidated {
        doc.text(
            MARGIN,
            y,
            10.0,
            true,
            "ALERTE : la stratégie a été liquidée.",
        );
        y += 20.0;
    }
    y += 20.0;

    let widths = [30.0 * MM, 60.0 * MM, 100.0 * MM];
    let row = |doc: &mut Document, y: f64, h: f64, cells: [String; 3], size: f64, bold: bool| {
        let mut x = MARGIN;
        for (w, text) in widths.iter().zip(cells) {
            doc.rect(x, y, *w, h);
            doc.text(x + 3.0, y + h / 2.0 + size * 0.35, size, bold, &text);
            x += w;
        }
    };
    let header = || {
        [
            "Date".to_string(),
            "Action".to_string(),
            "Détails".to_string(),
        ]
    };
    row(&mut doc, y, 10.0 * MM, header(), 10.0, true);
    y += 10.0 * MM;

    let start = sim.trades.len().saturating_sub(20);
    for t in &sim.trades[start..] {
        if y + 8.0 * MM > PAGE_H - MARGIN {
            doc.new_page();
            y = MARGIN;
            row(&mut doc, y, 10.0 * MM, header(), 10.0, true);
            y += 10.0 * MM;
        }
        let cells = [
            fmt_date(t.date),
            fit(&clean(&t.action), widths[1] - 6.0, 8.0),
            fit(&clean(&t.details), widths[2] - 6.0, 8.0),
        ];
        row(&mut doc, y, 8.0 * MM, cells, 8.0, false);
        y += 8.0 * MM;
    }
    doc.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_strings_are_escaped_and_latin1() {
        assert_eq!(pdf_string("a(b)\\é"), b"(a\\(b\\)\\\\\xE9)".to_vec());
        assert_eq!(pdf_string("💀 ok"), b"( ok)".to_vec());
    }

    #[test]
    fn clean_drops_emojis() {
        assert_eq!(clean("⚠️ Vente reportée"), "Vente reportée");
        assert_eq!(clean("💀 LIQUIDATION 💀"), "LIQUIDATION");
    }

    #[test]
    fn document_structure_is_valid() {
        let mut doc = Document::default();
        doc.text(10.0, 10.0, 12.0, false, "Bonjour");
        doc.new_page();
        doc.rect(10.0, 10.0, 50.0, 20.0);
        let bytes = doc.to_bytes();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.trim_end().ends_with("%%EOF"));
        // L'offset annoncé par startxref pointe bien sur la table xref.
        let xref: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(bytes[xref..].starts_with(b"xref"));
    }

    #[test]
    fn fit_adds_ellipsis_only_when_needed() {
        assert_eq!(fit("court", 100.0, 8.0), "court");
        let long = "Vente rattrapée (1 étape(s)) : 337.5028 BTC. Dette payée: $6,851.20.";
        let cut = fit(long, 100.0, 8.0);
        assert!(cut.ends_with('…') && cut.len() < long.len());
        assert!(text_width(&cut, 8.0, false) <= 100.0);
    }

    #[test]
    fn helvetica_width() {
        assert!((text_width("AB", 10.0, false) - 13.34).abs() < 1e-9);
    }
}

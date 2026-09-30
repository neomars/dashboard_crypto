//! SEC EDGAR (<https://www.sec.gov/edgar>) : dernières publications de
//! Strategy Inc. (MSTR), pour compléter le fichier de holdings intégré.
//!
//! - **Actions en circulation** : API XBRL `companyconcept`, fait
//!   `dei:EntityCommonStockSharesOutstanding` (page de couverture des 10-Q et
//!   10-K). Un fait par classe d'actions (A et B) : les valeurs d'un même dépôt
//!   et d'une même date sont additionnées.
//! - **BTC détenus** : texte des 8-K (annonces d'achat), lu par motifs fixes :
//!   phrase « held an aggregate of approximately N bitcoins » ou colonne
//!   « Aggregate BTC Holdings » du tableau des achats.
//!
//! Gratuit et sans clé. La SEC exige un User-Agent identifiant l'application
//! (section `[SEC]`, clé `user_agent` de config.ini pour le personnaliser).

use chrono::NaiveDate;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Identifiant SEC (CIK) de Strategy Inc.
pub const MSTR_CIK: &str = "0001050446";

pub fn submissions_url() -> String {
    format!("https://data.sec.gov/submissions/CIK{MSTR_CIK}.json")
}

pub fn shares_concept_url() -> String {
    format!(
        "https://data.sec.gov/api/xbrl/companyconcept/CIK{MSTR_CIK}/dei/EntityCommonStockSharesOutstanding.json"
    )
}

/// Dépôt SEC (entrée de `filings.recent` du fichier `submissions`).
#[derive(Debug, Clone, PartialEq)]
pub struct Filing {
    pub accession: String,
    pub form: String,
    pub filing_date: NaiveDate,
    /// Date de l'événement ou de la période couverte (vide pour certains dépôts).
    pub report_date: Option<NaiveDate>,
    pub primary_document: String,
}

impl Filing {
    /// Date retenue pour les données du dépôt.
    pub fn date(&self) -> NaiveDate {
        self.report_date.unwrap_or(self.filing_date)
    }

    pub fn document_url(&self) -> String {
        format!(
            "https://www.sec.gov/Archives/edgar/data/{}/{}/{}",
            MSTR_CIK.trim_start_matches('0'),
            self.accession.replace('-', ""),
            self.primary_document
        )
    }
}

fn ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Dépôts récents (fichier `submissions`, colonnes parallèles de `filings.recent`).
pub fn parse_submissions(v: &Value) -> Result<Vec<Filing>, String> {
    let recent = &v["filings"]["recent"];
    let col = |name: &str| -> Result<&Vec<Value>, String> {
        recent[name]
            .as_array()
            .ok_or_else(|| format!("réponse SEC inattendue : colonne « {name} » absente"))
    };
    let (accn, form, filed, report, doc) = (
        col("accessionNumber")?,
        col("form")?,
        col("filingDate")?,
        col("reportDate")?,
        col("primaryDocument")?,
    );
    Ok((0..accn.len())
        .filter_map(|i| {
            Some(Filing {
                accession: accn[i].as_str()?.to_string(),
                form: form.get(i)?.as_str()?.to_string(),
                filing_date: ymd(filed.get(i)?.as_str()?)?,
                report_date: report.get(i).and_then(Value::as_str).and_then(ymd),
                primary_document: doc.get(i)?.as_str()?.to_string(),
            })
        })
        .collect())
}

/// Nombre d'actions publié à une date (toutes classes confondues).
#[derive(Debug, Clone, PartialEq)]
pub struct SharesFact {
    pub date: NaiveDate,
    pub shares: f64,
    pub form: String,
    pub filed: NaiveDate,
}

/// Division de l'action MSTR par 10 (7 août 2024) : les cours Yahoo sont
/// ajustés, les nombres d'actions antérieurs sont donc multipliés par 10.
pub const SPLIT_DATE: (i32, u32, u32) = (2024, 8, 7);
pub const SPLIT_RATIO: f64 = 10.0;

/// Actions en circulation par date, d'après la réponse `companyconcept`.
/// Les faits d'un même dépôt et d'une même date (une valeur par classe) sont
/// additionnés ; pour une même date publiée plusieurs fois, le dépôt le plus
/// récent l'emporte. Les valeurs antérieures à la division sont ×10.
pub fn parse_shares_concept(v: &Value) -> Result<Vec<SharesFact>, String> {
    let facts = v["units"]["shares"]
        .as_array()
        .ok_or("réponse SEC inattendue : aucune valeur en « shares »")?;
    let split = NaiveDate::from_ymd_opt(SPLIT_DATE.0, SPLIT_DATE.1, SPLIT_DATE.2).unwrap();

    // (date, dépôt) → somme des classes
    let mut per_filing: BTreeMap<(NaiveDate, String), SharesFact> = BTreeMap::new();
    for f in facts {
        let (Some(end), Some(val), Some(accn), Some(filed)) = (
            f["end"].as_str().and_then(ymd),
            f["val"].as_f64(),
            f["accn"].as_str(),
            f["filed"].as_str().and_then(ymd),
        ) else {
            continue;
        };
        let entry = per_filing
            .entry((end, accn.to_string()))
            .or_insert(SharesFact {
                date: end,
                shares: 0.0,
                form: f["form"].as_str().unwrap_or("").to_string(),
                filed,
            });
        entry.shares += val;
    }

    let mut by_date: BTreeMap<NaiveDate, SharesFact> = BTreeMap::new();
    for mut fact in per_filing.into_values() {
        if fact.date < split {
            fact.shares *= SPLIT_RATIO;
        }
        match by_date.get(&fact.date) {
            Some(existing) if existing.filed >= fact.filed => {}
            _ => {
                by_date.insert(fact.date, fact);
            }
        }
    }
    Ok(by_date.into_values().filter(|f| f.shares > 0.0).collect())
}

/// Texte brut d'une page HTML : balises retirées, entités courantes décodées,
/// espaces normalisés.
pub fn html_to_text(html: &str) -> String {
    static SCRIPTS: OnceLock<Regex> = OnceLock::new();
    static TAGS: OnceLock<Regex> = OnceLock::new();
    static ENTITIES: OnceLock<Regex> = OnceLock::new();
    let scripts = SCRIPTS.get_or_init(|| {
        Regex::new(r"(?is)<(script|style|ix:header)\b.*?</(script|style|ix:header)>").unwrap()
    });
    let tags = TAGS.get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap());
    let entities = ENTITIES.get_or_init(|| Regex::new(r"&(#x?[0-9a-fA-F]+|[a-zA-Z]+);").unwrap());

    let without_scripts = scripts.replace_all(html, " ");
    let text = tags.replace_all(&without_scripts, " ");
    let text = entities.replace_all(&text, |c: &regex::Captures| {
        let e = &c[1];
        let code = if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok()
        } else if let Some(dec) = e.strip_prefix('#') {
            dec.parse().ok()
        } else {
            match e {
                "nbsp" => Some(0xA0),
                "amp" => Some('&' as u32),
                "lt" => Some('<' as u32),
                "gt" => Some('>' as u32),
                "quot" => Some('"' as u32),
                "apos" => Some('\'' as u32),
                "rsquo" | "lsquo" => Some('\'' as u32),
                "ndash" | "mdash" => Some('-' as u32),
                _ => None,
            }
        };
        code.and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_else(|| c[0].to_string())
    });
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_count(s: &str) -> Option<f64> {
    s.replace(',', "").parse::<f64>().ok()
}

/// Bornes de vraisemblance d'un nombre de BTC détenus.
fn plausible(btc: f64) -> bool {
    (1_000.0..=21_000_000.0).contains(&btc)
}

/// BTC détenus annoncés dans le texte d'un 8-K, ou `None` si le document
/// n'en parle pas.
///
/// 1. Phrase : « held an aggregate of approximately 628,791 bitcoins ».
/// 2. Tableau des achats : colonne « Aggregate BTC Holdings ». Dans le texte,
///    la ligne de données suit les en-têtes : BTC acquis, deux montants en
///    dollars, BTC détenus, deux montants en dollars. Les BTC détenus sont le
///    plus grand des nombres sans « $ » parmi ces six valeurs.
pub fn extract_btc_holdings(text: &str) -> Option<f64> {
    static SENTENCE: OnceLock<Regex> = OnceLock::new();
    static TABLE: OnceLock<Regex> = OnceLock::new();
    let sentence = SENTENCE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:aggregate\s+of|held|holds)\s+(?:approximately\s+|about\s+)?(\d{1,3}(?:,\d{3})+|\d+)\s+(?:bitcoins?|btc)\b",
        )
        .unwrap()
    });
    let from_sentence = sentence
        .captures_iter(text)
        .filter_map(|c| parse_count(&c[1]))
        .filter(|v| plausible(*v))
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        });
    if from_sentence.is_some() {
        return from_sentence;
    }

    let table = TABLE.get_or_init(|| Regex::new(r"(?i)aggregate\s+btc\s+holdings").unwrap());
    table
        .find_iter(text)
        .filter_map(|m| {
            let mut values: Vec<(bool, f64)> = Vec::new(); // (en dollars, valeur)
            let mut prev_dollar = false;
            for token in text[m.end()..].split_whitespace() {
                if values.len() == 6 {
                    break;
                }
                let dollar = token.starts_with('$');
                let digits = token
                    .trim_start_matches('$')
                    .trim_end_matches([',', ';', ')']);
                let is_amount = !digits.is_empty()
                    && digits
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
                    && digits.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && (digits.contains(',') || digits.contains('.') || dollar || prev_dollar);
                if is_amount {
                    if let Some(v) = parse_count(digits) {
                        values.push((dollar || prev_dollar, v));
                    }
                }
                prev_dollar = token == "$";
            }
            values
                .iter()
                .filter(|(dollar, v)| !dollar && plausible(*v))
                .map(|(_, v)| *v)
                .fold(None, |acc: Option<f64>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                })
        })
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        ymd(s).unwrap()
    }

    #[test]
    fn submissions_are_parsed_column_by_column() {
        let v = json!({"filings": {"recent": {
            "accessionNumber": ["0001193125-26-361845", "0000950170-26-000001"],
            "form": ["8-K", "10-Q"],
            "filingDate": ["2026-08-24", "2026-07-29"],
            "reportDate": ["2026-08-24", ""],
            "primaryDocument": ["mstr-20260824.htm", "mstr-20260630.htm"]
        }}});
        let f = parse_submissions(&v).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].form, "8-K");
        assert_eq!(f[0].date(), d("2026-08-24"));
        assert_eq!(f[1].report_date, None);
        assert_eq!(f[1].date(), d("2026-07-29"));
        assert_eq!(
            f[0].document_url(),
            "https://www.sec.gov/Archives/edgar/data/1050446/000119312526361845/mstr-20260824.htm"
        );
        assert!(parse_submissions(&json!({})).is_err());
    }

    #[test]
    fn shares_sum_classes_and_adjust_for_split() {
        let v = json!({"units": {"shares": [
            // Avant la division : classes A et B du même dépôt, ×10.
            {"end": "2021-07-22", "val": 7783443, "accn": "a1", "form": "10-Q", "filed": "2021-07-29"},
            {"end": "2021-07-22", "val": 1964025, "accn": "a1", "form": "10-Q", "filed": "2021-07-29"},
            // Après : une seule valeur, puis la même date republiée plus tard (amendement).
            {"end": "2026-07-24", "val": 384000000, "accn": "b1", "form": "10-Q", "filed": "2026-07-30"},
            {"end": "2026-07-24", "val": 384225751, "accn": "b2", "form": "10-Q/A", "filed": "2026-08-15"},
            {"end": "bad", "val": 1, "accn": "c", "filed": "2026-01-01"}
        ]}});
        let f = parse_shares_concept(&v).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].date, d("2021-07-22"));
        assert_eq!(f[0].shares, 97_474_680.0);
        assert_eq!(f[1].shares, 384_225_751.0);
        assert_eq!(f[1].form, "10-Q/A");
        assert!(parse_shares_concept(&json!({"units": {}})).is_err());
    }

    #[test]
    fn html_is_reduced_to_text() {
        let html = "<html><head><style>p{}</style></head><body><p>Strategy&#160;held&nbsp;an <b>aggregate</b></p>\n<td>&#x24;&amp;</td></body></html>";
        assert_eq!(html_to_text(html), "Strategy held an aggregate $&");
    }

    #[test]
    fn btc_holdings_from_sentence() {
        let text = "During the period, the Company acquired 21,021 bitcoins for approximately $2.46 billion. \
                    As of August 3, 2025, the Company held an aggregate of approximately 628,791 bitcoins, \
                    acquired at an aggregate purchase price of approximately $46.08 billion.";
        assert_eq!(extract_btc_holdings(text), Some(628_791.0));
        assert_eq!(
            extract_btc_holdings("the Company holds 214,246 BTC"),
            Some(214_246.0)
        );
    }

    #[test]
    fn btc_holdings_from_purchase_table() {
        let html = "<p>BTC Updates</p><table><tr><td>Period</td><td>BTC Acquired</td>\
            <td>Aggregate Purchase Price (in millions)</td><td>Average Purchase Price</td>\
            <td>Aggregate BTC Holdings</td><td>Aggregate Purchase Price (in billions)</td>\
            <td>Average Purchase Price</td></tr><tr><td>August 17, 2026 - August 23, 2026</td>\
            <td>3,081</td><td>$</td><td>356.9</td><td>$</td><td>115,829</td><td>840,447</td>\
            <td>$</td><td>63.36</td><td>$75,385</td></tr></table><p>ATM: 12,345,678 shares sold</p>";
        assert_eq!(extract_btc_holdings(&html_to_text(html)), Some(840_447.0));
        // Achat de moins de 1 000 BTC : nombre sans séparateur.
        let small = "Aggregate BTC Holdings Aggregate Purchase Price Average Purchase Price \
                     September 21, 2026 - September 27, 2026 487 $ 55.1 $ 113,140 847,666 $ 64.2 $ 75,700";
        assert_eq!(extract_btc_holdings(small), Some(847_666.0));
    }

    #[test]
    fn documents_without_holdings_give_none() {
        assert_eq!(
            extract_btc_holdings("Item 5.07 Submission of Matters to a Vote of Security Holders."),
            None
        );
        // Montant en dollars seul : pas de BTC détenus.
        assert_eq!(
            extract_btc_holdings("Aggregate BTC Holdings $ 1,000,000 $ 2,000"),
            None
        );
    }
}

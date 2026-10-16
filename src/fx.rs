use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RATES_URL: &str = "https://api.frankfurter.dev/v1/latest?base=EUR";
pub const MAX_AGE_MS: f64 = 6.0 * 3600.0 * 1000.0;

/// ECB reference rates, units of each currency per euro.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rates {
    pub date: String,
    pub fetched: f64,
    pub per_eur: BTreeMap<String, f64>,
}

impl Rates {
    pub fn parse(v: &Value, fetched: f64) -> Option<Self> {
        let per_eur: BTreeMap<String, f64> = v["rates"]
            .as_object()?
            .iter()
            .filter_map(|(k, r)| Some((k.clone(), r.as_f64().filter(|r| *r > 0.0)?)))
            .collect();
        (!per_eur.is_empty()).then(|| Self { date: v["date"].as_str().unwrap_or_default().to_string(), fetched, per_eur })
    }

    /// Units of `iso` per euro.
    pub fn rate(&self, iso: &str) -> Option<f64> {
        if iso == "EUR" { Some(1.0) } else { self.per_eur.get(iso).copied() }
    }

    /// Converts a euro amount, adding the card markup for foreign currencies.
    pub fn convert_eur(&self, iso: &str, markup_pct: f64, eur: f64) -> Option<f64> {
        if iso == "EUR" {
            return Some(eur);
        }
        Some(eur * self.rate(iso)? * (1.0 + markup_pct / 100.0))
    }
}

/// ISO 4217 code for a currency symbol or code.
pub fn iso_code(currency: &str) -> Option<String> {
    let c = currency.trim();
    let known = match c {
        "€" => "EUR",
        "¥" | "元" | "RMB" => "CNY",
        "$" | "US$" => "USD",
        "£" => "GBP",
        "Kč" | "Kc" => "CZK",
        "zł" | "zl" => "PLN",
        "Ft" => "HUF",
        "lei" => "RON",
        "Fr." | "SFr." => "CHF",
        "₺" => "TRY",
        "₹" => "INR",
        "₩" => "KRW",
        _ => "",
    };
    if !known.is_empty() {
        return Some(known.into());
    }
    (c.len() == 3 && c.chars().all(|ch| ch.is_ascii_alphabetic())).then(|| c.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conversion() {
        let r = Rates::parse(&json!({"date": "2026-10-15", "rates": {"CZK": 24.353, "PLN": 4.37}}), 1.0).unwrap();
        assert_eq!(r.convert_eur("EUR", 2.0, 1.5), Some(1.5));
        assert!((r.convert_eur("CZK", 0.0, 2.0).unwrap() - 48.706).abs() < 1e-9);
        assert!((r.convert_eur("CZK", 1.0, 1.0).unwrap() - 24.353 * 1.01).abs() < 1e-9);
        assert_eq!(r.convert_eur("XYZ", 0.0, 1.0), None);
    }

    #[test]
    fn codes() {
        assert_eq!(iso_code("€").as_deref(), Some("EUR"));
        assert_eq!(iso_code("Kč").as_deref(), Some("CZK"));
        assert_eq!(iso_code("chf").as_deref(), Some("CHF"));
        assert_eq!(iso_code("kr"), None);
    }
}

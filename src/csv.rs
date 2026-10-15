use std::collections::HashMap;

use chrono::{NaiveDate, NaiveTime};

use crate::format::parse_num;
use crate::model::Record;
use crate::stats::Entry;

pub const HEADERS: [&str; 20] = [
    "id",
    "date",
    "vehicle",
    "odometer_km",
    "trip_km",
    "liters",
    "price_per_liter",
    "total",
    "paid",
    "full",
    "missed_previous",
    "fuel",
    "station",
    "address",
    "lat",
    "lng",
    "display_l_100km",
    "calc_l_100km",
    "payment",
    "notes",
];

fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r', ';']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn n(v: Option<f64>) -> String {
    v.map(|v| {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    })
    .unwrap_or_default()
}

/// Builds a CSV export (UTF-8 with BOM for spreadsheet apps).
pub fn export(entries: &[(String, &Entry)]) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(&HEADERS.join(","));
    out.push_str("\r\n");
    for (vehicle, e) in entries {
        let r = &e.rec;
        let row = [
            r.id.clone(),
            r.date.clone(),
            vehicle.clone(),
            n(r.odometer),
            n(e.trip),
            n(r.liters),
            n(r.price),
            n(r.total),
            n(r.paid),
            r.full.to_string(),
            r.missed_previous.to_string(),
            r.fuel.clone(),
            r.station.clone(),
            r.address.clone(),
            n(r.lat),
            n(r.lng),
            n(r.display_cons),
            n(e.seg.as_ref().map(|s| (s.cons * 100.0).round() / 100.0)),
            r.payment.clone(),
            r.notes.clone(),
        ];
        out.push_str(&row.iter().map(|s| field(s)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// Parses CSV text; the delimiter (`,` `;` or tab) is taken from the header line.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let header = text.lines().next().unwrap_or("");
    let delim = [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| header.matches(*d).count())
        .unwrap_or(',');

    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => cur.push(c),
            }
        } else if c == '"' {
            quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut cur));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut cur));
            if row.iter().any(|f| !f.trim().is_empty()) {
                rows.push(std::mem::take(&mut row));
            }
            row.clear();
        } else {
            cur.push(c);
        }
    }
    row.push(cur);
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row);
    }
    rows
}

/// Accepts ISO, "YYYY/MM/DD" and "DD.MM.YYYY" dates, with optional "HH:MM".
fn parse_date(s: &str) -> Option<String> {
    let s = s.trim().replace('/', "-");
    let iso = crate::stats::parse_dt(&s.replacen(' ', "T", 1));
    let de = || {
        let (d, t) = s.split_once([' ', ',']).map_or((s.as_str(), ""), |(d, t)| (d, t.trim()));
        let date = NaiveDate::parse_from_str(d, "%d.%m.%Y").ok()?;
        let time = NaiveTime::parse_from_str(t, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(12, 0, 0)?);
        Some(date.and_time(time))
    };
    iso.or_else(de).map(|d| d.format("%Y-%m-%dT%H:%M").to_string())
}

fn truthy(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "ja" | "是" | "x" => Some(true),
        "0" | "false" | "no" | "n" | "nein" | "否" => Some(false),
        _ => None,
    }
}

/// Record parsed from a CSV row, together with its vehicle name.
pub struct Imported {
    pub vehicle: String,
    pub rec: Record,
}

/// Converts CSV rows (with header) into records; returns the records and the number of skipped rows.
pub fn to_records(rows: &[Vec<String>], new_id: &mut dyn FnMut() -> String) -> (Vec<Imported>, usize) {
    let Some((header, body)) = rows.split_first() else { return (Vec::new(), 0) };
    let idx: HashMap<String, usize> = header
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_lowercase(), i))
        .collect();
    let get = |row: &[String], key: &str| -> String {
        idx.get(key).and_then(|i| row.get(*i)).map(|s| s.trim().to_string()).unwrap_or_default()
    };
    let num = |row: &[String], key: &str| parse_num(&get(row, key));

    let mut out = Vec::new();
    let mut skipped = 0;
    for row in body {
        let date = parse_date(&get(row, "date"));
        let odometer = num(row, "odometer_km");
        let liters = num(row, "liters");
        let (Some(date), Some(_), Some(_)) = (date, odometer, liters) else {
            skipped += 1;
            continue;
        };
        let id = get(row, "id");
        out.push(Imported {
            vehicle: get(row, "vehicle"),
            rec: Record {
                id: if id.is_empty() { new_id() } else { id },
                date,
                odometer,
                liters,
                price: num(row, "price_per_liter"),
                total: num(row, "total"),
                paid: num(row, "paid"),
                full: truthy(&get(row, "full")).unwrap_or(true),
                missed_previous: truthy(&get(row, "missed_previous")).unwrap_or(false),
                fuel: get(row, "fuel"),
                station: get(row, "station"),
                address: get(row, "address"),
                lat: num(row, "lat"),
                lng: num(row, "lng"),
                display_cons: num(row, "display_l_100km"),
                payment: get(row, "payment"),
                notes: get(row, "notes"),
                ..Default::default()
            },
        });
    }
    (out, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::analyze;

    #[test]
    fn parse_quotes_and_delimiters() {
        let rows = parse("a;b;c\r\n1;\"x;y\";\"he said \"\"hi\"\"\"\n\n2;;\"multi\nline\"");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], ["1", "x;y", "he said \"hi\""]);
        assert_eq!(rows[2], ["2", "", "multi\nline"]);
    }

    #[test]
    fn export_import_roundtrip() {
        let recs = vec![
            Record {
                id: "a".into(),
                date: "2026-01-01T08:15".into(),
                odometer: Some(1000.0),
                liters: Some(40.5),
                price: Some(1.759),
                total: Some(71.24),
                station: "Shell, Main St".into(),
                notes: "quote \" here".into(),
                ..Default::default()
            },
            Record {
                id: "b".into(),
                date: "2026-01-20T18:00".into(),
                odometer: Some(1600.0),
                liters: Some(36.0),
                total: Some(63.0),
                full: false,
                ..Default::default()
            },
        ];
        let entries = analyze(&recs);
        let csv = export(&entries.iter().map(|e| ("Car".to_string(), e)).collect::<Vec<_>>());
        let (back, skipped) = to_records(&parse(&csv), &mut || unreachable!());
        assert_eq!(skipped, 0);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].vehicle, "Car");
        assert_eq!(back[0].rec.station, "Shell, Main St");
        assert_eq!(back[0].rec.notes, "quote \" here");
        assert_eq!(back[0].rec.price, Some(1.759));
        assert!(!back[1].rec.full);
    }

    #[test]
    fn import_minimal_rows() {
        let mut n = 0;
        let rows = parse(
            "date;odometer_km;liters;total\n2026/02/01;1000;40;70\n14.02.2026 07:45;1500;30,5;54\nbad;;;\n",
        );
        let (recs, skipped) = to_records(&rows, &mut || {
            n += 1;
            format!("gen{n}")
        });
        assert_eq!(recs.len(), 2);
        assert_eq!(skipped, 1);
        assert_eq!(recs[0].rec.date, "2026-02-01T12:00");
        assert_eq!(recs[1].rec.date, "2026-02-14T07:45");
        assert_eq!(recs[1].rec.liters, Some(30.5));
        assert_eq!(recs[0].rec.id, "gen1");
        assert!(recs[0].rec.full);
    }
}

use chrono::{Datelike, Timelike};

use crate::i18n::Lang;
use crate::stats::parse_dt;

pub const DASH: &str = "–";

/// Formats a number with the language's separators.
pub fn num(lang: Lang, n: f64, dec: usize) -> String {
    if !n.is_finite() {
        return DASH.into();
    }
    let s = format!("{:.*}", dec, n.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let (group, point) = match lang {
        Lang::De => ('.', ','),
        Lang::En | Lang::Zh => (',', '.'),
    };
    let mut out = String::new();
    if n < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        out.push('-');
    }
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(group);
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push(point);
        out.push_str(frac);
    }
    out
}

pub fn opt(lang: Lang, n: Option<f64>, dec: usize) -> String {
    n.map_or_else(|| DASH.into(), |n| num(lang, n, dec))
}

pub fn money(lang: Lang, cur: &str, n: f64, dec: usize) -> String {
    let v = num(lang, n, dec);
    let word = cur.chars().count() > 1 && cur.chars().all(char::is_alphabetic);
    match lang {
        Lang::De => format!("{v} {cur}"),
        _ if word => format!("{cur} {v}"),
        _ => format!("{cur}{v}"),
    }
}

pub fn money_opt(lang: Lang, cur: &str, n: Option<f64>, dec: usize) -> String {
    n.map_or_else(|| DASH.into(), |n| money(lang, cur, n, dec))
}

/// Formats a number for an input field (no grouping).
pub fn input_num(lang: Lang, n: f64, dec: usize) -> String {
    let s = format!("{:.*}", dec, n);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    match lang {
        Lang::De => s.replace('.', ","),
        _ => s,
    }
}

/// Parses user input, accepting both "1.234,5" and "1,234.5".
pub fn parse_num(s: &str) -> Option<f64> {
    let s: String = s.chars().filter(|c| !c.is_whitespace() && *c != '\'').collect();
    if s.is_empty() {
        return None;
    }
    let normalized = match (s.rfind('.'), s.rfind(',')) {
        (Some(d), Some(c)) if c > d => s.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => s.replace(',', ""),
        (None, Some(_)) => s.replace(',', "."),
        _ => s,
    };
    normalized.parse::<f64>().ok().filter(|n| n.is_finite())
}

pub fn date(lang: Lang, s: &str, with_time: bool) -> String {
    let Some(d) = parse_dt(s) else { return s.to_string() };
    let t = lang.t();
    let wd = t.weekdays[d.weekday().num_days_from_sunday() as usize];
    let mon = t.months[d.month0() as usize];
    let time = if with_time { format!(" {:02}:{:02}", d.hour(), d.minute()) } else { String::new() };
    match lang {
        Lang::Zh => format!("{}年{}月{}日 {wd}{time}", d.year(), d.month(), d.day()),
        Lang::En => format!("{wd}, {mon} {}, {}{time}", d.day(), d.year()),
        Lang::De => format!("{wd}, {}. {mon} {}{time}", d.day(), d.year()),
    }
}

pub fn short_date(lang: Lang, s: &str) -> String {
    let Some(d) = parse_dt(s) else { return s.to_string() };
    match lang {
        Lang::Zh => format!("{}/{}/{}", d.year() % 100, d.month(), d.day()),
        Lang::En => format!("{} {}, {}", lang.t().months[d.month0() as usize], d.day(), d.year()),
        Lang::De => format!("{:02}.{:02}.{}", d.day(), d.month(), d.year()),
    }
}

/// Compact date for chart axes.
pub fn axis_date(lang: Lang, s: &str) -> String {
    let Some(d) = parse_dt(s) else { return s.to_string() };
    let y = d.year() % 100;
    match lang {
        Lang::Zh => format!("{y}/{}/{}", d.month(), d.day()),
        Lang::En => format!("{}/{}/{y:02}", d.month(), d.day()),
        Lang::De => format!("{}.{}.{y:02}", d.day(), d.month()),
    }
}

/// "YYYY-MM" as a month heading.
pub fn month(lang: Lang, key: &str) -> String {
    let Some(d) = parse_dt(&format!("{key}-01")) else { return key.to_string() };
    match lang {
        Lang::Zh => format!("{}年{}月", d.year(), d.month()),
        _ => format!("{} {}", lang.t().months[d.month0() as usize], d.year()),
    }
}

/// "YYYY-MM" as a compact axis label.
pub fn month_axis(lang: Lang, key: &str) -> String {
    let Some(d) = parse_dt(&format!("{key}-01")) else { return key.to_string() };
    let m = match lang {
        Lang::Zh => format!("{}月", d.month()),
        _ => lang.t().months[d.month0() as usize].trim_end_matches('.').to_string(),
    };
    if d.month() == 1 { format!("{m} '{:02}", d.year() % 100) } else { m }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(num(Lang::En, 1234567.891, 2), "1,234,567.89");
        assert_eq!(num(Lang::De, 1234.5, 2), "1.234,50");
        assert_eq!(num(Lang::Zh, -12.0, 1), "-12.0");
        assert_eq!(num(Lang::En, -0.001, 1), "0.0");
        assert_eq!(num(Lang::En, 999.0, 0), "999");
        assert_eq!(money(Lang::De, "€", 65.4, 2), "65,40 €");
        assert_eq!(money(Lang::En, "€", 65.4, 2), "€65.40");
        assert_eq!(money(Lang::En, "CHF", 5.0, 2), "CHF 5.00");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_num("1,759"), Some(1.759));
        assert_eq!(parse_num(" 1.234,56 "), Some(1234.56));
        assert_eq!(parse_num("1,234.56"), Some(1234.56));
        assert_eq!(parse_num("52 340"), Some(52340.0));
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("abc"), None);
        assert_eq!(input_num(Lang::De, 37.2, 2), "37,2");
        assert_eq!(input_num(Lang::En, 40.0, 2), "40");
    }

    #[test]
    fn dates() {
        assert_eq!(date(Lang::Zh, "2026-10-14T18:30", true), "2026年10月14日 周三 18:30");
        assert_eq!(date(Lang::En, "2026-10-14T18:30", false), "Wed, Oct 14, 2026");
        assert_eq!(date(Lang::De, "2026-10-14T18:30", true), "Mi., 14. Okt. 2026 18:30");
        assert_eq!(month(Lang::De, "2026-03"), "März 2026");
        assert_eq!(month_axis(Lang::En, "2026-01"), "Jan '26");
        assert_eq!(axis_date(Lang::De, "2026-03-05T10:00"), "5.3.26");
        assert_eq!(axis_date(Lang::En, "2026-03-05T10:00"), "3/5/26");
    }
}

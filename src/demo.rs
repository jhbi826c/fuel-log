use chrono::{Datelike, Duration, NaiveDateTime};

use crate::model::Record;

/// Small deterministic PRNG.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

const STATIONS: [(&str, &str, f64, f64); 4] = [
    ("Aral", "Königsbrücker Str. 96", 51.0806, 13.7642),
    ("Shell", "Bautzner Landstr. 30", 51.0631, 13.8013),
    ("JET", "Hamburger Str. 52", 51.0611, 13.6912),
    ("TotalEnergies", "Wiener Str. 80", 51.0352, 13.7562),
];

fn base_price(currency: &str) -> f64 {
    match currency {
        "¥" | "CNY" | "RMB" => 7.9,
        "$" | "USD" => 1.0,
        "£" | "GBP" => 1.45,
        "CHF" => 1.85,
        _ => 1.75,
    }
}

/// About 18 months of simulated fill-ups up to `now`.
pub fn generate(vehicle_id: &str, now: NaiveDateTime, currency: &str) -> Vec<Record> {
    let mut rng = Rng(42);
    let base = base_price(currency);
    let tank = 50.0;
    let mut level = tank;
    let mut odo = 42_180.0;
    let mut date = now - Duration::days(540);
    let mut out = Vec::new();

    for i in 0.. {
        let days = rng.range(6.0, 16.0).round() as i64;
        date += Duration::days(days) + Duration::minutes(rng.range(-180.0, 180.0) as i64);
        if date > now {
            break;
        }
        let winter = matches!(date.month(), 11 | 12 | 1 | 2);
        let cons = rng.range(5.6, 6.6) + if winter { 0.7 } else { 0.0 };
        let max_trip = (level - 6.0) / cons * 100.0;
        let trip = rng.range(300.0, 650.0).min(max_trip).round();
        odo += trip;
        level -= trip * cons / 100.0;

        let full = i == 0 || rng.next() > 0.12;
        let liters = if full { tank - level } else { rng.range(15.0, 25.0).min(tank - level) };
        let liters = (liters * 100.0).round() / 100.0;
        level = if full { tank } else { level + liters };

        let season = ((date.ordinal() as f64) / 365.0 * std::f64::consts::TAU).sin();
        let price = (base * (1.0 + 0.05 * season + rng.range(-0.03, 0.03)) * 1000.0).round() / 1000.0;
        let total = (liters * price * 100.0).round() / 100.0;
        let paid = (rng.next() < 0.2).then(|| ((total - base * 1.2) * 100.0).round() / 100.0);
        let (station, address, lat, lng) = STATIONS[(rng.next() * rng.next() * 4.0) as usize];

        out.push(Record {
            id: format!("demo-{i}"),
            vehicle_id: vehicle_id.into(),
            date: date.format("%Y-%m-%dT%H:%M").to_string(),
            odometer: Some(odo),
            liters: Some(liters),
            price: Some(price),
            total: Some(total),
            paid,
            full,
            fuel: if rng.next() < 0.85 { "Super E10" } else { "Super E5" }.into(),
            station: station.into(),
            address: address.into(),
            lat: Some(lat),
            lng: Some(lng),
            display_cons: full.then(|| ((cons - rng.range(0.1, 0.4)) * 10.0).round() / 10.0),
            demo: true,
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{Range, analyze, compute};

    #[test]
    fn demo_data_is_plausible() {
        let now = crate::stats::parse_dt("2026-10-14T12:00").unwrap();
        let recs = generate("v", now, "€");
        assert!(recs.len() > 30);
        assert!(recs.iter().all(|r| r.liters.unwrap() > 0.0 && r.date.as_str() <= "2026-10-14T12:00"));
        let s = compute(&analyze(&recs), &Range::default());
        let cons = s.avg_cons.unwrap();
        assert!((5.5..7.5).contains(&cons), "{cons}");
        assert!(s.savings > 0.0);
    }
}

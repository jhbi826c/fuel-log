use std::cmp::Ordering;
use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, NaiveDateTime};

use crate::model::Record;

/// How a fill-up relates to the full-to-full consumption calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegState {
    /// Full tank without a previous full tank: starting point for the next segment.
    Start,
    /// Partial fill-up without a starting point.
    NoStart,
    /// Partial fill-up, counted into the next full tank.
    Partial,
    /// Full tank closing a segment.
    Ok,
    /// Full tank, but the distance is unknown or did not increase.
    Invalid,
}

/// Distance and fuel between two full tanks.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub dist: f64,
    pub liters: f64,
    pub cost: f64,
    pub cons: f64,
    pub fills: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub rec: Record,
    pub cost: f64,
    pub unit_price: Option<f64>,
    /// Distance since the previous fill-up.
    pub trip: Option<f64>,
    pub seg: Option<Segment>,
    pub state: SegState,
}

pub fn sort_records(recs: &mut [Record]) {
    recs.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.odometer.partial_cmp(&b.odometer).unwrap_or(Ordering::Equal))
    });
}

/// Sorts records chronologically and computes per-fill-up figures.
pub fn analyze(records: &[Record]) -> Vec<Entry> {
    let mut recs: Vec<Record> = records.iter().filter(|r| !r.deleted).cloned().collect();
    sort_records(&mut recs);

    let mut out: Vec<Entry> = Vec::with_capacity(recs.len());
    // Odometer at the last full tank (None when only trip distances are known).
    let mut anchor: Option<Option<f64>> = None;
    let (mut acc_l, mut acc_c, mut acc_d, mut acc_known, mut fills) = (0.0, 0.0, 0.0, true, 0usize);

    for r in recs {
        let cost = r.cost();
        let liters = r.liters.unwrap_or(0.0);
        let unit_price = r.liters.filter(|l| *l > 0.0).map(|l| cost / l);
        let odo_trip = match (out.last().and_then(|p| p.rec.odometer), r.odometer) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        };
        let trip = r.trip.or(odo_trip);
        if r.missed_previous {
            anchor = None;
        }

        let mut seg = None;
        let state = match anchor.take() {
            Some(from_odo) => {
                acc_l += liters;
                acc_c += cost;
                fills += 1;
                match trip {
                    Some(t) => acc_d += t,
                    None => acc_known = false,
                }
                if r.full {
                    let dist = match (from_odo, r.odometer) {
                        (Some(a), Some(b)) => Some(b - a),
                        _ => acc_known.then_some(acc_d),
                    };
                    let state = match dist {
                        Some(dist) if dist > 0.0 && acc_l > 0.0 => {
                            seg = Some(Segment { dist, liters: acc_l, cost: acc_c, cons: acc_l / dist * 100.0, fills });
                            SegState::Ok
                        }
                        _ => SegState::Invalid,
                    };
                    anchor = Some(r.odometer);
                    (acc_l, acc_c, acc_d, acc_known, fills) = (0.0, 0.0, 0.0, true, 0);
                    state
                } else {
                    anchor = Some(from_odo);
                    SegState::Partial
                }
            }
            None => {
                (acc_l, acc_c, acc_d, acc_known, fills) = (0.0, 0.0, 0.0, true, 0);
                if r.full {
                    anchor = Some(r.odometer);
                    // A full tank with a known trip measures the distance since the previous (unrecorded) full tank.
                    match r.trip.filter(|t| *t > 0.0) {
                        Some(dist) if liters > 0.0 => {
                            seg = Some(Segment { dist, liters, cost, cons: liters / dist * 100.0, fills: 1 });
                            SegState::Ok
                        }
                        _ => SegState::Start,
                    }
                } else {
                    SegState::NoStart
                }
            }
        };

        out.push(Entry { rec: r, cost, unit_price, trip, seg, state });
    }
    out
}

/// Date range; `from` inclusive, `to` exclusive, both "YYYY-MM-DD".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Range {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl Range {
    pub fn contains(&self, date: &str) -> bool {
        self.from.as_deref().is_none_or(|f| date >= f) && self.to.as_deref().is_none_or(|t| date < t)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PricePoint {
    pub value: f64,
    pub date: String,
    pub station: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// Days since the Unix epoch.
    pub x: f64,
    pub y: f64,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonthAgg {
    pub key: String,
    pub cost: f64,
    pub liters: f64,
    pub count: usize,
    pub dist: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct YearAgg {
    pub year: i32,
    pub cost: f64,
    pub liters: f64,
    pub count: usize,
    pub dist: f64,
    pub cons: Option<f64>,
    pub cost_per_100km: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupAgg {
    pub name: String,
    pub count: usize,
    pub liters: f64,
    pub cost: f64,
    pub avg_price: Option<f64>,
    pub cons: Option<f64>,
    pub last_date: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub total_cost: f64,
    pub total_liters: f64,
    pub savings: f64,
    pub distance: f64,
    pub avg_price: Option<f64>,
    pub min_price: Option<PricePoint>,
    pub max_price: Option<PricePoint>,
    pub avg_cost_per_fill: Option<f64>,
    pub avg_liters_per_fill: Option<f64>,
    pub avg_cons: Option<f64>,
    pub cost_per_100km: Option<f64>,
    pub seg_count: usize,
    pub seg_dist: f64,
    pub best_cons: Option<f64>,
    pub worst_cons: Option<f64>,
    pub last_cons: Option<f64>,
    pub cost_per_month: Option<f64>,
    pub km_per_month: Option<f64>,
    pub avg_interval_days: Option<f64>,
    pub avg_interval_km: Option<f64>,
    pub monthly: Vec<MonthAgg>,
    pub yearly: Vec<YearAgg>,
    pub stations: Vec<GroupAgg>,
    pub fuels: Vec<GroupAgg>,
    pub cons_series: Vec<Point>,
    pub price_series: Vec<Point>,
}

pub const DAYS_PER_MONTH: f64 = 30.436_875;

/// Parses "YYYY-MM-DDTHH:MM" or "YYYY-MM-DD" (noon).
pub fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    let full = s.get(..16).and_then(|p| NaiveDateTime::parse_from_str(p, "%Y-%m-%dT%H:%M").ok());
    full.or_else(|| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?.and_hms_opt(12, 0, 0))
}

pub fn day_number(s: &str) -> Option<f64> {
    Some(parse_dt(s)?.and_utc().timestamp() as f64 / 86400.0)
}

fn days_between(a: &str, b: &str) -> f64 {
    match (day_number(a), day_number(b)) {
        (Some(a), Some(b)) => b - a,
        _ => 0.0,
    }
}

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den > 0.0).then(|| num / den)
}

pub fn compute(all: &[Entry], range: &Range) -> Stats {
    let recs: Vec<&Entry> = all.iter().filter(|e| range.contains(&e.rec.date)).collect();
    let mut s = Stats { count: recs.len(), ..Default::default() };
    let (Some(first), Some(last)) = (recs.first(), recs.last()) else {
        return s;
    };
    let n = recs.len() as f64;

    s.total_cost = recs.iter().map(|e| e.cost).sum();
    s.total_liters = recs.iter().filter_map(|e| e.rec.liters).sum();
    s.savings = recs
        .iter()
        .filter_map(|e| Some((e.rec.total? - e.rec.paid?).max(0.0)))
        .sum();
    s.distance = recs.iter().filter_map(|e| e.trip).filter(|t| *t > 0.0).sum();
    s.avg_price = ratio(s.total_cost, s.total_liters);
    s.avg_cost_per_fill = Some(s.total_cost / n);
    s.avg_liters_per_fill = Some(s.total_liters / n);

    let price_point = |e: &&Entry| {
        e.unit_price.map(|value| PricePoint { value, date: e.rec.date.clone(), station: e.rec.station.clone() })
    };
    s.min_price = recs
        .iter()
        .filter_map(price_point)
        .min_by(|a, b| a.value.total_cmp(&b.value));
    s.max_price = recs
        .iter()
        .filter_map(price_point)
        .max_by(|a, b| a.value.total_cmp(&b.value));

    let segs: Vec<(&Entry, &Segment)> = recs.iter().filter_map(|e| Some((*e, e.seg.as_ref()?))).collect();
    let seg_liters: f64 = segs.iter().map(|(_, g)| g.liters).sum();
    let seg_cost: f64 = segs.iter().map(|(_, g)| g.cost).sum();
    s.seg_count = segs.len();
    s.seg_dist = segs.iter().map(|(_, g)| g.dist).sum();
    s.avg_cons = ratio(seg_liters * 100.0, s.seg_dist);
    s.cost_per_100km = ratio(seg_cost * 100.0, s.seg_dist);
    s.best_cons = segs.iter().map(|(_, g)| g.cons).min_by(f64::total_cmp);
    s.worst_cons = segs.iter().map(|(_, g)| g.cons).max_by(f64::total_cmp);
    s.last_cons = segs.last().map(|(_, g)| g.cons);

    let days = days_between(&first.rec.date, &last.rec.date);
    if recs.len() >= 2 && days >= 1.0 {
        s.cost_per_month = Some((s.total_cost - first.cost) / days * DAYS_PER_MONTH);
        s.avg_interval_days = Some(days / (n - 1.0));
        if let (Some(a), Some(b)) = (first.rec.odometer, last.rec.odometer)
            && b > a
        {
            s.km_per_month = Some((b - a) / days * DAYS_PER_MONTH);
            s.avg_interval_km = Some((b - a) / (n - 1.0));
        }
    }

    s.monthly = monthly(&recs);
    s.yearly = yearly(&recs);
    s.stations = groups(&recs, |r| r.station.trim().to_string());
    s.fuels = groups(&recs, |r| r.fuel.trim().to_string());

    s.cons_series = segs
        .iter()
        .filter_map(|(e, g)| Some(Point { x: day_number(&e.rec.date)?, y: g.cons, date: e.rec.date.clone() }))
        .collect();
    s.price_series = recs
        .iter()
        .filter_map(|e| Some(Point { x: day_number(&e.rec.date)?, y: e.unit_price?, date: e.rec.date.clone() }))
        .collect();
    s
}

fn month_key(date: &str) -> Option<(i32, u32)> {
    let d = parse_dt(date)?;
    Some((d.year(), d.month()))
}

/// Per-month totals, including empty months in between.
fn monthly(recs: &[&Entry]) -> Vec<MonthAgg> {
    let mut map: BTreeMap<(i32, u32), MonthAgg> = BTreeMap::new();
    for e in recs {
        let Some(k) = month_key(&e.rec.date) else { continue };
        let m = map.entry(k).or_default();
        m.cost += e.cost;
        m.liters += e.rec.liters.unwrap_or(0.0);
        m.count += 1;
        m.dist += e.trip.filter(|t| *t > 0.0).unwrap_or(0.0);
    }
    let (Some(&(mut y, mut m)), Some(&last)) = (map.keys().next(), map.keys().next_back()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while (y, m) <= last {
        let mut agg = map.remove(&(y, m)).unwrap_or_default();
        agg.key = format!("{y:04}-{m:02}");
        out.push(agg);
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }
    out
}

fn yearly(recs: &[&Entry]) -> Vec<YearAgg> {
    let mut map: BTreeMap<i32, (YearAgg, f64, f64, f64)> = BTreeMap::new();
    for e in recs {
        let Some((year, _)) = month_key(&e.rec.date) else { continue };
        let (y, seg_l, seg_d, seg_c) = map.entry(year).or_insert_with(|| (YearAgg { year, ..Default::default() }, 0.0, 0.0, 0.0));
        y.cost += e.cost;
        y.liters += e.rec.liters.unwrap_or(0.0);
        y.count += 1;
        y.dist += e.trip.filter(|t| *t > 0.0).unwrap_or(0.0);
        if let Some(g) = &e.seg {
            *seg_l += g.liters;
            *seg_d += g.dist;
            *seg_c += g.cost;
        }
    }
    map.into_values()
        .rev()
        .map(|(mut y, l, d, c)| {
            y.cons = ratio(l * 100.0, d);
            y.cost_per_100km = ratio(c * 100.0, d);
            y
        })
        .collect()
}

fn groups(recs: &[&Entry], key: impl Fn(&Record) -> String) -> Vec<GroupAgg> {
    let mut map: BTreeMap<String, (GroupAgg, f64, f64)> = BTreeMap::new();
    for e in recs {
        let name = key(&e.rec);
        let (g, seg_l, seg_d) = map
            .entry(name.clone())
            .or_insert_with(|| (GroupAgg { name, ..Default::default() }, 0.0, 0.0));
        g.count += 1;
        g.liters += e.rec.liters.unwrap_or(0.0);
        g.cost += e.cost;
        if e.rec.date > g.last_date {
            g.last_date = e.rec.date.clone();
        }
        if let Some(s) = &e.seg {
            *seg_l += s.liters;
            *seg_d += s.dist;
        }
    }
    let mut out: Vec<GroupAgg> = map
        .into_values()
        .map(|(mut g, l, d)| {
            g.avg_price = ratio(g.cost, g.liters);
            g.cons = ratio(l * 100.0, d);
            g
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(b.cost.total_cmp(&a.cost)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, date: &str, odo: f64, liters: f64, total: f64, full: bool) -> Record {
        Record {
            id: id.into(),
            vehicle_id: "v".into(),
            date: format!("{date}T10:00"),
            odometer: Some(odo),
            liters: Some(liters),
            total: Some(total),
            full,
            ..Default::default()
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn trip_rec(id: &str, date: &str, trip: f64, liters: f64, full: bool) -> Record {
        Record {
            id: id.into(),
            date: format!("{date}T10:00"),
            trip: Some(trip),
            liters: Some(liters),
            total: Some(liters * 1.8),
            full,
            ..Default::default()
        }
    }

    #[test]
    fn trip_distances_without_odometer() {
        let e = analyze(&[
            trip_rec("a", "2026-01-01", 500.0, 30.0, true),
            trip_rec("b", "2026-01-08", 200.0, 10.0, false),
            trip_rec("c", "2026-01-15", 300.0, 20.0, true),
        ]);
        assert_eq!(e[0].state, SegState::Ok);
        assert!(close(e[0].seg.as_ref().unwrap().cons, 6.0));
        assert_eq!(e[1].state, SegState::Partial);
        let g = e[2].seg.as_ref().unwrap();
        assert!(close(g.dist, 500.0));
        assert!(close(g.cons, 6.0));
        assert_eq!(e[2].trip, Some(300.0));
    }

    #[test]
    fn first_full_tank_with_trip_and_odometer() {
        let mut a = rec("a", "2026-01-01", 1000.0, 35.0, 63.0, true);
        a.trip = Some(500.0);
        let e = analyze(&[a, rec("b", "2026-01-15", 1600.0, 36.0, 64.0, true)]);
        assert!(close(e[0].seg.as_ref().unwrap().cons, 7.0));
        assert!(close(e[1].seg.as_ref().unwrap().cons, 6.0));
        let s = compute(&e, &Range::default());
        assert_eq!(s.seg_count, 2);
        assert!(close(s.avg_cons.unwrap(), 71.0 / 1100.0 * 100.0));
    }

    #[test]
    fn unknown_distance_is_invalid() {
        let mut b = rec("b", "2026-01-15", 0.0, 30.0, 54.0, true);
        b.odometer = None;
        b.trip = None;
        let e = analyze(&[rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true), b]);
        assert_eq!(e[1].state, SegState::Invalid);
    }

    #[test]
    fn full_to_full_consumption() {
        let e = analyze(&[
            rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true),
            rec("b", "2026-01-15", 1500.0, 30.0, 54.0, true),
        ]);
        assert_eq!(e[0].state, SegState::Start);
        assert_eq!(e[1].state, SegState::Ok);
        assert_eq!(e[1].trip, Some(500.0));
        let g = e[1].seg.as_ref().unwrap();
        assert!(close(g.cons, 6.0));
        assert!(close(g.cost, 54.0));
    }

    #[test]
    fn partial_fills_accumulate_into_next_full() {
        let e = analyze(&[
            rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true),
            rec("b", "2026-01-08", 1300.0, 10.0, 18.0, false),
            rec("c", "2026-01-15", 1600.0, 26.0, 46.0, true),
        ]);
        assert_eq!(e[1].state, SegState::Partial);
        let g = e[2].seg.as_ref().unwrap();
        assert!(close(g.liters, 36.0));
        assert!(close(g.dist, 600.0));
        assert!(close(g.cons, 6.0));
        assert_eq!(g.fills, 2);
    }

    #[test]
    fn missed_previous_breaks_chain() {
        let mut c = rec("c", "2026-02-01", 2200.0, 35.0, 60.0, true);
        c.missed_previous = true;
        let e = analyze(&[
            rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true),
            rec("b", "2026-01-15", 1500.0, 30.0, 54.0, true),
            c,
            rec("d", "2026-02-15", 2700.0, 32.5, 58.0, true),
        ]);
        assert_eq!(e[2].state, SegState::Start);
        assert!(e[2].seg.is_none());
        assert!(close(e[3].seg.as_ref().unwrap().cons, 6.5));
    }

    #[test]
    fn partial_without_start_and_bad_odometer() {
        let e = analyze(&[
            rec("a", "2026-01-01", 1000.0, 20.0, 35.0, false),
            rec("b", "2026-01-10", 1400.0, 40.0, 70.0, true),
            rec("c", "2026-01-20", 1400.0, 5.0, 9.0, true),
            rec("d", "2026-01-30", 1900.0, 30.0, 54.0, true),
        ]);
        assert_eq!(e[0].state, SegState::NoStart);
        assert_eq!(e[1].state, SegState::Start);
        assert_eq!(e[2].state, SegState::Invalid);
        assert!(close(e[3].seg.as_ref().unwrap().cons, 6.0));
    }

    #[test]
    fn deleted_records_are_ignored_and_sorting_is_by_date() {
        let mut x = rec("x", "2026-01-05", 1200.0, 5.0, 9.0, true);
        x.deleted = true;
        let e = analyze(&[
            rec("b", "2026-01-15", 1500.0, 30.0, 54.0, true),
            x,
            rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true),
        ]);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].rec.id, "a");
    }

    #[test]
    fn stats_totals_and_averages() {
        let mut b = rec("b", "2026-01-31", 1500.0, 30.0, 54.0, true);
        b.paid = Some(50.0);
        let e = analyze(&[
            rec("a", "2026-01-01", 1000.0, 40.0, 70.0, true),
            b,
            rec("c", "2026-03-02", 2000.0, 35.0, 63.0, true),
        ]);
        let s = compute(&e, &Range::default());
        assert_eq!(s.count, 3);
        assert!(close(s.total_cost, 183.0));
        assert!(close(s.total_liters, 105.0));
        assert!(close(s.savings, 4.0));
        assert!(close(s.distance, 1000.0));
        assert!(close(s.avg_cons.unwrap(), 6.5));
        assert!(close(s.cost_per_100km.unwrap(), 11.3));
        assert!(close(s.best_cons.unwrap(), 6.0));
        assert!(close(s.worst_cons.unwrap(), 7.0));
        assert!(close(s.last_cons.unwrap(), 7.0));
        assert!(close(s.avg_interval_days.unwrap(), 30.0));
        assert!(close(s.avg_interval_km.unwrap(), 500.0));
        assert!(close(s.cost_per_month.unwrap(), 113.0 / 60.0 * DAYS_PER_MONTH));
        assert_eq!(s.min_price.as_ref().unwrap().date, "2026-01-31T10:00");
        let months: Vec<_> = s.monthly.iter().map(|m| (m.key.as_str(), m.count)).collect();
        assert_eq!(months, [("2026-01", 2), ("2026-02", 0), ("2026-03", 1)]);
        assert_eq!(s.cons_series.len(), 2);
        assert_eq!(s.price_series.len(), 3);
    }

    #[test]
    fn range_filters_records_and_segments() {
        let e = analyze(&[
            rec("a", "2025-12-01", 1000.0, 40.0, 70.0, true),
            rec("b", "2026-01-15", 1500.0, 30.0, 54.0, true),
            rec("c", "2027-01-15", 2000.0, 40.0, 72.0, true),
        ]);
        let r = Range { from: Some("2026-01-01".into()), to: Some("2027-01-01".into()) };
        let s = compute(&e, &r);
        assert_eq!(s.count, 1);
        assert!(close(s.avg_cons.unwrap(), 6.0));
        assert!(s.cost_per_month.is_none());
        assert!(compute(&e, &Range { from: Some("2030-01-01".into()), to: None }).avg_price.is_none());
    }

    #[test]
    fn groups_and_years() {
        let mut a = rec("a", "2025-12-01", 1000.0, 40.0, 70.0, true);
        a.station = "Aral".into();
        let mut b = rec("b", "2026-01-15", 1500.0, 30.0, 54.0, true);
        b.station = "Aral".into();
        let mut c = rec("c", "2026-02-15", 2000.0, 40.0, 72.0, true);
        c.station = "Shell".into();
        let s = compute(&analyze(&[a, b, c]), &Range::default());
        assert_eq!(s.stations[0].name, "Aral");
        assert_eq!(s.stations[0].count, 2);
        assert!(close(s.stations[0].avg_price.unwrap(), 124.0 / 70.0));
        assert!(close(s.stations[1].cons.unwrap(), 8.0));
        assert_eq!(s.yearly.iter().map(|y| y.year).collect::<Vec<_>>(), [2026, 2025]);
        assert!(close(s.yearly[0].cons.unwrap(), 7.0));
        assert!(s.yearly[1].cons.is_none());
    }
}

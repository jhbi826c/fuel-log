use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One fill-up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    #[serde(default)]
    pub vehicle_id: String,
    /// Local date-time "YYYY-MM-DDTHH:MM".
    #[serde(default)]
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub odometer: Option<f64>,
    /// Distance driven since the previous fill-up, as entered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liters: Option<f64>,
    /// Pump price per liter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// Pump total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    /// Amount actually paid, if different from the pump total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid: Option<f64>,
    #[serde(default = "yes")]
    pub full: bool,
    /// A fill-up before this one was not recorded.
    #[serde(default, skip_serializing_if = "is_false")]
    pub missed_previous: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fuel: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub station: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lng: Option<f64>,
    /// Consumption shown by the car's trip computer, L/100 km.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_cons: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub payment: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub updated_at: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub demo: bool,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            id: String::new(),
            vehicle_id: String::new(),
            date: String::new(),
            odometer: None,
            trip: None,
            liters: None,
            price: None,
            total: None,
            paid: None,
            full: true,
            missed_previous: false,
            fuel: String::new(),
            station: String::new(),
            address: String::new(),
            lat: None,
            lng: None,
            display_cons: None,
            payment: String::new(),
            notes: String::new(),
            created_at: 0.0,
            updated_at: 0.0,
            deleted: false,
            demo: false,
        }
    }
}

impl Record {
    /// Money spent on this fill-up.
    pub fn cost(&self) -> f64 {
        self.paid
            .or(self.total)
            .or(match (self.liters, self.price) {
                (Some(l), Some(p)) => Some(l * p),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    pub fn coords(&self) -> Option<(f64, f64)> {
        Some((self.lat?, self.lng?))
    }

    /// Whether the record is usable (tombstones are always kept).
    pub fn is_valid(&self) -> bool {
        self.deleted || (!self.id.is_empty() && self.date.len() >= 10 && (self.odometer.is_some() || self.trip.is_some()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vehicle {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub updated_at: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
    /// Fuel types this vehicle uses; empty means any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fuels: Vec<String>,
}

/// Data that is exported and synced.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppData {
    pub version: u32,
    pub vehicles: Vec<Vehicle>,
    pub records: Vec<Record>,
}

trait Versioned {
    fn id(&self) -> &str;
    fn updated_at(&self) -> f64;
}

impl Versioned for Record {
    fn id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> f64 {
        self.updated_at
    }
}

impl Versioned for Vehicle {
    fn id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> f64 {
        self.updated_at
    }
}

/// Merges by id, newest `updated_at` wins; returns the number of changed items.
fn merge_list<T: Versioned + Clone>(local: &mut Vec<T>, remote: &[T]) -> usize {
    let mut changed = 0;
    for r in remote {
        if r.id().is_empty() {
            continue;
        }
        match local.iter_mut().find(|l| l.id() == r.id()) {
            Some(l) if r.updated_at() > l.updated_at() => {
                *l = r.clone();
                changed += 1;
            }
            Some(_) => {}
            None => {
                local.push(r.clone());
                changed += 1;
            }
        }
    }
    changed
}

impl AppData {
    /// Merges another data set into this one; returns the number of changed items.
    pub fn merge(&mut self, other: &AppData) -> usize {
        let other = other.clone().normalized();
        merge_list(&mut self.records, &other.records) + merge_list(&mut self.vehicles, &other.vehicles)
    }

    /// Drops invalid entries and sorts by id.
    pub fn normalized(mut self) -> Self {
        self.version = 1;
        self.records.retain(Record::is_valid);
        self.vehicles.retain(|v| !v.id.is_empty());
        self.records.sort_by(|a, b| a.id.cmp(&b.id));
        self.vehicles.sort_by(|a, b| a.id.cmp(&b.id));
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum MapProvider {
    #[default]
    Google,
    Apple,
    Osm,
    OsmAnd,
}

impl From<String> for MapProvider {
    fn from(s: String) -> Self {
        Self::from_key(&s)
    }
}

impl From<MapProvider> for String {
    fn from(p: MapProvider) -> Self {
        p.key().to_string()
    }
}

impl MapProvider {
    pub const ALL: [MapProvider; 4] = [Self::Google, Self::Apple, Self::Osm, Self::OsmAnd];

    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
            Self::Osm => "osm",
            Self::OsmAnd => "osmand",
        }
    }

    /// Unknown keys (e.g. removed providers) fall back to the default.
    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|p| p.key() == k).unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google Maps",
            Self::Apple => "Apple Maps",
            Self::Osm => "OpenStreetMap",
            Self::OsmAnd => "OsmAnd",
        }
    }

    /// Map link showing a point.
    pub fn point_url(self, lat: f64, lng: f64, label: &str) -> String {
        match self {
            Self::Google => format!("https://www.google.com/maps/search/?api=1&query={lat},{lng}"),
            Self::Apple => format!("https://maps.apple.com/?ll={lat},{lng}&q={}", enc(label)),
            Self::Osm => format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lng}#map=17/{lat}/{lng}"),
            Self::OsmAnd => format!("https://osmand.net/map/?pin={lat},{lng}#17/{lat}/{lng}"),
        }
    }

    /// Map link for a record's location or, failing that, its station name/address.
    pub fn url(self, r: &Record) -> Option<String> {
        let label = if r.station.is_empty() { "Fuel" } else { r.station.as_str() };
        if let Some((lat, lng)) = r.coords() {
            return Some(self.point_url(lat, lng, label));
        }
        let q = [r.station.as_str(), r.address.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if q.is_empty() {
            return None;
        }
        let q = enc(&q);
        Some(match self {
            Self::Google => format!("https://www.google.com/maps/search/?api=1&query={q}"),
            Self::Apple => format!("https://maps.apple.com/?q={q}"),
            Self::Osm | Self::OsmAnd => format!("https://www.openstreetmap.org/search?query={q}"),
        })
    }
}

/// Percent-encodes a URL query component.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Common fuel types and their OpenStreetMap `fuel:*` keys.
pub const FUEL_TYPES: [(&str, &[&str]); 9] = [
    ("Super E10", &["e10"]),
    ("Super E5", &["octane_95"]),
    ("Super Plus", &["octane_98"]),
    ("Super 100", &["octane_100"]),
    ("Diesel", &["diesel"]),
    ("Premium Diesel", &["GTL_diesel", "diesel:premium"]),
    ("LPG", &["lpg"]),
    ("CNG", &["cng"]),
    ("E85", &["e85"]),
];

pub fn is_known_fuel(name: &str) -> bool {
    FUEL_TYPES.iter().any(|(n, _)| *n == name)
}

/// Fuels a vehicle that runs on `fuel` can also take.
fn compatible_fuels(fuel: &str) -> &'static [&'static str] {
    match fuel {
        "Super E10" => &["Super E10", "Super E5", "Super Plus", "Super 100"],
        "Super E5" => &["Super E5", "Super Plus", "Super 100"],
        "Super Plus" => &["Super Plus", "Super 100"],
        "Super 100" => &["Super 100"],
        "Diesel" | "Premium Diesel" => &["Diesel", "Premium Diesel"],
        "LPG" => &["LPG"],
        "CNG" => &["CNG"],
        "E85" => &["E85"],
        _ => &[],
    }
}

/// Known fuels usable by a vehicle set to `fuels`, in list order.
pub fn usable_fuels(fuels: &[String]) -> Vec<&'static str> {
    FUEL_TYPES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| fuels.iter().any(|f| compatible_fuels(f).contains(name)))
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Self::Auto, Self::Light, Self::Dark];

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|t| t.key() == k).unwrap_or_default()
    }
}

/// Per-device preferences (not synced).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub lang: Lang,
    pub currency: String,
    pub map_provider: MapProvider,
    pub active_vehicle: String,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::En,
            currency: "€".into(),
            map_provider: MapProvider::Google,
            active_vehicle: String::new(),
            theme: Theme::Auto,
        }
    }
}

/// GitHub Gist sync configuration (per device, never exported).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncConf {
    pub token: String,
    pub gist_id: String,
    pub auto: bool,
    pub last_sync: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_prefers_paid_then_total_then_product() {
        let mut r = Record { liters: Some(10.0), price: Some(2.0), ..Default::default() };
        assert_eq!(r.cost(), 20.0);
        r.total = Some(19.9);
        assert_eq!(r.cost(), 19.9);
        r.paid = Some(18.0);
        assert_eq!(r.cost(), 18.0);
    }

    #[test]
    fn deserialize_defaults_full_to_true() {
        let r: Record = serde_json::from_str(r#"{"id":"a","date":"2026-01-01T10:00","odometer":5}"#).unwrap();
        assert!(r.is_valid());
        let t: Record = serde_json::from_str(r#"{"id":"t","date":"2026-01-01T10:00","trip":480}"#).unwrap();
        assert!(t.is_valid() && t.trip == Some(480.0));
        assert!(!Record { id: "x".into(), date: "2026-01-01T10:00".into(), ..Default::default() }.is_valid());
        assert!(r.full);
        assert!(!r.missed_previous);
    }

    #[test]
    fn roundtrip_skips_empty_fields() {
        let r = Record { id: "x".into(), date: "2026-01-01T10:00".into(), odometer: Some(1.0), ..Default::default() };
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("station"));
        assert_eq!(serde_json::from_str::<Record>(&s).unwrap(), r);
    }

    #[test]
    fn merge_newest_wins_and_tombstones_propagate() {
        let r = |id: &str, t: f64, station: &str| Record {
            id: id.into(),
            date: "2026-01-01T10:00".into(),
            odometer: Some(1.0),
            station: station.into(),
            updated_at: t,
            ..Default::default()
        };
        let mut local = AppData { version: 1, vehicles: vec![], records: vec![r("a", 1.0, "old"), r("b", 5.0, "keep")] };
        let mut deleted = r("b", 4.0, "x");
        deleted.deleted = true;
        let remote = AppData {
            version: 1,
            vehicles: vec![Vehicle { id: "v".into(), name: "Car".into(), updated_at: 1.0, deleted: false, fuels: Vec::new() }],
            records: vec![r("a", 2.0, "new"), deleted, r("c", 1.0, "added"), Record::default()],
        };
        assert_eq!(local.merge(&remote), 3);
        let local = local.normalized();
        assert_eq!(local.records.iter().map(|r| r.station.as_str()).collect::<Vec<_>>(), ["new", "keep", "added"]);
        assert_eq!(local.vehicles.len(), 1);

        let mut newer_delete = r("b", 9.0, "x");
        newer_delete.deleted = true;
        let mut l2 = local.clone();
        l2.merge(&AppData { records: vec![newer_delete], ..Default::default() });
        assert!(l2.records.iter().find(|r| r.id == "b").unwrap().deleted);
    }

    #[test]
    fn removed_map_provider_falls_back() {
        let s: Settings =
            serde_json::from_str(r#"{"mapProvider":"amap","currency":"Kč","tankerkoenigKey":"x","cardMarkup":1.5}"#).unwrap();
        assert_eq!(s.map_provider, MapProvider::Google);
        assert_eq!(s.currency, "Kč");
        assert!(serde_json::to_string(&s).unwrap().contains(r#""mapProvider":"google""#));
        assert_eq!(s.theme, Theme::Auto);
        let dark: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(dark.theme, Theme::Dark);
    }

    #[test]
    fn compatible_fuels_are_usable() {
        let v = |f: &[&str]| f.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(usable_fuels(&v(&["Super E10"])), ["Super E10", "Super E5", "Super Plus", "Super 100"]);
        assert_eq!(usable_fuels(&v(&["Diesel"])), ["Diesel", "Premium Diesel"]);
        assert_eq!(usable_fuels(&v(&["LPG", "Super E5"])), ["Super E5", "Super Plus", "Super 100", "LPG"]);
        assert!(usable_fuels(&v(&["AdBlue"])).is_empty());
    }

    #[test]
    fn map_url_encodes() {
        let r = Record { station: "Aral Straße".into(), ..Default::default() };
        assert_eq!(
            MapProvider::Google.url(&r).unwrap(),
            "https://www.google.com/maps/search/?api=1&query=Aral%20Stra%C3%9Fe"
        );
        assert!(MapProvider::Osm.url(&Record::default()).is_none());
    }
}

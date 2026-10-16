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
        self.deleted || (!self.id.is_empty() && self.date.len() >= 10 && self.odometer.is_some())
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

    /// Link that starts navigation to a point where the provider supports it.
    pub fn route_url(self, lat: f64, lng: f64, label: &str) -> String {
        match self {
            Self::Google => format!("https://www.google.com/maps/dir/?api=1&destination={lat},{lng}"),
            Self::Apple => format!("https://maps.apple.com/?daddr={lat},{lng}&q={}", enc(label)),
            Self::Osm | Self::OsmAnd => self.point_url(lat, lng, label),
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

/// Fuel grades that price sources report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FuelKind {
    #[default]
    E10,
    E5,
    Diesel,
    Premium,
    Lpg,
}

impl FuelKind {
    pub const ALL: [FuelKind; 5] = [Self::E10, Self::E5, Self::Diesel, Self::Premium, Self::Lpg];

    pub fn key(self) -> &'static str {
        match self {
            Self::E10 => "e10",
            Self::E5 => "e5",
            Self::Diesel => "diesel",
            Self::Premium => "premium",
            Self::Lpg => "lpg",
        }
    }

    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|f| f.key() == k).unwrap_or_default()
    }

    /// Name used in records and selectors.
    pub fn name(self) -> &'static str {
        match self {
            Self::E10 => "Super E10",
            Self::E5 => "Super E5 (95)",
            Self::Diesel => "Diesel",
            Self::Premium => "Super Plus (98)",
            Self::Lpg => "LPG",
        }
    }

    /// Best guess for a free-text fuel name.
    pub fn from_name(s: &str) -> Option<Self> {
        let s = s.to_lowercase();
        if s.contains("e10") {
            Some(Self::E10)
        } else if s.contains("diesel") || s.contains("柴油") || s.contains("nafta") || s.contains("gazole") {
            Some(Self::Diesel)
        } else if s.contains("lpg") || s.contains("autogas") || s.contains("gpl") {
            Some(Self::Lpg)
        } else if s.contains("98") || s.contains("plus") || s.contains("v-power") || s.contains("ultimate") {
            Some(Self::Premium)
        } else if s.contains("e5") || s.contains("95") || s.contains("super") || s.contains("natural") {
            Some(Self::E5)
        } else {
            None
        }
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

/// Per-device preferences (not synced).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub lang: Lang,
    pub currency: String,
    pub map_provider: MapProvider,
    pub active_vehicle: String,
    pub tankerkoenig_key: String,
    pub search_fuel: FuelKind,
    pub search_radius: u32,
    pub sort_by_price: bool,
    pub only_open: bool,
    /// Foreign-currency fee of the payment card, percent.
    pub card_markup: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::En,
            currency: "€".into(),
            map_provider: MapProvider::Google,
            active_vehicle: String::new(),
            tankerkoenig_key: String::new(),
            search_fuel: FuelKind::E10,
            search_radius: 10,
            sort_by_price: true,
            only_open: true,
            card_markup: 0.0,
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
            vehicles: vec![Vehicle { id: "v".into(), name: "Car".into(), updated_at: 1.0, deleted: false }],
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
        let s: Settings = serde_json::from_str(r#"{"mapProvider":"amap","currency":"Kč"}"#).unwrap();
        assert_eq!(s.map_provider, MapProvider::Google);
        assert_eq!(s.currency, "Kč");
        assert!(serde_json::to_string(&s).unwrap().contains(r#""mapProvider":"google""#));
    }

    #[test]
    fn fuel_names() {
        assert_eq!(FuelKind::from_name("Super E10"), Some(FuelKind::E10));
        assert_eq!(FuelKind::from_name("Natural 95"), Some(FuelKind::E5));
        assert_eq!(FuelKind::from_name("0# 柴油"), Some(FuelKind::Diesel));
        assert_eq!(FuelKind::from_name("Super Plus (98)"), Some(FuelKind::Premium));
        assert_eq!(FuelKind::from_name("Autogas"), Some(FuelKind::Lpg));
        assert_eq!(FuelKind::from_name("AdBlue"), None);
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

use std::cell::RefCell;

use serde_json::Value;

use super::Station;
use super::sources::{get_json, join_address, num, text};

const LIST_URL: &str = "https://creativecommons.tankerkoenig.de/json/list.php";
pub const SIGNUP_URL: &str = "https://creativecommons.tankerkoenig.de/";
pub const RADII_KM: [u32; 5] = [2, 5, 10, 15, 25];
/// How long a search result is reused for the same place and radius.
const CACHE_MS: f64 = 5.0 * 60_000.0;
const CACHE_LEN: usize = 8;

/// Fuels reported by Tankerkönig.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TkFuel {
    #[default]
    E10,
    E5,
    Diesel,
}

impl TkFuel {
    pub const ALL: [TkFuel; 3] = [Self::E10, Self::E5, Self::Diesel];

    /// Matching name in `FUEL_TYPES`.
    pub fn name(self) -> &'static str {
        match self {
            Self::E10 => "Super E10",
            Self::E5 => "Super E5",
            Self::Diesel => "Diesel",
        }
    }

    /// Best fit for a vehicle's fuel list.
    pub fn for_vehicle(fuels: &[String]) -> Self {
        let has = |names: &[&str]| fuels.iter().any(|f| names.contains(&f.as_str()));
        if has(&["Diesel", "Premium Diesel"]) {
            Self::Diesel
        } else if has(&["Super E10"]) || fuels.is_empty() {
            Self::E10
        } else {
            Self::E5
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TkStation {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub street: String,
    pub address: String,
    pub lat: f64,
    pub lng: f64,
    pub dist_km: f64,
    pub e5: Option<f64>,
    pub e10: Option<f64>,
    pub diesel: Option<f64>,
    pub is_open: bool,
}

impl TkStation {
    pub fn price(&self, fuel: TkFuel) -> Option<f64> {
        match fuel {
            TkFuel::E10 => self.e10,
            TkFuel::E5 => self.e5,
            TkFuel::Diesel => self.diesel,
        }
    }

    pub fn to_station(&self) -> Station {
        Station {
            id: format!("tk:{}", self.id),
            name: self.name.clone(),
            brand: self.brand.clone(),
            street: self.street.clone(),
            address: self.address.clone(),
            lat: self.lat,
            lng: self.lng,
            dist_km: self.dist_km,
            fuels: TkFuel::ALL.into_iter().filter(|f| self.price(*f).is_some()).map(TkFuel::name).collect(),
            fuel_tagged: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TkError {
    InvalidKey,
    Api(String),
    Network(String),
}

/// Whether `s` looks like a Tankerkönig key (a UUID).
pub fn is_valid_key(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12]) && groups.iter().all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Last four characters of a key, for display.
pub fn key_hint(key: &str) -> String {
    format!("••••{}", key.get(key.len().saturating_sub(4)..).unwrap_or_default())
}

pub fn list_url(key: &str, (lat, lng): (f64, f64), radius_km: u32) -> String {
    format!("{LIST_URL}?lat={lat:.5}&lng={lng:.5}&rad={}&sort=dist&type=all&apikey={key}", radius_km.clamp(1, 25))
}

fn price(v: &Value) -> Option<f64> {
    v.as_f64().filter(|p| *p > 0.0)
}

pub fn parse_list(v: &Value) -> Result<Vec<TkStation>, TkError> {
    if v["ok"].as_bool() != Some(true) {
        let msg = text(v, "message");
        let lower = msg.to_lowercase();
        return Err(if lower.contains("key") || lower.contains("apikey") { TkError::InvalidKey } else { TkError::Api(msg) });
    }
    let list = v["stations"].as_array().ok_or_else(|| TkError::Api("no stations".into()))?;
    Ok(list
        .iter()
        .filter_map(|s| {
            let street = text(s, "street");
            Some(TkStation {
                id: text(s, "id"),
                name: text(s, "name"),
                brand: text(s, "brand"),
                address: join_address(&street, &text(s, "houseNumber"), &text(s, "postCode"), &text(s, "place")),
                street,
                lat: num(&s["lat"])?,
                lng: num(&s["lng"])?,
                dist_km: num(&s["dist"]).unwrap_or(0.0),
                e5: price(&s["e5"]),
                e10: price(&s["e10"]),
                diesel: price(&s["diesel"]),
                is_open: s["isOpen"].as_bool().unwrap_or(true),
            })
        })
        .collect())
}

type CacheEntry = (String, f64, Vec<TkStation>);

thread_local! {
    static CACHE: RefCell<Vec<CacheEntry>> = const { RefCell::new(Vec::new()) };
}

/// Stations around a position with their current prices.
pub async fn stations(key: &str, origin: (f64, f64), radius_km: u32) -> Result<Vec<TkStation>, TkError> {
    let cache_key = format!("{:.3},{:.3},{radius_km}", origin.0, origin.1);
    let now = crate::web::now_ms();
    let hit = CACHE.with_borrow(|c| c.iter().find(|(k, t, _)| *k == cache_key && now - t < CACHE_MS).map(|e| e.2.clone()));
    if let Some(list) = hit {
        return Ok(list);
    }
    let v = get_json(&list_url(key, origin, radius_km))
        .await
        .map_err(|e| TkError::Network(e.replace(key, "***")))?;
    let list = parse_list(&v)?;
    CACHE.with_borrow_mut(|c| {
        c.retain(|(k, t, _)| *k != cache_key && now - t < CACHE_MS);
        c.push((cache_key, now, list.clone()));
        if c.len() > CACHE_LEN {
            c.remove(0);
        }
    });
    Ok(list)
}

/// Tries a key with a small search in central Berlin.
pub async fn check_key(key: &str) -> Result<(), TkError> {
    stations(key, (52.5200, 13.4050), 1).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys() {
        assert!(is_valid_key("00000000-0000-0000-0000-000000000002"));
        assert!(is_valid_key("a1B2c3d4-0000-0000-0000-00000000abcd"));
        assert!(!is_valid_key("00000000-0000-0000-0000-00000000000"));
        assert!(!is_valid_key("0000000g-0000-0000-0000-000000000002"));
        assert!(!is_valid_key(""));
        assert_eq!(key_hint("00000000-0000-0000-0000-00000000abcd"), "••••abcd");
    }

    #[test]
    fn parses_list() {
        let v = json!({"ok": true, "status": "ok", "stations": [
            {"id": "474e", "name": "TotalEnergies Berlin", "brand": "TotalEnergies", "street": "Margarete-Sommer-Str.",
             "place": "Berlin", "lat": 52.530831, "lng": 13.440946, "dist": 1.1, "diesel": 1.659, "e5": 1.809,
             "e10": false, "isOpen": false, "houseNumber": "2", "postCode": 10407}
        ]});
        let s = &parse_list(&v).unwrap()[0];
        assert_eq!(s.address, "Margarete-Sommer-Str. 2, 10407 Berlin");
        assert_eq!((s.e5, s.e10, s.diesel), (Some(1.809), None, Some(1.659)));
        assert!(!s.is_open);
        assert_eq!(s.to_station().fuels, ["Super E5", "Diesel"]);
        assert_eq!(s.to_station().label(), "TotalEnergies Margarete-Sommer-Str.");
    }

    #[test]
    fn errors() {
        let bad = json!({"status": "error", "ok": false, "message": "Key existiert nicht oder ist deaktiviert"});
        assert_eq!(parse_list(&bad), Err(TkError::InvalidKey));
        let other = json!({"ok": false, "message": "Radius zu groß"});
        assert_eq!(parse_list(&other), Err(TkError::Api("Radius zu groß".into())));
    }

    #[test]
    fn url_and_fuel() {
        let u = list_url("k", (52.521, 13.438), 40);
        assert!(u.contains("rad=25") && u.contains("lat=52.52100") && u.ends_with("apikey=k"));
        assert_eq!(TkFuel::for_vehicle(&["Premium Diesel".into()]), TkFuel::Diesel);
        assert_eq!(TkFuel::for_vehicle(&["Super E5".into()]), TkFuel::E5);
        assert_eq!(TkFuel::for_vehicle(&[]), TkFuel::E10);
    }
}

use gloo_net::http::Request;
use serde_json::Value;

use super::geo::{distance_km, guess_country, tidy_brand};
use super::{Average, Source, Station};
use crate::model::{FuelKind, enc};

pub type Origin = (f64, f64);

const OVERPASS: [&str; 2] = ["https://overpass-api.de/api/interpreter", "https://overpass.private.coffee/api/interpreter"];
const NOMINATIM: &str = "https://nominatim.openstreetmap.org";
const FUEL_PRICES_EU: &str = "https://www.fuel-prices.eu/live/api.php";

fn text(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok()).filter(|p| *p > 0.0)
}

fn first_text(v: &Value, keys: &[&str]) -> String {
    keys.iter().map(|k| text(v, k)).find(|s| !s.is_empty()).unwrap_or_default()
}

/// "Street 1, 01234 City".
pub fn join_address(street: &str, number: &str, postcode: &str, city: &str) -> String {
    let join = |parts: [&str; 2]| parts.iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    [join([street, number]), join([postcode, city])]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub async fn get_json(url: &str) -> Result<Value, String> {
    let res = Request::get(url).send().await.map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json::<Value>().await.map_err(|e| e.to_string())
}

pub fn tankerkoenig_url(key: &str, (lat, lng): Origin, radius_km: f64) -> String {
    format!(
        "https://creativecommons.tankerkoenig.de/json/list.php?lat={lat:.5}&lng={lng:.5}&rad={:.1}&sort=dist&type=all&apikey={}",
        radius_km.clamp(1.0, 25.0),
        enc(key.trim())
    )
}

pub fn parse_tankerkoenig(v: &Value, origin: Origin) -> Result<Vec<Station>, String> {
    if v["ok"].as_bool() != Some(true) {
        return Err(first_text(v, &["message", "status"]));
    }
    let list = v["stations"].as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .filter_map(|s| {
            let (lat, lng) = (s["lat"].as_f64()?, s["lng"].as_f64()?);
            let mut postcode = text(s, "postCode");
            if !postcode.is_empty() && postcode.len() < 5 && postcode.chars().all(|c| c.is_ascii_digit()) {
                postcode = format!("{postcode:0>5}");
            }
            let street = tidy_brand(&text(s, "street"));
            let prices = [(FuelKind::E5, "e5"), (FuelKind::E10, "e10"), (FuelKind::Diesel, "diesel")]
                .into_iter()
                .filter_map(|(k, key)| Some((k, num(&s[key])?)))
                .collect();
            Some(Station {
                id: format!("tk:{}", text(s, "id")),
                name: tidy_brand(&text(s, "name")),
                brand: tidy_brand(&text(s, "brand")),
                address: join_address(&street, &text(s, "houseNumber"), &postcode, &tidy_brand(&text(s, "place"))),
                street,
                country: Some("DE"),
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
                prices,
                is_open: s["isOpen"].as_bool(),
                source: Source::Tankerkoenig,
                ..Default::default()
            })
        })
        .collect())
}

pub fn econtrol_code(fuel: FuelKind) -> Option<&'static str> {
    match fuel {
        FuelKind::E5 | FuelKind::E10 => Some("SUP"),
        FuelKind::Diesel => Some("DIE"),
        FuelKind::Premium | FuelKind::Lpg => None,
    }
}

pub fn econtrol_url((lat, lng): Origin, code: &str) -> String {
    format!(
        "https://api.e-control.at/sprit/1.0/search/gas-stations/by-address?latitude={lat:.5}&longitude={lng:.5}&fuelType={code}&includeClosed=true"
    )
}

pub fn parse_econtrol(v: &Value, origin: Origin, fuel: FuelKind) -> Result<Vec<Station>, String> {
    let list = v.as_array().ok_or_else(|| first_text(v, &["message", "error"]))?;
    Ok(list
        .iter()
        .filter_map(|s| {
            let loc = &s["location"];
            let (lat, lng) = (num(&loc["latitude"])?, num(&loc["longitude"])?);
            let street = text(loc, "address");
            let prices = s["prices"]
                .as_array()
                .and_then(|ps| ps.iter().find_map(|p| num(&p["amount"])))
                .map(|p| vec![(fuel, p)])
                .unwrap_or_default();
            Some(Station {
                id: format!("ec:{}", text(s, "id")),
                name: text(s, "name"),
                address: join_address(&street, "", &text(loc, "postalCode"), &text(loc, "city")),
                street,
                country: Some("AT"),
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
                prices,
                is_open: s["open"].as_bool(),
                source: Source::EControl,
                ..Default::default()
            })
        })
        .collect())
}

fn fpe_fuel(code: &str) -> Option<FuelKind> {
    match code {
        "e5" | "sp95" => Some(FuelKind::E5),
        "e10" => Some(FuelKind::E10),
        "diesel" => Some(FuelKind::Diesel),
        "sp98" => Some(FuelKind::Premium),
        "gpl" => Some(FuelKind::Lpg),
        _ => None,
    }
}

/// Prices in a fuel-prices.eu object, preferring the first code per fuel.
fn fpe_prices(obj: &Value, value: impl Fn(&Value) -> Option<f64>) -> Vec<(FuelKind, f64)> {
    let mut out: Vec<(FuelKind, f64)> = Vec::new();
    for code in ["e5", "sp95", "e10", "diesel", "sp98", "gpl"] {
        if let (Some(kind), Some(p)) = (fpe_fuel(code), value(&obj[code]))
            && !out.iter().any(|(k, _)| *k == kind)
        {
            out.push((kind, p));
        }
    }
    out
}

pub fn fpe_stations_url(country: &str, (lat, lng): Origin, radius_km: f64) -> String {
    format!(
        "{FUEL_PRICES_EU}?action=stations&country={country}&lat={lat:.5}&lng={lng:.5}&radius={}&limit=100",
        radius_km.ceil().clamp(1.0, 50.0) as u32
    )
}

pub fn parse_fpe_stations(v: &Value, origin: Origin, country: &'static str) -> Result<Vec<Station>, String> {
    if v["ok"].as_bool() != Some(true) {
        return Err(first_text(v, &["error", "message"]));
    }
    let list = v["data"]["stations"].as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .filter_map(|s| {
            let (lat, lng) = (num(&s["lat"])?, num(&s["lng"])?);
            let street = text(s, "address");
            let updated = s["prices"]
                .as_object()
                .and_then(|m| m.values().filter_map(|p| p["updated"].as_str()).max().map(str::to_string))
                .unwrap_or_default();
            Some(Station {
                id: format!("fpe:{country}:{}", text(s, "id")),
                name: text(s, "name"),
                brand: text(s, "brand"),
                address: join_address(&street, "", &text(s, "postal_code"), &text(s, "city")),
                street,
                country: Some(country),
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
                prices: fpe_prices(&s["prices"], |p| num(&p["price"])),
                is_open: s["is_open"].as_bool(),
                source: Source::FuelPricesEu,
                updated,
                ..Default::default()
            })
        })
        .collect())
}

pub fn fpe_summary_url() -> String {
    format!("{FUEL_PRICES_EU}?action=summary")
}

/// Weekly national averages for the given countries.
pub fn parse_fpe_summary(v: &Value, countries: &[&'static str]) -> Vec<Average> {
    let list = v["data"]["countries"].as_array().cloned().unwrap_or_default();
    countries
        .iter()
        .filter_map(|code| {
            let c = list.iter().find(|c| text(c, "country_code").eq_ignore_ascii_case(code))?;
            let prices = fpe_prices(&c["fuels"], |f| num(&f["avg"]));
            (!prices.is_empty()).then(|| Average {
                country: code,
                date: text(c, "price_date"),
                prices,
                currency: text(&c["native"], "currency"),
                native: fpe_prices(&c["native"], num),
            })
        })
        .collect()
}

pub fn overpass_query((lat, lng): Origin, radius_km: f64) -> String {
    format!(
        "[out:json][timeout:20];nwr[\"amenity\"=\"fuel\"](around:{:.0},{lat:.5},{lng:.5});out tags center 300;",
        (radius_km * 1000.0).clamp(200.0, 25_000.0)
    )
}

pub fn parse_overpass(v: &Value, origin: Origin) -> Result<Vec<Station>, String> {
    let list = v["elements"].as_array().ok_or_else(|| first_text(v, &["remark", "error"]))?;
    Ok(list
        .iter()
        .filter_map(|e| {
            let lat = num(&e["lat"]).or_else(|| num(&e["center"]["lat"]))?;
            let lng = num(&e["lon"]).or_else(|| num(&e["center"]["lon"]))?;
            let t = &e["tags"];
            let street = text(t, "addr:street");
            let postcode = text(t, "addr:postcode");
            Some(Station {
                id: format!("osm:{}/{}", text(e, "type"), text(e, "id")),
                name: first_text(t, &["name", "brand", "operator"]),
                brand: text(t, "brand"),
                address: join_address(
                    &street,
                    &text(t, "addr:housenumber"),
                    &postcode,
                    &first_text(t, &["addr:city", "addr:place", "addr:suburb"]),
                ),
                street,
                country: guess_country(lat, lng, &postcode, &text(t, "addr:country")),
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
                source: Source::Osm,
                ..Default::default()
            })
        })
        .collect())
}

fn nominatim_city(a: &Value) -> String {
    first_text(a, &["city", "town", "village", "hamlet", "municipality", "suburb"])
}

pub fn nominatim_search_url((lat, lng): Origin, radius_km: f64) -> String {
    let d_lat = radius_km / 111.0;
    let d_lng = radius_km / (111.0 * lat.to_radians().cos().max(0.1));
    format!(
        "{NOMINATIM}/search?amenity=fuel&viewbox={:.4},{:.4},{:.4},{:.4}&bounded=1&format=jsonv2&addressdetails=1&extratags=1&limit=40",
        lng - d_lng,
        lat + d_lat,
        lng + d_lng,
        lat - d_lat
    )
}

pub fn parse_nominatim_search(v: &Value, origin: Origin) -> Result<Vec<Station>, String> {
    let list = v.as_array().ok_or_else(|| first_text(v, &["error"]))?;
    Ok(list
        .iter()
        .filter_map(|s| {
            let (lat, lng) = (num(&s["lat"])?, num(&s["lon"])?);
            let a = &s["address"];
            let street = text(a, "road");
            let postcode = text(a, "postcode");
            let brand = text(&s["extratags"], "brand");
            Some(Station {
                id: format!("osm:{}/{}", text(s, "osm_type"), text(s, "osm_id")),
                name: if text(s, "name").is_empty() { brand.clone() } else { text(s, "name") },
                brand,
                address: join_address(&street, &text(a, "house_number"), &postcode, &nominatim_city(a)),
                street,
                country: guess_country(lat, lng, &postcode, &text(a, "country_code")),
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
                source: Source::Osm,
                ..Default::default()
            })
        })
        .collect())
}

/// Fuel stations from OpenStreetMap, trying Overpass mirrors before Nominatim.
pub async fn osm_stations(origin: Origin, radius_km: f64) -> Result<Vec<Station>, String> {
    let query = enc(&overpass_query(origin, radius_km));
    let mut last_err = String::new();
    for base in OVERPASS {
        match get_json(&format!("{base}?data={query}")).await.and_then(|v| parse_overpass(&v, origin)) {
            Ok(list) => return Ok(list),
            Err(e) => last_err = e,
        }
    }
    get_json(&nominatim_search_url(origin, radius_km))
        .await
        .and_then(|v| parse_nominatim_search(&v, origin))
        .map_err(|e| format!("{last_err}; {e}"))
}

pub fn parse_reverse(v: &Value) -> Option<(String, Option<&'static str>)> {
    let a = &v["address"];
    let postcode = text(a, "postcode");
    let address = join_address(&text(a, "road"), &text(a, "house_number"), &postcode, &nominatim_city(a));
    let (lat, lng) = (num(&v["lat"]).unwrap_or(0.0), num(&v["lon"]).unwrap_or(0.0));
    (!address.is_empty()).then(|| (address, guess_country(lat, lng, &postcode, &text(a, "country_code"))))
}

/// Country of a position.
pub async fn reverse_country((lat, lng): Origin) -> Option<&'static str> {
    let v = get_json(&format!("{NOMINATIM}/reverse?format=jsonv2&lat={lat:.5}&lon={lng:.5}&zoom=5&addressdetails=1")).await.ok()?;
    guess_country(lat, lng, "", &text(&v["address"], "country_code"))
}

/// Street address of a position.
pub async fn reverse_geocode((lat, lng): Origin) -> Result<Option<String>, String> {
    let v = get_json(&format!("{NOMINATIM}/reverse?format=jsonv2&lat={lat:.6}&lon={lng:.6}&zoom=18&addressdetails=1")).await?;
    Ok(parse_reverse(&v).map(|(a, _)| a))
}

/// Position of a place name.
pub async fn geocode(q: &str) -> Result<Option<(f64, f64, String)>, String> {
    let v = get_json(&format!("{NOMINATIM}/search?q={}&format=jsonv2&limit=1", enc(q.trim()))).await?;
    Ok(v.as_array()
        .and_then(|a| a.first())
        .and_then(|p| Some((num(&p["lat"])?, num(&p["lon"])?, text(p, "display_name")))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const O: Origin = (52.521, 13.438);

    #[test]
    fn tankerkoenig() {
        let v = json!({"ok": true, "stations": [
            {"id": "a1", "name": "Aral Tankstelle", "brand": "ARAL", "street": "HOLZMARKTSTRASSE", "place": "Berlin",
             "lat": 52.514153, "lng": 13.421487, "dist": 1.4, "diesel": 1.609, "e5": 1.809, "e10": false,
             "isOpen": true, "houseNumber": "12/14", "postCode": 1099}
        ]});
        let s = &parse_tankerkoenig(&v, O).unwrap()[0];
        assert_eq!(s.brand, "Aral");
        assert_eq!(s.address, "Holzmarktstrasse 12/14, 01099 Berlin");
        assert_eq!(s.price(FuelKind::E5), Some(1.809));
        assert_eq!(s.price(FuelKind::E10), None);
        assert_eq!(s.country, Some("DE"));
        assert!((s.dist_km - 1.4).abs() < 0.1);
        assert_eq!(parse_tankerkoenig(&json!({"ok": false, "message": "apikey nicht angegeben"}), O).unwrap_err(), "apikey nicht angegeben");
    }

    #[test]
    fn econtrol() {
        let v = json!([{"id": 1494440, "name": "TMC", "location": {"address": "Rechte Wienzeile 43", "postalCode": "1050",
            "city": "Wien", "latitude": 48.1963966, "longitude": 16.358812}, "open": true,
            "prices": [{"fuelType": "DIE", "amount": 2.069}]}, {"id": 2, "name": "No price",
            "location": {"latitude": 48.2, "longitude": 16.36}, "prices": []}]);
        let list = parse_econtrol(&v, (48.2082, 16.3738), FuelKind::Diesel).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].price(FuelKind::Diesel), Some(2.069));
        assert_eq!(list[0].address, "Rechte Wienzeile 43, 1050 Wien");
        assert!(list[1].prices.is_empty());
    }

    #[test]
    fn fuel_prices_eu() {
        let v = json!({"ok": true, "data": {"stations": [{"id": 10076127, "name": "Petrol Ljubljana - Tivolska",
            "brand": "Petrol", "address": "Tivolska Cesta 43", "city": "Ljubljana", "postal_code": "1000",
            "lat": 46.0580724, "lng": 14.5034455, "is_open": true, "prices": {
                "diesel": {"price": 1.881, "updated": "2026-10-16T06:05:48+02:00"},
                "sp95": {"price": 1.742, "updated": "2026-10-16T06:05:48+02:00"},
                "sp98": {"price": 2.111, "updated": "2026-10-16T06:05:48+02:00"}}}]}});
        let s = &parse_fpe_stations(&v, (46.05, 14.5), "SI").unwrap()[0];
        assert_eq!(s.price(FuelKind::E5), Some(1.742));
        assert_eq!(s.price(FuelKind::Premium), Some(2.111));
        assert_eq!(s.updated, "2026-10-16T06:05:48+02:00");

        let summary = json!({"data": {"countries": [{"country_code": "CZ", "price_date": "2026-10-12",
            "fuels": {"diesel": {"avg": 1.921}, "sp95": {"avg": 1.859}},
            "native": {"currency": "CZK", "diesel": 46.86, "sp95": 45.35}}]}});
        let a = &parse_fpe_summary(&summary, &["CZ", "PL"])[0];
        assert_eq!(a.country, "CZ");
        assert_eq!(a.price(FuelKind::E5), Some(1.859));
        assert_eq!(a.currency, "CZK");
        assert_eq!(a.native, vec![(FuelKind::E5, 45.35), (FuelKind::Diesel, 46.86)]);
    }

    #[test]
    fn overpass_and_nominatim() {
        let v = json!({"elements": [
            {"type": "node", "id": 1, "lat": 50.7717, "lon": 14.2066, "tags": {"amenity": "fuel", "name": "Shell",
             "addr:street": "Předmostí", "addr:housenumber": "1960/4", "addr:postcode": "405 02", "addr:city": "Děčín"}},
            {"type": "way", "id": 2, "center": {"lat": 51.06, "lon": 13.75}, "tags": {"brand": "Aral", "addr:postcode": "01099"}}
        ]});
        let list = parse_overpass(&v, (51.0, 14.0)).unwrap();
        assert_eq!(list[0].country, Some("CZ"));
        assert_eq!(list[0].address, "Předmostí 1960/4, 405 02 Děčín");
        assert_eq!(list[1].name, "Aral");
        assert_eq!(list[1].country, Some("DE"));
        assert_eq!(list[1].id, "osm:way/2");

        let n = json!([{"osm_type": "node", "osm_id": 5, "name": "", "lat": "50.7986", "lon": "14.2380",
            "extratags": {"brand": "AVIA"}, "address": {"road": "Loubská", "house_number": "83", "postcode": "405 02",
            "town": "Děčín", "country_code": "cz"}}]);
        let s = &parse_nominatim_search(&n, (51.0, 14.0)).unwrap()[0];
        assert_eq!(s.name, "AVIA");
        assert_eq!(s.country, Some("CZ"));

        let r = json!({"lat": "51.08", "lon": "13.76", "address": {"road": "Am Lagerplatz", "house_number": "2",
            "postcode": "01099", "city": "Dresden", "country_code": "de"}});
        assert_eq!(parse_reverse(&r), Some(("Am Lagerplatz 2, 01099 Dresden".into(), Some("DE"))));
    }

    #[test]
    fn urls() {
        assert!(tankerkoenig_url("k e y", O, 40.0).contains("rad=25.0&sort=dist&type=all&apikey=k%20e%20y"));
        assert!(overpass_query(O, 5.0).contains("around:5000,52.52100,13.43800"));
        assert!(nominatim_search_url(O, 10.0).contains("bounded=1"));
        assert!(fpe_stations_url("FR", O, 80.0).contains("radius=50"));
    }
}

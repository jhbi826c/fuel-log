use futures::future::{Either, select, select_ok};
use gloo_net::http::Request;
use serde_json::Value;

use super::Station;
use super::geo::distance_km;
use crate::model::enc;

pub type Origin = (f64, f64);

const OVERPASS: [&str; 2] = ["https://overpass-api.de/api/interpreter", "https://overpass.private.coffee/api/interpreter"];
const NOMINATIM: &str = "https://nominatim.openstreetmap.org";
const TIMEOUT_MS: i32 = 12_000;
/// How long to keep waiting for Overpass once Nominatim has answered.
const OVERPASS_GRACE_MS: i32 = 3_000;

fn text(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
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

/// GET a JSON document, giving up after 12 s.
pub async fn get_json(url: &str) -> Result<Value, String> {
    crate::web::timeout(TIMEOUT_MS, async {
        let res = Request::get(url).send().await.map_err(|e| e.to_string())?;
        if !res.ok() {
            return Err(format!("HTTP {}", res.status()));
        }
        res.json::<Value>().await.map_err(|e| e.to_string())
    })
    .await?
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
            Some(Station {
                id: format!("osm:{}/{}", text(e, "type"), text(e, "id")),
                name: first_text(t, &["name", "brand", "operator"]),
                brand: text(t, "brand"),
                address: join_address(
                    &street,
                    &text(t, "addr:housenumber"),
                    &text(t, "addr:postcode"),
                    &first_text(t, &["addr:city", "addr:place", "addr:suburb"]),
                ),
                street,
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
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
            let brand = text(&s["extratags"], "brand");
            Some(Station {
                id: format!("osm:{}/{}", text(s, "osm_type"), text(s, "osm_id")),
                name: if text(s, "name").is_empty() { brand.clone() } else { text(s, "name") },
                brand,
                address: join_address(&street, &text(a, "house_number"), &text(a, "postcode"), &nominatim_city(a)),
                street,
                lat,
                lng,
                dist_km: distance_km(origin, (lat, lng)),
            })
        })
        .collect())
}

/// Fuel stations from OpenStreetMap: Overpass mirrors and Nominatim queried in parallel; the first Overpass answer wins, Nominatim is the fallback.
pub async fn osm_stations(origin: Origin, radius_km: f64) -> Result<Vec<Station>, String> {
    let query = enc(&overpass_query(origin, radius_km));
    let overpass = |base: &'static str| {
        let url = format!("{base}?data={query}");
        async move { get_json(&url).await.and_then(|v| parse_overpass(&v, origin)) }
    };
    let nominatim = async {
        get_json(&nominatim_search_url(origin, radius_km))
            .await
            .and_then(|v| parse_nominatim_search(&v, origin))
    };
    let overpass = select_ok([Box::pin(overpass(OVERPASS[0])), Box::pin(overpass(OVERPASS[1]))]);
    match select(overpass, Box::pin(nominatim)).await {
        Either::Left((Ok((list, _)), _)) => Ok(list),
        Either::Left((Err(e), nominatim)) => nominatim.await.map_err(|n| format!("{e}; {n}")),
        Either::Right((Ok(list), overpass)) => match crate::web::timeout(OVERPASS_GRACE_MS, overpass).await {
            Ok(Ok((better, _))) => Ok(better),
            _ => Ok(list),
        },
        Either::Right((Err(n), overpass)) => overpass.await.map(|(list, _)| list).map_err(|e| format!("{e}; {n}")),
    }
}

pub fn parse_reverse(v: &Value) -> Option<String> {
    let a = &v["address"];
    let address = join_address(&text(a, "road"), &text(a, "house_number"), &text(a, "postcode"), &nominatim_city(a));
    (!address.is_empty()).then_some(address)
}

/// Street address of a position.
pub async fn reverse_geocode((lat, lng): Origin) -> Result<Option<String>, String> {
    let v = get_json(&format!("{NOMINATIM}/reverse?format=jsonv2&lat={lat:.6}&lon={lng:.6}&zoom=18&addressdetails=1")).await?;
    Ok(parse_reverse(&v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overpass_and_nominatim() {
        let v = json!({"elements": [
            {"type": "node", "id": 1, "lat": 50.7717, "lon": 14.2066, "tags": {"amenity": "fuel", "name": "Shell",
             "addr:street": "Předmostí", "addr:housenumber": "1960/4", "addr:postcode": "405 02", "addr:city": "Děčín"}},
            {"type": "way", "id": 2, "center": {"lat": 51.06, "lon": 13.75}, "tags": {"brand": "Aral", "addr:postcode": "01099"}}
        ]});
        let list = parse_overpass(&v, (51.0, 14.0)).unwrap();
        assert_eq!(list[0].address, "Předmostí 1960/4, 405 02 Děčín");
        assert_eq!(list[1].name, "Aral");
        assert_eq!(list[1].id, "osm:way/2");

        let n = json!([{"osm_type": "node", "osm_id": 5, "name": "", "lat": "50.7986", "lon": "14.2380",
            "extratags": {"brand": "AVIA"}, "address": {"road": "Loubská", "house_number": "83", "postcode": "405 02",
            "town": "Děčín"}}]);
        let s = &parse_nominatim_search(&n, (51.0, 14.0)).unwrap()[0];
        assert_eq!(s.name, "AVIA");
        assert_eq!(s.address, "Loubská 83, 405 02 Děčín");

        let r = json!({"address": {"road": "Am Lagerplatz", "house_number": "2", "postcode": "01099", "city": "Dresden"}});
        assert_eq!(parse_reverse(&r).as_deref(), Some("Am Lagerplatz 2, 01099 Dresden"));
        assert_eq!(parse_reverse(&json!({"error": "Unable to geocode"})), None);
    }

    #[test]
    fn urls() {
        assert!(overpass_query((52.521, 13.438), 5.0).contains("around:5000,52.52100,13.43800"));
        assert!(nominatim_search_url((52.521, 13.438), 10.0).contains("bounded=1"));
    }
}

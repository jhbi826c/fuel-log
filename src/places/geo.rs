/// Distance between two coordinates in kilometers.
pub fn distance_km((lat1, lng1): (f64, f64), (lat2, lng2): (f64, f64)) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// Where live station prices for a country come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Tankerkoenig,
    EControl,
    FuelPricesEu,
    None,
}

pub struct Country {
    pub code: &'static str,
    /// South, west, north, east.
    pub bbox: (f64, f64, f64, f64),
    /// Postcode shape: `#` digit, `A` letter, other characters literal.
    pub postcode: &'static str,
    /// Allowed first digits of postcodes; empty means any.
    pub first: &'static str,
    pub live: Live,
}

const fn c(code: &'static str, bbox: (f64, f64, f64, f64), postcode: &'static str, first: &'static str, live: Live) -> Country {
    Country { code, bbox, postcode, first, live }
}

pub const COUNTRIES: &[Country] = &[
    c("DE", (47.27, 5.87, 55.06, 15.04), "#####", "", Live::Tankerkoenig),
    c("AT", (46.37, 9.53, 49.02, 17.16), "####", "", Live::EControl),
    c("FR", (41.33, -5.14, 51.09, 9.56), "#####", "", Live::FuelPricesEu),
    c("IT", (35.49, 6.63, 47.09, 18.52), "#####", "", Live::FuelPricesEu),
    c("ES", (27.64, -18.16, 43.79, 4.33), "#####", "", Live::FuelPricesEu),
    c("HR", (42.39, 13.49, 46.55, 19.45), "#####", "", Live::FuelPricesEu),
    c("SI", (45.42, 13.38, 46.88, 16.61), "####", "", Live::FuelPricesEu),
    c("DK", (54.56, 8.07, 57.75, 15.2), "####", "", Live::FuelPricesEu),
    c("SE", (55.34, 11.11, 69.06, 24.17), "### ##", "", Live::FuelPricesEu),
    c("GR", (34.8, 19.37, 41.75, 29.65), "### ##", "", Live::FuelPricesEu),
    c("RO", (43.62, 20.26, 48.27, 29.69), "######", "", Live::FuelPricesEu),
    c("CZ", (48.55, 12.09, 51.06, 18.86), "### ##", "1234567", Live::None),
    c("SK", (47.73, 16.83, 49.61, 22.57), "### ##", "089", Live::None),
    c("PL", (49.0, 14.12, 54.84, 24.15), "##-###", "", Live::None),
    c("HU", (45.74, 16.11, 48.59, 22.9), "####", "", Live::None),
    c("CH", (45.82, 5.96, 47.81, 10.49), "####", "", Live::None),
    c("NL", (50.75, 3.36, 53.56, 7.23), "#### AA", "", Live::None),
    c("BE", (49.5, 2.54, 51.51, 6.41), "####", "", Live::None),
    c("LU", (49.45, 5.73, 50.18, 6.53), "####", "", Live::None),
];

pub fn country(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code.eq_ignore_ascii_case(code))
}

fn in_bbox(c: &Country, lat: f64, lng: f64) -> bool {
    let (s, w, n, e) = c.bbox;
    (s..=n).contains(&lat) && (w..=e).contains(&lng)
}

/// Countries whose bounding box overlaps a circle.
pub fn countries_near(lat: f64, lng: f64, radius_km: f64) -> Vec<&'static Country> {
    let d_lat = radius_km / 111.0;
    let d_lng = radius_km / (111.0 * lat.to_radians().cos().max(0.1));
    COUNTRIES
        .iter()
        .filter(|c| {
            let (s, w, n, e) = c.bbox;
            lat - d_lat <= n && lat + d_lat >= s && lng - d_lng <= e && lng + d_lng >= w
        })
        .collect()
}

fn shape_matches(pattern: &str, value: &str) -> bool {
    value.chars().count() == pattern.chars().count()
        && pattern.chars().zip(value.chars()).all(|(p, v)| match p {
            '#' => v.is_ascii_digit(),
            'A' => v.is_ascii_alphabetic(),
            _ => p == v,
        })
}

fn first_ok(c: &Country, value: &str) -> bool {
    c.first.is_empty() || value.chars().next().is_some_and(|d| c.first.contains(d))
}

fn compact(s: &str) -> String {
    s.chars().filter(|ch| *ch != ' ').collect()
}

/// Postcode written exactly in the country's format.
fn postcode_exact(c: &Country, value: &str) -> bool {
    shape_matches(c.postcode, value) && first_ok(c, value)
}

/// Postcode matching the country's format when spaces are ignored.
fn postcode_loose(c: &Country, value: &str) -> bool {
    shape_matches(&compact(c.postcode), &compact(value)) && first_ok(c, value)
}

/// Country of a station from an explicit code, or from its position and postcode.
pub fn guess_country(lat: f64, lng: f64, postcode: &str, explicit: &str) -> Option<&'static str> {
    if let Some(c) = country(explicit.trim()) {
        return Some(c.code);
    }
    let candidates: Vec<&Country> = COUNTRIES.iter().filter(|c| in_bbox(c, lat, lng)).collect();
    if let Some((prefix, _)) = postcode.trim().split_once('-') {
        let code = match prefix.to_ascii_uppercase().as_str() {
            "D" => "DE",
            "A" => "AT",
            "L" => "LU",
            "B" => "BE",
            "F" => "FR",
            "I" => "IT",
            "H" => "HU",
            other => other,
        }
        .to_string();
        if let Some(c) = candidates.iter().find(|c| c.code == code) {
            return Some(c.code);
        }
    }
    let value = postcode.trim();
    let value = value.rsplit_once('-').filter(|(p, _)| p.chars().all(|c| c.is_ascii_alphabetic())).map_or(value, |(_, v)| v);
    if !value.is_empty() {
        let unique = |list: Vec<&&Country>| (list.len() == 1).then(|| list[0].code);
        if value.contains([' ', '-'])
            && let Some(code) = unique(candidates.iter().filter(|c| postcode_exact(c, value)).collect())
        {
            return Some(code);
        }
        let loose: Vec<&&Country> = candidates.iter().filter(|c| postcode_loose(c, value)).collect();
        if let Some(code) = unique(loose.clone()) {
            return Some(code);
        }
        if let Some(code) = unique(loose.into_iter().filter(|c| !c.first.is_empty()).collect()) {
            return Some(code);
        }
    }
    (candidates.len() == 1).then(|| candidates[0].code)
}

/// "ARAL" → "Aral"; mixed-case and short names are kept.
pub fn tidy_brand(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            if w.chars().count() > 3 && w.chars().all(|c| !c.is_lowercase()) {
                let mut cs = w.chars();
                cs.next()
                    .map(|f| f.to_string() + &cs.as_str().to_lowercase())
                    .unwrap_or_default()
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        let d = distance_km((51.0504, 13.7373), (50.7726, 14.2128));
        assert!((d - 45.0).abs() < 2.0, "{d}");
    }

    #[test]
    fn countries_around_dresden() {
        let codes: Vec<_> = countries_near(51.05, 13.74, 25.0).iter().map(|c| c.code).collect();
        assert!(codes.contains(&"DE") && codes.contains(&"CZ"));
        assert!(!codes.contains(&"AT"));
    }

    #[test]
    fn border_country_guess() {
        assert_eq!(guess_country(51.06, 13.75, "01099", ""), Some("DE"));
        assert_eq!(guess_country(50.77, 14.2, "405 02", ""), Some("CZ"));
        assert_eq!(guess_country(50.79, 14.24, "40502", ""), Some("CZ"));
        assert_eq!(guess_country(50.91, 14.13, "01814", ""), Some("DE"));
        assert_eq!(guess_country(48.6, 13.4, "94032", ""), Some("DE"));
        assert_eq!(guess_country(49.1, 17.9, "763 26", ""), Some("CZ"));
        assert_eq!(guess_country(49.1, 17.9, "911 01", ""), Some("SK"));
        assert_eq!(guess_country(51.15, 14.99, "59-900", ""), Some("PL"));
        assert_eq!(guess_country(50.77, 14.2, "", "cz"), Some("CZ"));
        assert_eq!(guess_country(50.77, 14.2, "", ""), None);
        assert_eq!(guess_country(49.6, 6.1, "L-1234", ""), Some("LU"));
    }

    #[test]
    fn brands() {
        assert_eq!(tidy_brand("ARAL"), "Aral");
        assert_eq!(tidy_brand("JET"), "JET");
        assert_eq!(tidy_brand("TotalEnergies"), "TotalEnergies");
        assert_eq!(tidy_brand("AGIP ENI"), "Agip ENI");
    }
}

pub mod geo;
mod sources;

use futures::future::join_all;

pub use sources::{geocode, reverse_geocode};

use crate::model::FuelKind;
use geo::{Live, countries_near, country, distance_km};
use sources::{Origin, get_json};

/// Where a station entry comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    Tankerkoenig,
    EControl,
    FuelPricesEu,
    #[default]
    Osm,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tankerkoenig => "Tankerkönig (MTS-K)",
            Self::EControl => "E-Control",
            Self::FuelPricesEu => "fuel-prices.eu",
            Self::Osm => "OpenStreetMap",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub street: String,
    pub address: String,
    pub country: Option<&'static str>,
    pub lat: f64,
    pub lng: f64,
    pub dist_km: f64,
    /// Live prices in EUR per liter.
    pub prices: Vec<(FuelKind, f64)>,
    /// Weekly national average for the searched fuel, EUR per liter.
    pub avg: Option<f64>,
    pub is_open: Option<bool>,
    pub source: Source,
    pub updated: String,
}

impl Station {
    pub fn price(&self, fuel: FuelKind) -> Option<f64> {
        self.prices.iter().find(|(k, _)| *k == fuel).map(|(_, p)| *p)
    }

    /// Name used when recording a fill-up here.
    pub fn label(&self) -> String {
        if !self.brand.is_empty() && !self.street.is_empty() {
            format!("{} {}", self.brand, self.street)
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.brand.clone()
        }
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() { self.brand.clone() } else { self.name.clone() }
    }
}

/// Weekly national average prices of a country without station data.
#[derive(Clone, Debug, PartialEq)]
pub struct Average {
    pub country: &'static str,
    pub date: String,
    /// EUR per liter.
    pub prices: Vec<(FuelKind, f64)>,
    pub currency: String,
    /// Per liter in `currency`.
    pub native: Vec<(FuelKind, f64)>,
}

impl Average {
    pub fn price(&self, fuel: FuelKind) -> Option<f64> {
        self.prices.iter().find(|(k, _)| *k == fuel).map(|(_, p)| *p)
    }

    pub fn native_price(&self, fuel: FuelKind) -> Option<f64> {
        self.native.iter().find(|(k, _)| *k == fuel).map(|(_, p)| *p)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Issue {
    MissingKey,
    Failed(Source, String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResult {
    pub stations: Vec<Station>,
    pub averages: Vec<Average>,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub lat: f64,
    pub lng: f64,
    pub radius_km: f64,
    pub fuel: FuelKind,
    pub tankerkoenig_key: String,
}

/// Combines priced stations with OSM stations that no price source covers.
pub fn merge(priced: Vec<Station>, osm: Vec<Station>, origin: Origin, radius_km: f64) -> Vec<Station> {
    let mut out: Vec<Station> = Vec::new();
    for s in priced {
        if !out.iter().any(|o| distance_km((o.lat, o.lng), (s.lat, s.lng)) < 0.05) {
            out.push(s);
        }
    }
    let priced_len = out.len();
    for s in osm {
        if !out[..priced_len].iter().any(|o| distance_km((o.lat, o.lng), (s.lat, s.lng)) < 0.12) {
            out.push(s);
        }
    }
    for s in &mut out {
        s.dist_km = distance_km(origin, (s.lat, s.lng));
    }
    out.retain(|s| s.dist_km <= radius_km + 0.05);
    out
}

/// Assigns the search center's country to stations of unknown country inside its bounding box.
pub fn fill_countries(stations: &mut [Station], center: Option<&'static str>) {
    let Some(c) = center.and_then(country) else { return };
    let (s, w, n, e) = c.bbox;
    for st in stations.iter_mut().filter(|st| st.country.is_none()) {
        if (s..=n).contains(&st.lat) && (w..=e).contains(&st.lng) {
            st.country = Some(c.code);
        }
    }
}

/// Sorts by live price (unpriced last, by distance) or by distance.
pub fn sort_stations(list: &mut [Station], fuel: FuelKind, by_price: bool) {
    list.sort_by(|a, b| {
        let dist = a.dist_km.total_cmp(&b.dist_km);
        if !by_price {
            return dist;
        }
        match (a.price(fuel), b.price(fuel)) {
            (Some(x), Some(y)) => x.total_cmp(&y).then(dist),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => dist,
        }
    });
}

/// Searches all sources that cover the area around a position.
pub async fn search(q: Query) -> SearchResult {
    let origin = (q.lat, q.lng);
    let near = countries_near(q.lat, q.lng, q.radius_km);
    let key = q.tankerkoenig_key.trim().to_string();
    let wants = |live: Live| near.iter().any(|c| c.live == live);
    let mut issues = Vec::new();

    let tk = async {
        if !wants(Live::Tankerkoenig) || key.is_empty() {
            return None;
        }
        let url = sources::tankerkoenig_url(&key, origin, q.radius_km);
        Some(get_json(&url).await.and_then(|v| sources::parse_tankerkoenig(&v, origin)))
    };
    let ec = async {
        let code = sources::econtrol_code(q.fuel).filter(|_| wants(Live::EControl))?;
        let url = sources::econtrol_url(origin, code);
        Some(get_json(&url).await.and_then(|v| sources::parse_econtrol(&v, origin, q.fuel)))
    };
    let fpe = join_all(near.iter().filter(|c| c.live == Live::FuelPricesEu).map(|c| async move {
        let url = sources::fpe_stations_url(c.code, origin, q.radius_km);
        get_json(&url).await.and_then(|v| sources::parse_fpe_stations(&v, origin, c.code))
    }));
    let osm = sources::osm_stations(origin, q.radius_km);
    let center = sources::reverse_country(origin);
    let (tk, ec, fpe, osm, center) = futures::join!(tk, ec, fpe, osm, center);

    let mut priced = Vec::new();
    let tagged = [(Source::Tankerkoenig, tk), (Source::EControl, ec)]
        .into_iter()
        .filter_map(|(s, r)| Some((s, r?)))
        .chain(fpe.into_iter().map(|r| (Source::FuelPricesEu, r)));
    for (source, res) in tagged {
        match res {
            Ok(list) => priced.extend(list),
            Err(e) => issues.push(Issue::Failed(source, e)),
        }
    }
    let osm = osm.unwrap_or_else(|e| {
        issues.push(Issue::Failed(Source::Osm, e));
        Vec::new()
    });
    let mut stations = merge(priced, osm, origin, q.radius_km);
    fill_countries(&mut stations, center);
    let german = center == Some("DE") || stations.iter().any(|s| s.country == Some("DE"));
    if german && key.is_empty() {
        issues.insert(0, Issue::MissingKey);
    }

    let mut without_live: Vec<&'static str> = stations
        .iter()
        .filter(|s| s.source == Source::Osm)
        .filter_map(|s| s.country)
        .filter(|c| country(c).is_some_and(|c| c.live == Live::None))
        .collect();
    without_live.sort_unstable();
    without_live.dedup();
    let mut averages = Vec::new();
    if !without_live.is_empty() {
        match get_json(&sources::fpe_summary_url()).await {
            Ok(v) => averages = sources::parse_fpe_summary(&v, &without_live),
            Err(e) => issues.push(Issue::Failed(Source::FuelPricesEu, e)),
        }
        for s in stations.iter_mut().filter(|s| s.source == Source::Osm) {
            s.avg = averages.iter().find(|a| Some(a.country) == s.country).and_then(|a| a.price(q.fuel));
        }
    }

    SearchResult { stations, averages, issues }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(id: &str, lat: f64, lng: f64, source: Source, prices: Vec<(FuelKind, f64)>) -> Station {
        Station { id: id.into(), lat, lng, source, prices, ..Default::default() }
    }

    #[test]
    fn merge_drops_osm_duplicates_and_far_stations() {
        let priced = vec![st("tk1", 51.0500, 13.7400, Source::Tankerkoenig, vec![(FuelKind::E10, 1.7)])];
        let osm = vec![
            st("osm-dup", 51.0505, 13.7402, Source::Osm, vec![]),
            st("osm-cz", 51.0600, 13.7500, Source::Osm, vec![]),
            st("osm-cz-2", 51.0601, 13.7501, Source::Osm, vec![]),
            st("osm-far", 51.5, 14.5, Source::Osm, vec![]),
        ];
        let ids: Vec<_> = merge(priced, osm, (51.05, 13.74), 5.0).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["tk1", "osm-cz", "osm-cz-2"]);
    }

    #[test]
    fn unknown_countries_take_center_country() {
        let mut list = vec![
            st("a", 50.77, 14.2, Source::Osm, vec![]),
            Station { country: Some("DE"), ..st("b", 50.8, 14.1, Source::Osm, vec![]) },
            st("c", 40.0, 0.0, Source::Osm, vec![]),
        ];
        fill_countries(&mut list, Some("CZ"));
        assert_eq!(list.iter().map(|s| s.country).collect::<Vec<_>>(), [Some("CZ"), Some("DE"), None]);
    }

    #[test]
    fn sorting() {
        let mut list = vec![
            Station { dist_km: 1.0, ..st("near-none", 0.0, 0.0, Source::Osm, vec![]) },
            Station { dist_km: 3.0, ..st("far-cheap", 0.0, 0.0, Source::Tankerkoenig, vec![(FuelKind::E10, 1.6)]) },
            Station { dist_km: 2.0, ..st("mid-pricey", 0.0, 0.0, Source::Tankerkoenig, vec![(FuelKind::E10, 1.8)]) },
        ];
        sort_stations(&mut list, FuelKind::E10, true);
        assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["far-cheap", "mid-pricey", "near-none"]);
        sort_stations(&mut list, FuelKind::E10, false);
        assert_eq!(list[0].id, "near-none");
    }

    #[test]
    fn labels() {
        let s = Station { name: "Aral Tankstelle".into(), brand: "Aral".into(), street: "Hauptstr.".into(), ..Default::default() };
        assert_eq!(s.label(), "Aral Hauptstr.");
        let o = Station { name: "Benzina, Děčín".into(), ..Default::default() };
        assert_eq!(o.label(), "Benzina, Děčín");
    }
}

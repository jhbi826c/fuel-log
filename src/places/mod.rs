pub mod geo;
mod sources;

pub use sources::{geocode, reverse_geocode};

use crate::model::usable_fuels;

/// A fuel station from OpenStreetMap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub street: String,
    pub address: String,
    pub lat: f64,
    pub lng: f64,
    pub dist_km: f64,
    /// Fuel types tagged in OpenStreetMap; empty when unknown.
    pub fuels: Vec<&'static str>,
    /// Whether the station has any `fuel:*=yes` tag.
    pub fuel_tagged: bool,
    /// Former brand when the OpenStreetMap entry still uses an outdated name.
    pub renamed_from: Option<&'static str>,
    /// Name you used for this station in an earlier fill-up.
    pub own_name: Option<String>,
}

/// Whether `s` starts with the word `word` (case-insensitive).
fn starts_with_word(s: &str, word: &str) -> bool {
    s.get(..word.len()).is_some_and(|p| p.eq_ignore_ascii_case(word))
        && s[word.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
}

impl Station {
    /// Name used when recording a fill-up here.
    pub fn label(&self) -> String {
        if let Some(own) = &self.own_name {
            own.clone()
        } else if !self.brand.is_empty() && !self.street.is_empty() {
            format!("{} {}", self.brand, self.street)
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.brand.clone()
        }
    }

    pub fn title(&self) -> String {
        match &self.own_name {
            Some(own) => own.clone(),
            None if self.name.is_empty() => self.brand.clone(),
            None => self.name.clone(),
        }
    }

    /// Shows stations of renamed brands under their current name.
    pub fn apply_rebrands(mut self) -> Self {
        for (old, new) in REBRANDS {
            let in_name = starts_with_word(&self.name, old);
            if self.brand.eq_ignore_ascii_case(old) || in_name {
                if in_name {
                    self.name = format!("{new}{}", &self.name[old.len()..]);
                }
                self.brand = new.to_string();
                self.renamed_from = Some(old);
            }
        }
        self
    }

    /// False only when the station's fuel tags rule out every fuel usable with `wanted`.
    pub fn may_offer(&self, wanted: &[String]) -> bool {
        let usable = usable_fuels(wanted);
        usable.is_empty() || !self.fuel_tagged || self.fuels.iter().any(|f| usable.contains(f))
    }
}

/// A station from your own records: position, name, address and date.
pub struct Known<'a> {
    pub lat: f64,
    pub lng: f64,
    pub name: &'a str,
    pub address: &'a str,
    pub date: &'a str,
}

/// Distance within which a recorded fill-up counts as the same station.
pub const SAME_STATION_KM: f64 = 0.15;

/// Uses the name and address from your latest fill-up at each station instead of OpenStreetMap's.
pub fn apply_own_names(list: &mut [Station], known: &[Known]) {
    for st in list.iter_mut() {
        let latest = known
            .iter()
            .filter(|k| !k.name.trim().is_empty() && geo::distance_km((k.lat, k.lng), (st.lat, st.lng)) < SAME_STATION_KM)
            .max_by(|a, b| a.date.cmp(b.date));
        if let Some(k) = latest {
            st.own_name = Some(k.name.trim().to_string());
            if !k.address.trim().is_empty() {
                st.address = k.address.trim().to_string();
            }
        }
    }
}

/// Brands whose whole network was renamed after many OpenStreetMap entries were made: (former, current).
const REBRANDS: [(&str, &str); 4] =
    [("Benzina", "Orlen"), ("Total", "TotalEnergies"), ("Statoil", "Circle K"), ("DEA", "Shell")];

/// Search radii tried in turn until enough stations are found.
const NEARBY_STEPS_KM: [f64; 3] = [2.0, 5.0, 10.0];
const NEARBY_MIN: usize = 3;

async fn within(origin: (f64, f64), radius_km: f64) -> Result<Vec<Station>, String> {
    let mut list: Vec<Station> = sources::osm_stations(origin, radius_km)
        .await?
        .into_iter()
        .filter(|s| s.dist_km <= radius_km)
        .collect();
    list.sort_by(|a, b| a.dist_km.total_cmp(&b.dist_km));
    Ok(list)
}

/// Nearest stations that may offer `wanted` fuels, widening the radius when few are found; returns the radius used.
pub async fn nearby(origin: (f64, f64), limit: usize, wanted: &[String]) -> Result<(f64, Vec<Station>), String> {
    let mut found = (NEARBY_STEPS_KM[0], Vec::new());
    for radius in NEARBY_STEPS_KM {
        let mut list = within(origin, radius).await?;
        list.retain(|s| s.may_offer(wanted));
        found = (radius, list);
        if found.1.len() >= NEARBY_MIN {
            break;
        }
    }
    found.1.truncate(limit);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        let s = Station { name: "Aral Tankstelle".into(), brand: "Aral".into(), street: "Hauptstr.".into(), ..Default::default() };
        assert_eq!(s.label(), "Aral Hauptstr.");
        let o = Station { name: "Benzina, Děčín".into(), ..Default::default() };
        assert_eq!(o.label(), "Benzina, Děčín");
        assert_eq!(Station { brand: "OMV".into(), ..Default::default() }.title(), "OMV");
    }

    #[test]
    fn rebrands() {
        let s = Station {
            name: "Benzina, Ústí nad Labem, Předmostí".into(),
            brand: "Benzina".into(),
            street: "Předmostí".into(),
            ..Default::default()
        }
        .apply_rebrands();
        assert_eq!(s.title(), "Orlen, Ústí nad Labem, Předmostí");
        assert_eq!(s.label(), "Orlen Předmostí");
        assert_eq!(s.renamed_from, Some("Benzina"));
        let named_only = Station { name: "BENZINA Plus".into(), ..Default::default() }.apply_rebrands();
        assert_eq!(named_only.title(), "Orlen Plus");
        let other = Station { name: "Shell".into(), brand: "Shell".into(), ..Default::default() }.apply_rebrands();
        assert_eq!((other.title(), other.renamed_from), ("Shell".into(), None));
    }

    #[test]
    fn rebrands_match_whole_words_only() {
        let rb = |name: &str, brand: &str| Station { name: name.into(), brand: brand.into(), ..Default::default() }.apply_rebrands();
        assert_eq!(rb("Total", "Total").title(), "TotalEnergies");
        assert_eq!(rb("TotalEnergies Berlin", "TotalEnergies").renamed_from, None);
        assert_eq!(rb("TotalEnergies Berlin", "TotalEnergies").title(), "TotalEnergies Berlin");
        assert_eq!(rb("Statoil Oslo", "").title(), "Circle K Oslo");
        assert_eq!(rb("DEAL Tankstelle", "").renamed_from, None);
        assert_eq!(rb("Agip", "Agip").title(), "Agip");
    }

    #[test]
    fn own_names_win() {
        let mut list = vec![
            Station { name: "Lotos".into(), lat: 52.0, lng: 21.0, address: "OSM street".into(), ..Default::default() },
            Station { name: "Far away".into(), lat: 52.1, lng: 21.0, ..Default::default() },
        ];
        let known = [
            Known { lat: 52.0005, lng: 21.0, name: "Orlen Warszawa", address: "ul. Prosta 1", date: "2026-01-01T10:00" },
            Known { lat: 52.0, lng: 21.0005, name: "Orlen Prosta", address: "", date: "2026-03-01T10:00" },
        ];
        apply_own_names(&mut list, &known);
        assert_eq!(list[0].title(), "Orlen Prosta");
        assert_eq!(list[0].label(), "Orlen Prosta");
        assert_eq!(list[0].address, "OSM street");
        assert_eq!(list[1].own_name, None);
    }

    #[test]
    fn fuel_filter() {
        let want = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let diesel_only = Station { fuels: vec!["Diesel"], fuel_tagged: true, ..Default::default() };
        let hydrogen = Station { fuel_tagged: true, ..Default::default() };
        let untagged = Station::default();
        assert!(diesel_only.may_offer(&want(&["Diesel"])));
        assert!(!diesel_only.may_offer(&want(&["Super E10", "Super E5"])));
        let czech = Station { fuels: vec!["Super E5", "Super Plus", "Diesel"], fuel_tagged: true, ..Default::default() };
        assert!(czech.may_offer(&want(&["Super E10"])));
        assert!(!czech.may_offer(&want(&["LPG"])));
        assert!(!hydrogen.may_offer(&want(&["Diesel"])));
        assert!(untagged.may_offer(&want(&["Diesel"])));
        assert!(diesel_only.may_offer(&[]));
        assert!(diesel_only.may_offer(&want(&["AdBlue"])));
    }
}

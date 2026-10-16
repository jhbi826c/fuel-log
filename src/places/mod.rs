pub mod geo;
mod sources;

pub use sources::reverse_geocode;

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
}

impl Station {
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

/// Fuel stations within `radius_km`, nearest first.
pub async fn nearby(origin: (f64, f64), radius_km: f64, limit: usize) -> Result<Vec<Station>, String> {
    let mut list: Vec<Station> = sources::osm_stations(origin, radius_km)
        .await?
        .into_iter()
        .filter(|s| s.dist_km <= radius_km)
        .collect();
    list.sort_by(|a, b| a.dist_km.total_cmp(&b.dist_km));
    list.truncate(limit);
    Ok(list)
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
}

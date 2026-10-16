/// Distance between two coordinates in kilometers.
pub fn distance_km((lat1, lng1): (f64, f64), (lat2, lng2): (f64, f64)) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        let d = distance_km((51.0504, 13.7373), (50.7726, 14.2128));
        assert!((d - 45.0).abs() < 2.0, "{d}");
    }
}

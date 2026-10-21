use std::f64::consts::PI;

/// Ground resolution at zoom 0 on the equator, meters per pixel of a 256 px tile.
const M_PER_PX_Z0: f64 = 156_543.033_92;
const MAX_LAT: f64 = 85.051_128_78;

/// Distance between two coordinates in kilometers.
pub fn distance_km((lat1, lng1): (f64, f64), (lat2, lng2): (f64, f64)) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// Web Mercator position, both axes in 0..1.
pub fn project((lat, lng): (f64, f64)) -> (f64, f64) {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    ((lng + 180.0) / 360.0, (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0)
}

pub fn unproject((x, y): (f64, f64)) -> (f64, f64) {
    ((PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees(), x * 360.0 - 180.0)
}

pub fn meters_per_px(lat: f64, zoom: f64) -> f64 {
    M_PER_PX_Z0 * lat.to_radians().cos() / 2f64.powf(zoom)
}

/// Zoom at which `km` spans `px` pixels around latitude `lat`.
pub fn zoom_for_km(km: f64, px: f64, lat: f64) -> f64 {
    (M_PER_PX_Z0 * lat.to_radians().cos() * px / (km * 1000.0)).log2()
}

/// A latitude/longitude box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl Bounds {
    pub fn center(&self) -> (f64, f64) {
        let (x0, y0) = project((self.north, self.west));
        let (x1, y1) = project((self.south, self.east));
        unproject(((x0 + x1) / 2.0, (y0 + y1) / 2.0))
    }

    pub fn clamp(&self, (lat, lng): (f64, f64)) -> (f64, f64) {
        (lat.clamp(self.south, self.north), lng.clamp(self.west, self.east))
    }

    /// Largest zoom showing the whole box in a `w`×`h` px view of 256 px tiles.
    pub fn fit_zoom(&self, w: f64, h: f64) -> f64 {
        let (x0, y0) = project((self.north, self.west));
        let (x1, y1) = project((self.south, self.east));
        (w / (256.0 * (x1 - x0))).log2().min((h / (256.0 * (y1 - y0))).log2())
    }
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
    fn mercator() {
        let (x, y) = project((51.0504, 13.7373));
        let (lat, lng) = unproject((x, y));
        assert!((lat - 51.0504).abs() < 1e-9 && (lng - 13.7373).abs() < 1e-9);
        assert_eq!(project((0.0, 0.0)), (0.5, 0.5));
        assert!((meters_per_px(0.0, 0.0) - M_PER_PX_Z0).abs() < 1e-6);
        let z = zoom_for_km(10.0, 300.0, 51.0);
        assert!((10_000.0 / meters_per_px(51.0, z) - 300.0).abs() < 1e-6);
    }

    #[test]
    fn bounds() {
        let b = Bounds { south: 47.2, west: 5.8, north: 55.1, east: 15.1 };
        assert_eq!(b.clamp((50.0, 20.0)), (50.0, 15.1));
        let (lat, lng) = b.center();
        assert!((lng - 10.45).abs() < 1e-9 && lat > 51.15 && lat < 51.4);
        let z = b.fit_zoom(360.0, 400.0);
        assert!(z > 5.0 && z < 6.0, "{z}");
    }
}

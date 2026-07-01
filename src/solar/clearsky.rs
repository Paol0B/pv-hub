use crate::solar::position::SolarPosition;

/// Haurwitz clear-sky GHI (W/m^2); 0 when the sun is at/below the horizon.
pub fn haurwitz_ghi(pos: &SolarPosition) -> f64 {
    let cos_zen = pos.zenith_deg.to_radians().cos();
    if cos_zen <= 0.0 {
        0.0
    } else {
        (1098.0 * cos_zen * (-0.059 / cos_zen).exp()).max(0.0)
    }
}

/// Clearness index kt = measured GHI / clear-sky GHI. None when clear-sky ~0.
pub fn clearness_index(ghi: f64, clearsky_ghi: f64) -> Option<f64> {
    if clearsky_ghi < 1.0 {
        None
    } else {
        Some((ghi / clearsky_ghi).clamp(0.0, 1.5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearsky_zero_when_sun_down() {
        let pos = SolarPosition { elevation_deg: -1.0, azimuth_deg: 0.0, zenith_deg: 91.0 };
        assert_eq!(haurwitz_ghi(&pos), 0.0);
    }

    #[test]
    fn clearsky_high_at_noon() {
        let pos = SolarPosition { elevation_deg: 85.0, azimuth_deg: 180.0, zenith_deg: 5.0 };
        let v = haurwitz_ghi(&pos);
        assert!(v > 900.0 && v < 1100.0, "got {v}");
    }

    #[test]
    fn kt_is_ratio() {
        let kt = clearness_index(800.0, 1000.0).unwrap();
        assert!((kt - 0.8).abs() < 1e-9);
        assert!(clearness_index(500.0, 0.0).is_none());
    }
}

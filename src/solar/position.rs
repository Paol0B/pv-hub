use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy)]
pub struct SolarPosition {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    pub zenith_deg: f64,
}

/// Angle of incidence between the sun and a tilted plane, in degrees.
/// tilt_deg = surface tilt from horizontal; surface_azimuth_deg from North (180 = South).
pub fn angle_of_incidence(pos: &SolarPosition, tilt_deg: f64, surface_azimuth_deg: f64) -> f64 {
    let z = pos.zenith_deg.to_radians();
    let beta = tilt_deg.to_radians();
    let gamma = (pos.azimuth_deg - surface_azimuth_deg).to_radians();
    let cos_aoi = z.cos() * beta.cos() + z.sin() * beta.sin() * gamma.cos();
    cos_aoi.clamp(-1.0, 1.0).acos().to_degrees()
}

/// Kasten-Young relative air mass; None when the sun is below the horizon.
pub fn air_mass(pos: &SolarPosition) -> Option<f64> {
    if pos.elevation_deg <= 0.0 {
        return None;
    }
    let z = pos.zenith_deg;
    Some(1.0 / (z.to_radians().cos() + 0.50572 * (96.07995 - z).powf(-1.6364)))
}

/// Extraterrestrial normal irradiance (W/m^2) for day-of-year.
pub fn extraterrestrial_normal(doy: u32) -> f64 {
    1361.0 * (1.0 + 0.033 * (2.0 * std::f64::consts::PI * doy as f64 / 365.0).cos())
}

/// Extraterrestrial horizontal irradiance (W/m^2); 0 when sun is down.
pub fn extraterrestrial_horizontal(pos: &SolarPosition, doy: u32) -> f64 {
    let coszen = pos.zenith_deg.to_radians().cos();
    if coszen <= 0.0 {
        0.0
    } else {
        extraterrestrial_normal(doy) * coszen
    }
}

/// Compute solar position via the PSA algorithm (spa crate).
pub fn solar_position(utc: DateTime<Utc>, lat: f64, lon: f64) -> SolarPosition {
    match spa::solar_position::<spa::StdFloatOps>(utc, lat, lon) {
        Ok(p) => SolarPosition {
            zenith_deg: p.zenith_angle,
            azimuth_deg: p.azimuth,
            elevation_deg: 90.0 - p.zenith_angle,
        },
        Err(_) => SolarPosition {
            zenith_deg: 90.0,
            azimuth_deg: 0.0,
            elevation_deg: 0.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} != {b} (±{eps})");
    }

    #[test]
    fn aoi_zero_when_sun_normal_to_panel() {
        let pos = SolarPosition { elevation_deg: 60.0, azimuth_deg: 180.0, zenith_deg: 30.0 };
        approx(angle_of_incidence(&pos, 30.0, 180.0), 0.0, 1e-6);
    }

    #[test]
    fn aoi_equals_zenith_for_horizontal_panel() {
        let pos = SolarPosition { elevation_deg: 50.0, azimuth_deg: 120.0, zenith_deg: 40.0 };
        approx(angle_of_incidence(&pos, 0.0, 180.0), 40.0, 1e-6);
    }

    #[test]
    fn air_mass_is_one_at_zenith() {
        let pos = SolarPosition { elevation_deg: 90.0, azimuth_deg: 180.0, zenith_deg: 0.0 };
        approx(air_mass(&pos).unwrap(), 1.0, 1e-2);
    }

    #[test]
    fn air_mass_none_below_horizon() {
        let pos = SolarPosition { elevation_deg: -5.0, azimuth_deg: 0.0, zenith_deg: 95.0 };
        assert!(air_mass(&pos).is_none());
    }

    #[test]
    fn extraterrestrial_within_solar_constant_band() {
        let v = extraterrestrial_normal(172);
        assert!(v > 1300.0 && v < 1420.0, "got {v}");
    }

    #[test]
    fn milan_summer_noon_is_high() {
        use chrono::TimeZone;
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 11, 30, 0).unwrap();
        let pos = solar_position(t, 45.4642, 9.19);
        assert!(pos.elevation_deg > 60.0 && pos.elevation_deg < 72.0, "elev {}", pos.elevation_deg);
        assert!(pos.azimuth_deg > 150.0 && pos.azimuth_deg < 210.0, "az {}", pos.azimuth_deg);
    }
}

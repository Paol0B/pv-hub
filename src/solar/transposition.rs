use crate::solar::position::{angle_of_incidence, extraterrestrial_normal, SolarPosition};

/// Plane-of-array global irradiance (W/m^2) via the Hay-Davies anisotropic model.
///
/// ghi/dni/dhi in W/m^2; tilt_deg from horizontal; azimuth_deg of the panel from North.
#[allow(clippy::too_many_arguments)]
pub fn poa_hay_davies(
    pos: &SolarPosition,
    ghi: f64,
    dni: f64,
    dhi: f64,
    tilt_deg: f64,
    surface_azimuth_deg: f64,
    albedo: f64,
    doy: u32,
) -> f64 {
    let beta = tilt_deg.to_radians();
    let zen = pos.zenith_deg.to_radians();
    let aoi = angle_of_incidence(pos, tilt_deg, surface_azimuth_deg).to_radians();

    // Beam on tilted plane.
    let cos_aoi = aoi.cos().max(0.0);
    let beam = dni * cos_aoi;

    // Ratio of beam on tilt vs horizontal (guard low sun, ~ above 5deg).
    let cos_zen = zen.cos();
    let rb = if cos_zen > 0.087 { cos_aoi / cos_zen } else { 0.0 };

    // Anisotropy index (clamped).
    let i0n = extraterrestrial_normal(doy);
    let ai = if i0n > 0.0 { (dni / i0n).clamp(0.0, 1.0) } else { 0.0 };

    // Hay-Davies sky diffuse.
    let iso = (1.0 + beta.cos()) / 2.0;
    let diffuse = dhi * (ai * rb + (1.0 - ai) * iso);

    // Ground-reflected.
    let ground = ghi * albedo * (1.0 - beta.cos()) / 2.0;

    (beam + diffuse + ground).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solar::position::SolarPosition;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} != {b} (±{eps})");
    }

    #[test]
    fn horizontal_panel_equals_ghi() {
        let pos = SolarPosition { elevation_deg: 50.0, azimuth_deg: 180.0, zenith_deg: 40.0 };
        let dni = 700.0;
        let dhi = 120.0;
        let ghi = dni * pos.zenith_deg.to_radians().cos() + dhi;
        let poa = poa_hay_davies(&pos, ghi, dni, dhi, 0.0, 180.0, 0.2, 172);
        approx(poa, ghi, 1.0);
    }

    #[test]
    fn tilt_toward_sun_beats_horizontal() {
        let pos = SolarPosition { elevation_deg: 30.0, azimuth_deg: 180.0, zenith_deg: 60.0 };
        let (dni, dhi) = (750.0, 90.0);
        let ghi = dni * pos.zenith_deg.to_radians().cos() + dhi;
        let flat = poa_hay_davies(&pos, ghi, dni, dhi, 0.0, 180.0, 0.2, 172);
        let tilted = poa_hay_davies(&pos, ghi, dni, dhi, 45.0, 180.0, 0.2, 172);
        assert!(tilted > flat, "tilted {tilted} !> flat {flat}");
    }

    #[test]
    fn never_negative() {
        let pos = SolarPosition { elevation_deg: 2.0, azimuth_deg: 90.0, zenith_deg: 88.0 };
        let poa = poa_hay_davies(&pos, 10.0, 5.0, 8.0, 30.0, 180.0, 0.2, 1);
        assert!(poa >= 0.0);
    }
}

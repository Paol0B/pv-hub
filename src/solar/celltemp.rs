/// Faiman module temperature model (°C). u0≈25, u1≈6.84.
pub fn faiman(ambient_c: f64, poa: f64, wind_ms: f64, u0: f64, u1: f64) -> f64 {
    ambient_c + poa / (u0 + u1 * wind_ms.max(0.0))
}

/// NOCT module temperature model (°C).
pub fn noct(ambient_c: f64, poa: f64, noct_c: f64) -> f64 {
    ambient_c + (noct_c - 20.0) / 800.0 * poa
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} != {b}");
    }

    #[test]
    fn faiman_zero_irradiance_equals_ambient() {
        approx(faiman(25.0, 0.0, 1.0, 25.0, 6.84), 25.0, 1e-9);
    }

    #[test]
    fn faiman_wind_lowers_temperature() {
        let calm = faiman(25.0, 900.0, 0.0, 25.0, 6.84);
        let windy = faiman(25.0, 900.0, 5.0, 25.0, 6.84);
        assert!(windy < calm);
        assert!(calm > 25.0);
    }

    #[test]
    fn noct_reference_point() {
        approx(noct(20.0, 800.0, 45.0), 45.0, 1e-9);
    }
}

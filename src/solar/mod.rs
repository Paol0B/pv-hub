pub mod celltemp;
pub mod clearsky;
pub mod position;
pub mod transposition;

use crate::config::{CellTempModel, Config};
use crate::model::{Metric, WeatherInputs};
use chrono::{DateTime, Datelike, Utc};

use self::celltemp::{faiman, noct};
use self::clearsky::{clearness_index, haurwitz_ghi};
use self::position::{air_mass, angle_of_incidence, extraterrestrial_horizontal, solar_position};
use self::transposition::poa_hay_davies;

pub struct SolarEngine;

impl SolarEngine {
    /// Compute all solar-derived metrics for `now`, using current weather from `w`.
    /// Position-only metrics are always produced; irradiance-dependent metrics are
    /// only produced when the needed weather inputs are present.
    pub fn compute(cfg: &Config, now: DateTime<Utc>, w: &WeatherInputs) -> Vec<(Metric, f64)> {
        let mut out = Vec::new();
        let pos = solar_position(now, cfg.latitude, cfg.longitude);
        let doy = now.ordinal();

        out.push((Metric::SunElevation, pos.elevation_deg));
        out.push((Metric::SunAzimuth, pos.azimuth_deg));
        out.push((Metric::SunZenith, pos.zenith_deg));
        out.push((Metric::Aoi, angle_of_incidence(&pos, cfg.tilt_deg, cfg.azimuth_deg)));
        out.push((Metric::IsDaytime, if pos.elevation_deg > 0.0 { 1.0 } else { 0.0 }));
        out.push((Metric::Extraterrestrial, extraterrestrial_horizontal(&pos, doy)));
        if let Some(am) = air_mass(&pos) {
            out.push((Metric::AirMass, am));
        }

        // POA (local) needs GHI/DNI/DHI.
        if let (Some(ghi), Some(dni), Some(dhi)) = (w.ghi, w.dni, w.dhi) {
            let poa = poa_hay_davies(&pos, ghi, dni, dhi, cfg.tilt_deg, cfg.azimuth_deg, cfg.albedo, doy);
            out.push((Metric::PoaLocal, poa));

            if let Some(pp) = w.poa_provider {
                if pp.abs() > 1.0 {
                    out.push((Metric::PoaDeltaPct, (poa - pp) / pp * 100.0));
                }
            }

            // Cell temperature needs ambient + POA (+ wind for Faiman).
            if let Some(tamb) = w.ambient_temp {
                let tcell = match cfg.celltemp {
                    CellTempModel::Faiman => {
                        faiman(tamb, poa, w.wind_speed.unwrap_or(0.0), cfg.faiman_u0, cfg.faiman_u1)
                    }
                    CellTempModel::Noct => noct(tamb, poa, cfg.noct),
                };
                out.push((Metric::ModuleTemp, tcell));
            }
        }

        // Clear-sky + kt need GHI.
        let cs = haurwitz_ghi(&pos);
        out.push((Metric::ClearskyGhi, cs));
        if let Some(ghi) = w.ghi {
            if let Some(kt) = clearness_index(ghi, cs) {
                out.push((Metric::ClearskyIndex, kt));
            }
        }
        out
    }

    /// Static site-echo metrics that never change at runtime.
    pub fn site_metrics(cfg: &Config) -> Vec<(Metric, f64)> {
        vec![
            (Metric::Latitude, cfg.latitude),
            (Metric::Longitude, cfg.longitude),
            (Metric::Tilt, cfg.tilt_deg),
            (Metric::Azimuth, cfg.azimuth_deg),
            (Metric::Albedo, cfg.albedo),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::WeatherInputs;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[test]
    fn produces_position_metrics_without_weather() {
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 11, 30, 0).unwrap();
        let out = SolarEngine::compute(&cfg(), t, &WeatherInputs::default());
        let map: HashMap<_, _> = out.into_iter().collect();
        assert!(map.contains_key(&Metric::SunElevation));
        assert!(map.contains_key(&Metric::Aoi));
        assert!(!map.contains_key(&Metric::PoaLocal));
    }

    #[test]
    fn produces_poa_and_module_temp_with_weather() {
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 11, 30, 0).unwrap();
        let w = WeatherInputs {
            ghi: Some(800.0),
            dni: Some(700.0),
            dhi: Some(140.0),
            ambient_temp: Some(27.0),
            wind_speed: Some(3.0),
            poa_provider: Some(910.0),
        };
        let map: HashMap<_, _> = SolarEngine::compute(&cfg(), t, &w).into_iter().collect();
        assert!(*map.get(&Metric::PoaLocal).unwrap() > 0.0);
        assert!(*map.get(&Metric::ModuleTemp).unwrap() > 27.0);
        assert!(map.contains_key(&Metric::PoaDeltaPct));
        assert!(map.contains_key(&Metric::ClearskyIndex));
    }
}

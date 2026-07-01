use crate::model::Metric;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegKind {
    F32,
    U32,
}

#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub metric: Metric,
    pub id: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub category: &'static str,
    pub register: u16,
    pub kind: RegKind,
}

/// Single source of truth for every metric: label, unit, and Modbus placement.
pub fn catalog() -> &'static [MetricDef] {
    use Metric::*;
    use RegKind::*;
    &[
        // A — Irradiance
        MetricDef { metric: Ghi, id: "ghi", label: "GHI", unit: "W/m2", category: "irradiance", register: 0, kind: F32 },
        MetricDef { metric: Dni, id: "dni", label: "DNI", unit: "W/m2", category: "irradiance", register: 2, kind: F32 },
        MetricDef { metric: Dhi, id: "dhi", label: "DHI", unit: "W/m2", category: "irradiance", register: 4, kind: F32 },
        MetricDef { metric: PoaLocal, id: "poa_local", label: "POA local", unit: "W/m2", category: "irradiance", register: 6, kind: F32 },
        MetricDef { metric: PoaProvider, id: "poa_provider", label: "POA provider", unit: "W/m2", category: "irradiance", register: 8, kind: F32 },
        MetricDef { metric: PoaDeltaPct, id: "poa_delta_pct", label: "POA delta", unit: "%", category: "irradiance", register: 10, kind: F32 },
        MetricDef { metric: ClearskyGhi, id: "clearsky_ghi", label: "Clear-sky GHI", unit: "W/m2", category: "irradiance", register: 12, kind: F32 },
        MetricDef { metric: ClearskyIndex, id: "clearsky_index", label: "Clearness kt", unit: "", category: "irradiance", register: 14, kind: F32 },
        MetricDef { metric: Extraterrestrial, id: "extraterrestrial", label: "Extraterrestrial", unit: "W/m2", category: "irradiance", register: 16, kind: F32 },
        // B — Solar geometry
        MetricDef { metric: SunElevation, id: "sun_elevation", label: "Sun elevation", unit: "deg", category: "geometry", register: 30, kind: F32 },
        MetricDef { metric: SunAzimuth, id: "sun_azimuth", label: "Sun azimuth", unit: "deg", category: "geometry", register: 32, kind: F32 },
        MetricDef { metric: SunZenith, id: "sun_zenith", label: "Sun zenith", unit: "deg", category: "geometry", register: 34, kind: F32 },
        MetricDef { metric: Aoi, id: "aoi", label: "AOI", unit: "deg", category: "geometry", register: 36, kind: F32 },
        MetricDef { metric: AirMass, id: "air_mass", label: "Air mass", unit: "", category: "geometry", register: 38, kind: F32 },
        MetricDef { metric: IsDaytime, id: "is_daytime", label: "Daytime", unit: "", category: "geometry", register: 40, kind: F32 },
        // C — Temperature
        MetricDef { metric: AmbientTemp, id: "ambient_temp", label: "Ambient temp", unit: "degC", category: "temperature", register: 50, kind: F32 },
        MetricDef { metric: ModuleTemp, id: "module_temp", label: "Module temp", unit: "degC", category: "temperature", register: 52, kind: F32 },
        // D — Meteo
        MetricDef { metric: WindSpeed, id: "wind_speed", label: "Wind speed", unit: "m/s", category: "meteo", register: 60, kind: F32 },
        MetricDef { metric: WindDirection, id: "wind_direction", label: "Wind dir", unit: "deg", category: "meteo", register: 62, kind: F32 },
        MetricDef { metric: RelHumidity, id: "rel_humidity", label: "Humidity", unit: "%", category: "meteo", register: 64, kind: F32 },
        MetricDef { metric: CloudCover, id: "cloud_cover", label: "Cloud cover", unit: "%", category: "meteo", register: 66, kind: F32 },
        MetricDef { metric: Precipitation, id: "precipitation", label: "Precipitation", unit: "mm", category: "meteo", register: 68, kind: F32 },
        MetricDef { metric: SurfacePressure, id: "surface_pressure", label: "Pressure", unit: "hPa", category: "meteo", register: 70, kind: F32 },
        // E — Site echo
        MetricDef { metric: Latitude, id: "latitude", label: "Latitude", unit: "deg", category: "site", register: 90, kind: F32 },
        MetricDef { metric: Longitude, id: "longitude", label: "Longitude", unit: "deg", category: "site", register: 92, kind: F32 },
        MetricDef { metric: Tilt, id: "tilt", label: "Tilt", unit: "deg", category: "site", register: 94, kind: F32 },
        MetricDef { metric: Azimuth, id: "azimuth", label: "Azimuth", unit: "deg", category: "site", register: 96, kind: F32 },
        MetricDef { metric: Albedo, id: "albedo", label: "Albedo", unit: "", category: "site", register: 98, kind: F32 },
        // F — Health
        MetricDef { metric: DataAge, id: "data_age", label: "Data age", unit: "s", category: "health", register: 110, kind: F32 },
        MetricDef { metric: LastUpdateEpoch, id: "last_update_epoch", label: "Last update", unit: "s", category: "health", register: 112, kind: U32 },
        MetricDef { metric: ProviderOk, id: "provider_ok", label: "Provider OK", unit: "", category: "health", register: 114, kind: F32 },
        MetricDef { metric: PollErrorsTotal, id: "poll_errors_total", label: "Poll errors", unit: "", category: "health", register: 116, kind: F32 },
    ]
}

pub fn def_for(metric: Metric) -> Option<&'static MetricDef> {
    catalog().iter().find(|d| d.metric == metric)
}

/// Highest register word used, +1 (bank size needed to hold everything).
pub fn bank_words() -> usize {
    catalog().iter().map(|d| d.register as usize + 2).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_register_collisions() {
        let mut used: HashSet<u16> = HashSet::new();
        for d in catalog() {
            for w in [d.register, d.register + 1] {
                assert!(used.insert(w), "register {w} used twice (metric {:?})", d.metric);
            }
        }
    }

    #[test]
    fn every_metric_variant_has_a_def() {
        for m in Metric::ALL {
            assert!(def_for(m).is_some(), "no MetricDef for {m:?}");
        }
        assert_eq!(catalog().len(), 32);
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for d in catalog() {
            assert!(seen.insert(d.id), "duplicate id {}", d.id);
        }
    }

    #[test]
    fn bank_is_large_enough() {
        assert_eq!(bank_words(), 118);
    }
}

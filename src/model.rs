use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Every quantity pv-hub tracks. Adding a metric here + a MetricDef in the
/// catalog is all that's needed for it to flow to every sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    // Irradiance
    Ghi,
    Dni,
    Dhi,
    PoaLocal,
    PoaProvider,
    PoaDeltaPct,
    ClearskyGhi,
    ClearskyIndex,
    Extraterrestrial,
    // Solar geometry
    SunElevation,
    SunAzimuth,
    SunZenith,
    Aoi,
    AirMass,
    IsDaytime,
    // Temperature
    AmbientTemp,
    ModuleTemp,
    // Meteo
    WindSpeed,
    WindDirection,
    RelHumidity,
    CloudCover,
    Precipitation,
    SurfacePressure,
    // Site echo
    Latitude,
    Longitude,
    Tilt,
    Azimuth,
    Albedo,
    // Health
    DataAge,
    LastUpdateEpoch,
    ProviderOk,
    PollErrorsTotal,
}

impl Metric {
    /// All variants, in catalog order. Must stay in sync with `catalog()`.
    pub const ALL: [Metric; 32] = [
        Metric::Ghi,
        Metric::Dni,
        Metric::Dhi,
        Metric::PoaLocal,
        Metric::PoaProvider,
        Metric::PoaDeltaPct,
        Metric::ClearskyGhi,
        Metric::ClearskyIndex,
        Metric::Extraterrestrial,
        Metric::SunElevation,
        Metric::SunAzimuth,
        Metric::SunZenith,
        Metric::Aoi,
        Metric::AirMass,
        Metric::IsDaytime,
        Metric::AmbientTemp,
        Metric::ModuleTemp,
        Metric::WindSpeed,
        Metric::WindDirection,
        Metric::RelHumidity,
        Metric::CloudCover,
        Metric::Precipitation,
        Metric::SurfacePressure,
        Metric::Latitude,
        Metric::Longitude,
        Metric::Tilt,
        Metric::Azimuth,
        Metric::Albedo,
        Metric::DataAge,
        Metric::LastUpdateEpoch,
        Metric::ProviderOk,
        Metric::PollErrorsTotal,
    ];
}

/// Weather inputs the solar engine reads from current state.
#[derive(Debug, Clone, Default)]
pub struct WeatherInputs {
    pub ghi: Option<f64>,
    pub dni: Option<f64>,
    pub dhi: Option<f64>,
    pub ambient_temp: Option<f64>,
    pub wind_speed: Option<f64>,
    pub poa_provider: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct SolarState {
    values: HashMap<Metric, f64>,
    pub last_weather_update: Option<DateTime<Utc>>,
    pub provider_ok: bool,
    pub poll_errors_total: u64,
}

impl SolarState {
    pub fn set(&mut self, metric: Metric, value: f64) {
        self.values.insert(metric, value);
    }

    pub fn raw(&self, metric: Metric) -> Option<f64> {
        self.values.get(&metric).copied()
    }

    /// Weather inputs for the solar engine.
    pub fn weather_inputs(&self) -> WeatherInputs {
        WeatherInputs {
            ghi: self.raw(Metric::Ghi),
            dni: self.raw(Metric::Dni),
            dhi: self.raw(Metric::Dhi),
            ambient_temp: self.raw(Metric::AmbientTemp),
            wind_speed: self.raw(Metric::WindSpeed),
            poa_provider: self.raw(Metric::PoaProvider),
        }
    }

    /// Value for a metric, computing health metrics on the fly.
    pub fn value(&self, metric: Metric, now: DateTime<Utc>) -> Option<f64> {
        match metric {
            Metric::DataAge => self.last_weather_update.map(|t| (now - t).num_seconds().max(0) as f64),
            Metric::LastUpdateEpoch => self.last_weather_update.map(|t| t.timestamp() as f64),
            Metric::ProviderOk => Some(if self.provider_ok { 1.0 } else { 0.0 }),
            Metric::PollErrorsTotal => Some(self.poll_errors_total as f64),
            other => self.raw(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn set_and_get_roundtrip() {
        let mut s = SolarState::default();
        s.set(Metric::Ghi, 812.0);
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(s.value(Metric::Ghi, now), Some(812.0));
        assert_eq!(s.value(Metric::Dni, now), None);
    }

    #[test]
    fn data_age_is_computed() {
        let mut s = SolarState::default();
        let t0 = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        s.last_weather_update = Some(t0);
        let now = t0 + chrono::Duration::seconds(42);
        assert_eq!(s.value(Metric::DataAge, now), Some(42.0));
        assert_eq!(s.value(Metric::LastUpdateEpoch, now), Some(1_700_000_000.0));
    }

    #[test]
    fn provider_ok_and_errors() {
        let mut s = SolarState::default();
        s.provider_ok = true;
        s.poll_errors_total = 3;
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(s.value(Metric::ProviderOk, now), Some(1.0));
        assert_eq!(s.value(Metric::PollErrorsTotal, now), Some(3.0));
    }

    #[test]
    fn all_has_expected_len() {
        assert_eq!(Metric::ALL.len(), 32);
    }
}

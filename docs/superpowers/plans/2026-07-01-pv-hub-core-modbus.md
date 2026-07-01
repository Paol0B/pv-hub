# pv-hub — Plan 1: Core service + Solar engine + Modbus TCP

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A headless Rust service that reads env config, fetches weather/solar data from Open-Meteo, computes PV-diagnostic quantities (solar position, POA irradiance, module temperature, clear-sky index), stores them in a central `SolarState`, and exposes everything over a read-only Modbus TCP slave — fully working and testable without the web UI (Plan 2) or container (Plan 3).

**Architecture:** Pluggable providers write `Sample`s into a shared `Hub` (`Arc<RwLock<SolarState>>` + broadcast). A single `Metric` enum + static `catalog` describe every quantity once (label, unit, Modbus register). Sinks derive from the catalog — Plan 1 ships the Modbus sink; MQTT/HTTP are future sinks. A scheduler runs a slow weather poll and a fast solar-math recompute.

**Tech Stack:** Rust (edition 2021), tokio, chrono, reqwest (rustls), serde/serde_json, spa (solar position), tracing. Modbus TCP framing is hand-rolled (read-only FC03/FC04) to stay dependency-light and fully testable. Tests: built-in `cargo test` + wiremock for the HTTP provider.

---

## File structure (Plan 1)

```
Cargo.toml                  crate manifest + deps
src/lib.rs                  module declarations + init_tracing() + run()
src/main.rs                 thin binary: calls pv_hub::run()
src/config.rs               Config + from_map/from_env + validation
src/model.rs                Metric enum, SolarState, WeatherInputs
src/catalog.rs              MetricDef list (register map), lookups, invariants
src/hub.rs                  Hub: Arc<RwLock<SolarState>> + broadcast + apply()
src/solar/mod.rs            SolarEngine::compute() orchestration + models enum
src/solar/position.rs       spa wrapper: SolarPosition + AOI + air mass + extraterrestrial + sun times
src/solar/transposition.rs  Hay-Davies POA
src/solar/celltemp.rs       Faiman / NOCT cell temperature
src/solar/clearsky.rs       Haurwitz clear-sky GHI + kt
src/providers/mod.rs        Provider trait + Sample
src/providers/openmeteo.rs  Open-Meteo URL build + response parse
src/sinks/mod.rs            sink module root
src/sinks/modbus/mod.rs     register-bank encoding + word order
src/sinks/modbus/frame.rs   pure MBAP/PDU request handler (FC03/FC04)
src/sinks/modbus/server.rs  tokio TCP accept loop
src/scheduler.rs            weather + solar periodic tasks
tests/fixtures/openmeteo.json  sample provider response (test fixture)
```

Every module is `pub mod`-declared in `src/lib.rs` as it is created.

---

## Conventions used in every task

- **Angles:** stored/exposed in **degrees**; trigonometry done in **radians** (`f64::to_radians` / `to_degrees`).
- **Azimuth convention:** measured **clockwise from North** (0=N, 90=E, 180=S, 270=W) for both sun and panel — matches the `spa` crate and `PVHUB_AZIMUTH_DEG=180` meaning South.
- **Float comparison in tests:** use an `approx(a, b, eps)` helper (defined per test module) — never `==` on floats.
- **Commit messages:** conventional style, **never** add a `Co-Authored-By` trailer.

---

### Task 1: Cargo scaffold + tracing + runnable skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "pv-hub"
version = "0.1.0"
edition = "2021"

[lib]
name = "pv_hub"
path = "src/lib.rs"

[[bin]]
name = "pv-hub"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync", "signal"] }
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
spa = "0.5"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
thiserror = "1"

[dev-dependencies]
wiremock = "0.6"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync", "io-util"] }
```

- [ ] **Step 2: Create `src/lib.rs`**

```rust
//! pv-hub — Solarimetro microservice core.

use anyhow::Result;

pub fn init_tracing(level: &str) {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));
    let _ = fmt().with_env_filter(filter).try_init();
}

/// Entry point used by the binary. Fleshed out in Task 14.
pub async fn run() -> Result<()> {
    init_tracing("info");
    tracing::info!("pv-hub {} starting", env!("CARGO_PKG_VERSION"));
    Ok(())
}
```

- [ ] **Step 3: Create `src/main.rs`**

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pv_hub::run().await
}
```

- [ ] **Step 4: Verify it builds and runs**

Run: `cargo run`
Expected: compiles; logs `pv-hub 0.1.0 starting` and exits 0.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/main.rs
git commit -m "chore: scaffold pv-hub crate with tracing"
```

---

### Task 2: Config from environment

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs` (add `pub mod config;`)

- [ ] **Step 1: Add module declaration to `src/lib.rs`**

Add near the top, after the doc comment:

```rust
pub mod config;
```

- [ ] **Step 2: Write the failing test**

Create `src/config.rs`:

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionModel { HayDavies, Perez }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTempModel { Faiman, Noct }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordOrder { Abcd, Cdab }

#[derive(Debug, Clone)]
pub struct Config {
    pub site_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: Option<f64>,
    pub tilt_deg: f64,
    pub azimuth_deg: f64,
    pub albedo: f64,
    pub transposition: TranspositionModel,
    pub celltemp: CellTempModel,
    pub faiman_u0: f64,
    pub faiman_u1: f64,
    pub noct: f64,
    pub poll_interval_s: u64,
    pub solar_interval_s: u64,
    pub provider: String,
    pub openmeteo_base_url: String,
    pub openmeteo_api_key: Option<String>,
    pub http_bind: String,
    pub http_port: u16,
    pub modbus_enable: bool,
    pub modbus_bind: String,
    pub modbus_port: u16,
    pub modbus_unit_id: u8,
    pub modbus_word_order: WordOrder,
    pub modbus_holding_mirror: bool,
    pub default_theme: String,
    pub log_level: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        m
    }

    #[test]
    fn parses_required_and_defaults() {
        let c = Config::from_map(&base()).unwrap();
        assert_eq!(c.latitude, 45.4642);
        assert_eq!(c.longitude, 9.19);
        assert_eq!(c.tilt_deg, 30.0);
        assert_eq!(c.azimuth_deg, 180.0);
        assert_eq!(c.albedo, 0.20);
        assert_eq!(c.modbus_port, 502);
        assert_eq!(c.modbus_word_order, WordOrder::Abcd);
        assert!(c.modbus_holding_mirror);
        assert_eq!(c.transposition, TranspositionModel::HayDavies);
        assert_eq!(c.celltemp, CellTempModel::Faiman);
    }

    #[test]
    fn missing_latitude_is_error() {
        let mut m = base();
        m.remove("PVHUB_LATITUDE");
        let err = Config::from_map(&m).unwrap_err();
        assert!(err.contains("PVHUB_LATITUDE"), "got: {err}");
    }

    #[test]
    fn overrides_are_applied() {
        let mut m = base();
        m.insert("PVHUB_TILT_DEG".into(), "15".into());
        m.insert("PVHUB_MODBUS_WORD_ORDER".into(), "cdab".into());
        m.insert("PVHUB_CELLTEMP".into(), "noct".into());
        let c = Config::from_map(&m).unwrap();
        assert_eq!(c.tilt_deg, 15.0);
        assert_eq!(c.modbus_word_order, WordOrder::Cdab);
        assert_eq!(c.celltemp, CellTempModel::Noct);
    }

    #[test]
    fn out_of_range_latitude_is_error() {
        let mut m = base();
        m.insert("PVHUB_LATITUDE".into(), "120".into());
        assert!(Config::from_map(&m).is_err());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test config::tests`
Expected: FAIL — `from_map` not found.

- [ ] **Step 4: Implement `from_map` / `from_env`**

Add to `src/config.rs` (above the `#[cfg(test)]` block):

```rust
impl Config {
    pub fn from_env() -> Result<Config, String> {
        let map: HashMap<String, String> = std::env::vars().collect();
        Config::from_map(&map)
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Config, String> {
        let req_f64 = |k: &str| -> Result<f64, String> {
            env.get(k)
                .ok_or_else(|| format!("missing required env {k}"))?
                .parse::<f64>()
                .map_err(|e| format!("{k}: {e}"))
        };
        let f64_or = |k: &str, d: f64| -> Result<f64, String> {
            match env.get(k) {
                Some(v) => v.parse::<f64>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u64_or = |k: &str, d: u64| -> Result<u64, String> {
            match env.get(k) {
                Some(v) => v.parse::<u64>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u16_or = |k: &str, d: u16| -> Result<u16, String> {
            match env.get(k) {
                Some(v) => v.parse::<u16>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u8_or = |k: &str, d: u8| -> Result<u8, String> {
            match env.get(k) {
                Some(v) => v.parse::<u8>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let bool_or = |k: &str, d: bool| -> bool {
            env.get(k).map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")).unwrap_or(d)
        };
        let str_or = |k: &str, d: &str| -> String { env.get(k).cloned().unwrap_or_else(|| d.to_string()) };

        let latitude = req_f64("PVHUB_LATITUDE")?;
        if !(-90.0..=90.0).contains(&latitude) {
            return Err("PVHUB_LATITUDE out of range [-90,90]".into());
        }
        let longitude = req_f64("PVHUB_LONGITUDE")?;
        if !(-180.0..=180.0).contains(&longitude) {
            return Err("PVHUB_LONGITUDE out of range [-180,180]".into());
        }

        let transposition = match str_or("PVHUB_TRANSPOSITION", "hay_davies").as_str() {
            "hay_davies" => TranspositionModel::HayDavies,
            "perez" => TranspositionModel::Perez,
            other => return Err(format!("PVHUB_TRANSPOSITION invalid: {other}")),
        };
        let celltemp = match str_or("PVHUB_CELLTEMP", "faiman").as_str() {
            "faiman" => CellTempModel::Faiman,
            "noct" => CellTempModel::Noct,
            other => return Err(format!("PVHUB_CELLTEMP invalid: {other}")),
        };
        let modbus_word_order = match str_or("PVHUB_MODBUS_WORD_ORDER", "abcd").as_str() {
            "abcd" => WordOrder::Abcd,
            "cdab" => WordOrder::Cdab,
            other => return Err(format!("PVHUB_MODBUS_WORD_ORDER invalid: {other}")),
        };

        Ok(Config {
            site_name: str_or("PVHUB_SITE_NAME", "pv-hub"),
            latitude,
            longitude,
            elevation_m: env.get("PVHUB_ELEVATION_M").map(|v| v.parse().unwrap_or(0.0)),
            tilt_deg: f64_or("PVHUB_TILT_DEG", 30.0)?,
            azimuth_deg: f64_or("PVHUB_AZIMUTH_DEG", 180.0)?,
            albedo: f64_or("PVHUB_ALBEDO", 0.20)?,
            transposition,
            celltemp,
            faiman_u0: f64_or("PVHUB_CELLTEMP_U0", 25.0)?,
            faiman_u1: f64_or("PVHUB_CELLTEMP_U1", 6.84)?,
            noct: f64_or("PVHUB_CELLTEMP_NOCT", 45.0)?,
            poll_interval_s: u64_or("PVHUB_POLL_INTERVAL_S", 600)?,
            solar_interval_s: u64_or("PVHUB_SOLAR_INTERVAL_S", 60)?,
            provider: str_or("PVHUB_PROVIDER", "openmeteo"),
            openmeteo_base_url: str_or("PVHUB_OPENMETEO_BASE_URL", "https://api.open-meteo.com/v1/forecast"),
            openmeteo_api_key: env.get("PVHUB_OPENMETEO_API_KEY").cloned(),
            http_bind: str_or("PVHUB_HTTP_BIND", "0.0.0.0"),
            http_port: u16_or("PVHUB_HTTP_PORT", 8080)?,
            modbus_enable: bool_or("PVHUB_MODBUS_ENABLE", true),
            modbus_bind: str_or("PVHUB_MODBUS_BIND", "0.0.0.0"),
            modbus_port: u16_or("PVHUB_MODBUS_PORT", 502)?,
            modbus_unit_id: u8_or("PVHUB_MODBUS_UNIT_ID", 1)?,
            modbus_word_order,
            modbus_holding_mirror: bool_or("PVHUB_MODBUS_HOLDING_MIRROR", true),
            default_theme: str_or("PVHUB_DEFAULT_THEME", "auto"),
            log_level: str_or("PVHUB_LOG_LEVEL", "info"),
        })
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test config::tests`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/lib.rs
git commit -m "feat: env-driven Config with validation"
```

---

### Task 3: Metric enum + SolarState

**Files:**
- Create: `src/model.rs`
- Modify: `src/lib.rs` (add `pub mod model;`)

- [ ] **Step 1: Add `pub mod model;` to `src/lib.rs`.**

- [ ] **Step 2: Write the failing test**

Create `src/model.rs`:

```rust
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Every quantity pv-hub tracks. Adding a metric here + a MetricDef in the
/// catalog is all that's needed for it to flow to every sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    // Irradiance
    Ghi, Dni, Dhi, PoaLocal, PoaProvider, PoaDeltaPct, ClearskyGhi, ClearskyIndex, Extraterrestrial,
    // Solar geometry
    SunElevation, SunAzimuth, SunZenith, Aoi, AirMass, IsDaytime,
    // Temperature
    AmbientTemp, ModuleTemp,
    // Meteo
    WindSpeed, WindDirection, RelHumidity, CloudCover, Precipitation, SurfacePressure,
    // Site echo
    Latitude, Longitude, Tilt, Azimuth, Albedo,
    // Health
    DataAge, LastUpdateEpoch, ProviderOk, PollErrorsTotal,
}

impl Metric {
    /// All variants, in catalog order. Must stay in sync with `catalog()` (Task 9).
    pub const ALL: [Metric; 32] = [
        Metric::Ghi, Metric::Dni, Metric::Dhi, Metric::PoaLocal, Metric::PoaProvider,
        Metric::PoaDeltaPct, Metric::ClearskyGhi, Metric::ClearskyIndex, Metric::Extraterrestrial,
        Metric::SunElevation, Metric::SunAzimuth, Metric::SunZenith, Metric::Aoi, Metric::AirMass,
        Metric::IsDaytime, Metric::AmbientTemp, Metric::ModuleTemp, Metric::WindSpeed,
        Metric::WindDirection, Metric::RelHumidity, Metric::CloudCover, Metric::Precipitation,
        Metric::SurfacePressure, Metric::Latitude, Metric::Longitude, Metric::Tilt, Metric::Azimuth,
        Metric::Albedo, Metric::DataAge, Metric::LastUpdateEpoch, Metric::ProviderOk,
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
            Metric::DataAge => self
                .last_weather_update
                .map(|t| (now - t).num_seconds().max(0) as f64),
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
```

Note: `Metric::ALL` lists every variant and must match `catalog()` (Task 9), whose `every_metric_variant_has_a_def` test guarantees no metric is forgotten in the register map.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test model::tests`
Expected: FAIL — `model` module empty / types missing.

- [ ] **Step 4:** The code in Step 2 is the implementation. Ensure it compiles.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test model::tests`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/model.rs src/lib.rs
git commit -m "feat: Metric enum and central SolarState"
```

---

### Task 4: Solar position wrapper

**Files:**
- Create: `src/solar/mod.rs`
- Create: `src/solar/position.rs`
- Modify: `src/lib.rs` (add `pub mod solar;`)

- [ ] **Step 1: Add `pub mod solar;` to `src/lib.rs`. Create `src/solar/mod.rs`:**

```rust
pub mod position;
pub mod transposition;
pub mod celltemp;
pub mod clearsky;

use crate::config::{Config, CellTempModel};
use crate::model::{Metric, WeatherInputs};
use chrono::{DateTime, Utc};

/// Runs all pure solar math and returns metric samples for the given instant.
/// Implemented in Task 8.
pub struct SolarEngine;
```

(`transposition`, `celltemp`, `clearsky` files are created in Tasks 5-7; add empty stubs now so `mod.rs` compiles.)

Create empty stubs:
- `src/solar/transposition.rs` → `// filled in Task 5`
- `src/solar/celltemp.rs` → `// filled in Task 6`
- `src/solar/clearsky.rs` → `// filled in Task 7`

- [ ] **Step 2: Write the failing test**

Create `src/solar/position.rs`:

```rust
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
    if coszen <= 0.0 { 0.0 } else { extraterrestrial_normal(doy) * coszen }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) { assert!((a - b).abs() < eps, "{a} != {b} (±{eps})"); }

    #[test]
    fn aoi_zero_when_sun_normal_to_panel() {
        // Sun at elevation 60 (zenith 30), due south; panel tilt 30 facing south → AOI = 0.
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
        let v = extraterrestrial_normal(172); // ~summer solstice
        assert!(v > 1300.0 && v < 1420.0, "got {v}");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test solar::position::tests`
Expected: FAIL — module empty before pasting.

- [ ] **Step 4:** Step 2 already contains the implementation. Now add the `spa`-backed calculator below the helpers in `src/solar/position.rs`:

```rust
/// Compute solar position via the NREL SPA algorithm (spa crate).
pub fn solar_position(utc: DateTime<Utc>, lat: f64, lon: f64) -> SolarPosition {
    match spa::solar_position::<spa::StdFloatOps>(utc, lat, lon) {
        Ok(p) => SolarPosition {
            zenith_deg: p.zenith_angle,
            azimuth_deg: p.azimuth,
            elevation_deg: 90.0 - p.zenith_angle,
        },
        Err(_) => SolarPosition { zenith_deg: 90.0, azimuth_deg: 0.0, elevation_deg: 0.0 },
    }
}
```

> **Crate note:** `spa` 0.5 exposes `spa::solar_position::<Ops>(datetime, lat, lon) -> Result<SolarPos, _>` with `SolarPos { azimuth, zenith_angle }` in degrees (azimuth clockwise from North). If the installed version differs, adjust only this function — the rest of the codebase depends on our `SolarPosition` struct, not on `spa`.

- [ ] **Step 5: Add an integration sanity test** for `solar_position` at the end of the `tests` module:

```rust
    #[test]
    fn milan_summer_noon_is_high() {
        use chrono::TimeZone;
        // 2024-06-21 ~11:30 UTC (near solar noon in Milan, UTC+2 DST)
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 11, 30, 0).unwrap();
        let pos = solar_position(t, 45.4642, 9.19);
        assert!(pos.elevation_deg > 60.0 && pos.elevation_deg < 72.0, "elev {}", pos.elevation_deg);
        assert!(pos.azimuth_deg > 150.0 && pos.azimuth_deg < 210.0, "az {}", pos.azimuth_deg);
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test solar::position::tests`
Expected: 6 passed.

- [ ] **Step 7: Commit**

```bash
git add src/solar/mod.rs src/solar/position.rs src/solar/transposition.rs src/solar/celltemp.rs src/solar/clearsky.rs src/lib.rs
git commit -m "feat: solar position wrapper, AOI, air mass, extraterrestrial"
```

---

### Task 5: Hay-Davies transposition (POA)

**Files:**
- Modify: `src/solar/transposition.rs`

- [ ] **Step 1: Write the failing test.** Replace the stub in `src/solar/transposition.rs`:

```rust
use crate::solar::position::{angle_of_incidence, extraterrestrial_normal, SolarPosition};

/// Plane-of-array global irradiance (W/m^2) via the Hay-Davies anisotropic model.
///
/// ghi/dni/dhi in W/m^2; tilt_deg from horizontal; azimuth_deg of the panel from North.
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

    // Ratio of beam on tilt vs horizontal (guard low sun).
    let cos_zen = zen.cos();
    let rb = if cos_zen > 0.087 { cos_aoi / cos_zen } else { 0.0 }; // ~ sun above 5°

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

    fn approx(a: f64, b: f64, eps: f64) { assert!((a - b).abs() < eps, "{a} != {b} (±{eps})"); }

    #[test]
    fn horizontal_panel_equals_ghi() {
        // tilt 0: beam+diffuse should reconstruct GHI (= DNI*cos z + DHI); ground term = 0.
        let pos = SolarPosition { elevation_deg: 50.0, azimuth_deg: 180.0, zenith_deg: 40.0 };
        let dni = 700.0; let dhi = 120.0;
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test solar::transposition::tests`
Expected: FAIL before paste; after paste it is the implementation.

- [ ] **Step 3:** The code above is the implementation.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test solar::transposition::tests`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/solar/transposition.rs
git commit -m "feat: Hay-Davies POA transposition"
```

---

### Task 6: Cell temperature (Faiman / NOCT)

**Files:**
- Modify: `src/solar/celltemp.rs`

- [ ] **Step 1: Write the failing test.** Replace the stub:

```rust
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
    fn approx(a: f64, b: f64, eps: f64) { assert!((a - b).abs() < eps, "{a} != {b}"); }

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
        // At 800 W/m^2, NOCT 45: rise = (45-20)/800*800 = 25 → cell = ambient + 25.
        approx(noct(20.0, 800.0, 45.0), 45.0, 1e-9);
    }
}
```

- [ ] **Step 2: Run test to verify it fails.** Run: `cargo test solar::celltemp::tests` → FAIL before paste.

- [ ] **Step 3:** Code above is the implementation.

- [ ] **Step 4: Run tests.** Run: `cargo test solar::celltemp::tests` → 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/solar/celltemp.rs
git commit -m "feat: Faiman and NOCT cell temperature models"
```

---

### Task 7: Clear-sky GHI + clearness index (Haurwitz)

**Files:**
- Modify: `src/solar/clearsky.rs`

- [ ] **Step 1: Write the failing test.** Replace the stub:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails.** Run: `cargo test solar::clearsky::tests` → FAIL before paste.

- [ ] **Step 3:** Code above is the implementation.

- [ ] **Step 4: Run tests.** Run: `cargo test solar::clearsky::tests` → 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/solar/clearsky.rs
git commit -m "feat: Haurwitz clear-sky GHI and clearness index"
```

---

### Task 8: SolarEngine orchestration

**Files:**
- Modify: `src/solar/mod.rs`

- [ ] **Step 1: Write the failing test.** Replace the `SolarEngine` stub in `src/solar/mod.rs` with:

```rust
use self::celltemp::{faiman, noct};
use self::clearsky::{clearness_index, haurwitz_ghi};
use self::position::{
    air_mass, angle_of_incidence, extraterrestrial_horizontal, solar_position,
};
use self::transposition::poa_hay_davies;
use chrono::Datelike;

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
                    CellTempModel::Faiman => faiman(tamb, poa, w.wind_speed.unwrap_or(0.0), cfg.faiman_u0, cfg.faiman_u1),
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
        assert!(!map.contains_key(&Metric::PoaLocal)); // no weather → no POA
    }

    #[test]
    fn produces_poa_and_module_temp_with_weather() {
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 11, 30, 0).unwrap();
        let w = WeatherInputs {
            ghi: Some(800.0), dni: Some(700.0), dhi: Some(140.0),
            ambient_temp: Some(27.0), wind_speed: Some(3.0), poa_provider: Some(910.0),
        };
        let map: HashMap<_, _> = SolarEngine::compute(&cfg(), t, &w).into_iter().collect();
        assert!(*map.get(&Metric::PoaLocal).unwrap() > 0.0);
        assert!(*map.get(&Metric::ModuleTemp).unwrap() > 27.0);
        assert!(map.contains_key(&Metric::PoaDeltaPct));
        assert!(map.contains_key(&Metric::ClearskyIndex));
    }
}
```

- [ ] **Step 2: Run test to verify it fails.** Run: `cargo test solar::tests` → FAIL before paste.

- [ ] **Step 3:** Code above is the implementation.

- [ ] **Step 4: Run tests.** Run: `cargo test solar::tests` → 2 passed. Then `cargo test solar` to confirm all solar submodules still pass.

- [ ] **Step 5: Commit**

```bash
git add src/solar/mod.rs
git commit -m "feat: SolarEngine orchestration of solar-derived metrics"
```

---

### Task 9: Metric catalog (Modbus register map)

**Files:**
- Create: `src/catalog.rs`
- Modify: `src/lib.rs` (add `pub mod catalog;`)

- [ ] **Step 1: Add `pub mod catalog;` to `src/lib.rs`.**

- [ ] **Step 2: Write the failing test.** Create `src/catalog.rs`:

```rust
use crate::model::Metric;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegKind { F32, U32 }

#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub metric: Metric,
    pub id: &'static str,     // snake_case key (JSON / MQTT subtopic)
    pub label: &'static str,
    pub unit: &'static str,
    pub category: &'static str,
    pub register: u16,        // Modbus base offset (0-based)
    pub kind: RegKind,        // both kinds occupy 2 registers
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
        MetricDef { metric: PoaDeltaPct, id: "poa_delta_pct", label: "POA Δ", unit: "%", category: "irradiance", register: 10, kind: F32 },
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
        // Catalog must cover the full enum, including PollErrorsTotal.
        let all = [
            Metric::Ghi, Metric::Dni, Metric::Dhi, Metric::PoaLocal, Metric::PoaProvider,
            Metric::PoaDeltaPct, Metric::ClearskyGhi, Metric::ClearskyIndex, Metric::Extraterrestrial,
            Metric::SunElevation, Metric::SunAzimuth, Metric::SunZenith, Metric::Aoi, Metric::AirMass,
            Metric::IsDaytime, Metric::AmbientTemp, Metric::ModuleTemp, Metric::WindSpeed,
            Metric::WindDirection, Metric::RelHumidity, Metric::CloudCover, Metric::Precipitation,
            Metric::SurfacePressure, Metric::Latitude, Metric::Longitude, Metric::Tilt, Metric::Azimuth,
            Metric::Albedo, Metric::DataAge, Metric::LastUpdateEpoch, Metric::ProviderOk, Metric::PollErrorsTotal,
        ];
        for m in all {
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
```

- [ ] **Step 3: Run test to verify it fails.** Run: `cargo test catalog::tests` → FAIL before paste.

- [ ] **Step 4:** Code above is the implementation.

- [ ] **Step 5: Run tests.** Run: `cargo test catalog::tests` → 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/catalog.rs src/lib.rs
git commit -m "feat: metric catalog with Modbus register map + invariants"
```

---

### Task 10: Hub (shared state + broadcast)

**Files:**
- Create: `src/hub.rs`
- Modify: `src/lib.rs` (add `pub mod hub;`)

- [ ] **Step 1: Add `pub mod hub;` to `src/lib.rs`.**

- [ ] **Step 2: Write the failing test.** Create `src/hub.rs`:

```rust
use crate::model::{Metric, SolarState};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Shared central state. Providers apply samples; sinks read snapshots and
/// subscribe to change notifications.
#[derive(Clone)]
pub struct Hub {
    state: Arc<RwLock<SolarState>>,
    tx: broadcast::Sender<()>,
}

impl Hub {
    pub fn new() -> Hub {
        let (tx, _rx) = broadcast::channel(16);
        Hub { state: Arc::new(RwLock::new(SolarState::default())), tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.tx.subscribe()
    }

    /// Apply metric samples; optionally mark a weather update time and provider status.
    pub async fn apply(&self, samples: &[(Metric, f64)], weather_update: Option<DateTime<Utc>>, provider_ok: Option<bool>) {
        {
            let mut s = self.state.write().await;
            for (m, v) in samples {
                s.set(*m, *v);
            }
            if let Some(t) = weather_update {
                s.last_weather_update = Some(t);
            }
            if let Some(ok) = provider_ok {
                s.provider_ok = ok;
            }
        }
        let _ = self.tx.send(());
    }

    pub async fn record_poll_error(&self) {
        {
            let mut s = self.state.write().await;
            s.poll_errors_total += 1;
            s.provider_ok = false;
        }
        let _ = self.tx.send(());
    }

    /// Read-only snapshot for sinks.
    pub async fn snapshot(&self) -> SolarState {
        self.state.read().await.clone()
    }
}

impl Default for Hub {
    fn default() -> Self { Hub::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[tokio::test]
    async fn apply_updates_and_notifies() {
        let hub = Hub::new();
        let mut rx = hub.subscribe();
        let t = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(t), Some(true)).await;
        assert!(rx.try_recv().is_ok());
        let snap = hub.snapshot().await;
        assert_eq!(snap.value(Metric::Ghi, t), Some(812.0));
        assert!(snap.provider_ok);
    }

    #[tokio::test]
    async fn poll_error_increments_and_clears_ok() {
        let hub = Hub::new();
        hub.apply(&[], None, Some(true)).await;
        hub.record_poll_error().await;
        let now = Utc::now();
        let snap = hub.snapshot().await;
        assert_eq!(snap.value(Metric::PollErrorsTotal, now), Some(1.0));
        assert_eq!(snap.value(Metric::ProviderOk, now), Some(0.0));
    }
}
```

- [ ] **Step 3: Run test to verify it fails.** Run: `cargo test hub::tests` → FAIL before paste.

- [ ] **Step 4:** Code above is the implementation.

- [ ] **Step 5: Run tests.** Run: `cargo test hub::tests` → 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src/hub.rs src/lib.rs
git commit -m "feat: Hub shared state with broadcast notifications"
```

---

### Task 11: Open-Meteo provider

**Files:**
- Create: `src/providers/mod.rs`
- Create: `src/providers/openmeteo.rs`
- Create: `tests/fixtures/openmeteo.json`
- Modify: `src/lib.rs` (add `pub mod providers;`)

- [ ] **Step 1: Add `pub mod providers;` to `src/lib.rs`. Create `src/providers/mod.rs`:**

```rust
pub mod openmeteo;

use crate::model::Metric;

/// A provider fetches remote data and returns metric samples.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn poll(&self) -> anyhow::Result<Vec<(Metric, f64)>>;
    fn name(&self) -> &'static str;
}
```

Add to `Cargo.toml` dependencies: `async-trait = "0.1"`.

- [ ] **Step 2: Create the test fixture** `tests/fixtures/openmeteo.json`:

```json
{
  "current": {
    "time": "2024-06-21T11:30",
    "temperature_2m": 27.4,
    "relative_humidity_2m": 41,
    "wind_speed_10m": 3.2,
    "wind_direction_10m": 225,
    "cloud_cover": 12,
    "precipitation": 0.0,
    "surface_pressure": 1013.0,
    "shortwave_radiation": 812.0,
    "direct_normal_irradiance": 690.0,
    "diffuse_radiation": 145.0,
    "global_tilted_irradiance": 918.0
  }
}
```

- [ ] **Step 3: Write the failing test.** Create `src/providers/openmeteo.rs`:

```rust
use crate::config::Config;
use crate::model::Metric;
use crate::providers::Provider;
use serde::Deserialize;

pub struct OpenMeteoProvider {
    base_url: String,
    api_key: Option<String>,
    lat: f64,
    lon: f64,
    tilt: f64,
    azimuth: f64,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct OmResponse { current: OmCurrent }

#[derive(Deserialize)]
struct OmCurrent {
    temperature_2m: Option<f64>,
    relative_humidity_2m: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    cloud_cover: Option<f64>,
    precipitation: Option<f64>,
    surface_pressure: Option<f64>,
    shortwave_radiation: Option<f64>,
    direct_normal_irradiance: Option<f64>,
    diffuse_radiation: Option<f64>,
    global_tilted_irradiance: Option<f64>,
}

impl OpenMeteoProvider {
    pub fn new(cfg: &Config) -> OpenMeteoProvider {
        OpenMeteoProvider {
            base_url: cfg.openmeteo_base_url.clone(),
            api_key: cfg.openmeteo_api_key.clone(),
            lat: cfg.latitude,
            lon: cfg.longitude,
            tilt: cfg.tilt_deg,
            azimuth: cfg.azimuth_deg,
            client: reqwest::Client::new(),
        }
    }

    /// Build the request URL. Open-Meteo azimuth is 0=S, +E; we convert from
    /// our North-referenced azimuth (180=S) via (az - 180).
    pub fn url(&self) -> String {
        let current = "temperature_2m,relative_humidity_2m,wind_speed_10m,wind_direction_10m,\
cloud_cover,precipitation,surface_pressure,shortwave_radiation,direct_normal_irradiance,\
diffuse_radiation,global_tilted_irradiance";
        let om_azimuth = self.azimuth - 180.0;
        let mut url = format!(
            "{}?latitude={}&longitude={}&current={}&tilt={}&azimuth={}&wind_speed_unit=ms&timezone=UTC",
            self.base_url, self.lat, self.lon, current, self.tilt, om_azimuth
        );
        if let Some(k) = &self.api_key {
            url.push_str(&format!("&apikey={k}"));
        }
        url
    }

    /// Parse a raw response body into samples.
    pub fn parse(body: &str) -> anyhow::Result<Vec<(Metric, f64)>> {
        let r: OmResponse = serde_json::from_str(body)?;
        let c = r.current;
        let mut out = Vec::new();
        let mut push = |m: Metric, v: Option<f64>| { if let Some(v) = v { out.push((m, v)); } };
        push(Metric::AmbientTemp, c.temperature_2m);
        push(Metric::RelHumidity, c.relative_humidity_2m);
        push(Metric::WindSpeed, c.wind_speed_10m);
        push(Metric::WindDirection, c.wind_direction_10m);
        push(Metric::CloudCover, c.cloud_cover);
        push(Metric::Precipitation, c.precipitation);
        push(Metric::SurfacePressure, c.surface_pressure);
        push(Metric::Ghi, c.shortwave_radiation);
        push(Metric::Dni, c.direct_normal_irradiance);
        push(Metric::Dhi, c.diffuse_radiation);
        push(Metric::PoaProvider, c.global_tilted_irradiance);
        Ok(out)
    }
}

#[async_trait::async_trait]
impl Provider for OpenMeteoProvider {
    async fn poll(&self) -> anyhow::Result<Vec<(Metric, f64)>> {
        let body = self.client.get(self.url()).send().await?.error_for_status()?.text().await?;
        OpenMeteoProvider::parse(&body)
    }
    fn name(&self) -> &'static str { "openmeteo" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[test]
    fn url_contains_key_params() {
        let p = OpenMeteoProvider::new(&cfg());
        let u = p.url();
        assert!(u.contains("latitude=45.4642"));
        assert!(u.contains("shortwave_radiation"));
        assert!(u.contains("global_tilted_irradiance"));
        assert!(u.contains("tilt=30"));
        assert!(u.contains("azimuth=0")); // 180 (South, our convention) → 0 in Open-Meteo
    }

    #[test]
    fn parses_fixture() {
        let body = include_str!("../../tests/fixtures/openmeteo.json");
        let map: HashMap<_, _> = OpenMeteoProvider::parse(body).unwrap().into_iter().collect();
        assert_eq!(map.get(&Metric::Ghi), Some(&812.0));
        assert_eq!(map.get(&Metric::PoaProvider), Some(&918.0));
        assert_eq!(map.get(&Metric::AmbientTemp), Some(&27.4));
    }

    #[tokio::test]
    async fn poll_against_mock_server() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = include_str!("../../tests/fixtures/openmeteo.json");
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        m.insert("PVHUB_OPENMETEO_BASE_URL".into(), server.uri());
        let c = Config::from_map(&m).unwrap();
        let p = OpenMeteoProvider::new(&c);
        let samples = p.poll().await.unwrap();
        assert!(samples.iter().any(|(mtc, _)| *mtc == Metric::Ghi));
    }
}
```

- [ ] **Step 4: Run test to verify it fails.** Run: `cargo test providers::openmeteo::tests` → FAIL before paste.

- [ ] **Step 5:** Code above is the implementation.

- [ ] **Step 6: Run tests.** Run: `cargo test providers::openmeteo::tests` → 3 passed.

- [ ] **Step 7: Commit**

```bash
git add src/providers Cargo.toml Cargo.lock tests/fixtures/openmeteo.json src/lib.rs
git commit -m "feat: Open-Meteo provider (URL build + parse + poll)"
```

---

### Task 12: Modbus register-bank encoding

**Files:**
- Create: `src/sinks/mod.rs`
- Create: `src/sinks/modbus/mod.rs`
- Modify: `src/lib.rs` (add `pub mod sinks;`)

- [ ] **Step 1: Add `pub mod sinks;` to `src/lib.rs`. Create `src/sinks/mod.rs`:**

```rust
pub mod modbus;
```

- [ ] **Step 2: Write the failing test.** Create `src/sinks/modbus/mod.rs`:

```rust
pub mod frame;
pub mod server;

use crate::catalog::{catalog, RegKind};
use crate::config::WordOrder;
use crate::model::SolarState;
use chrono::{DateTime, Utc};

/// Split a 32-bit value into two 16-bit registers honoring word order.
fn words_from_u32(raw: u32, order: WordOrder) -> [u16; 2] {
    let hi = (raw >> 16) as u16;
    let lo = (raw & 0xFFFF) as u16;
    match order {
        WordOrder::Abcd => [hi, lo],
        WordOrder::Cdab => [lo, hi],
    }
}

/// Build the full Modbus register bank from the catalog + current state.
pub fn build_bank(state: &SolarState, now: DateTime<Utc>, order: WordOrder) -> Vec<u16> {
    let mut bank = vec![0u16; crate::catalog::bank_words()];
    for d in catalog() {
        let Some(v) = state.value(d.metric, now) else { continue };
        let words = match d.kind {
            RegKind::F32 => words_from_u32((v as f32).to_bits(), order),
            RegKind::U32 => words_from_u32(v as u32, order),
        };
        let base = d.register as usize;
        bank[base] = words[0];
        bank[base + 1] = words[1];
    }
    bank
}

/// Decode two registers back to f32 (used by tests / clients).
pub fn f32_from_words(w0: u16, w1: u16, order: WordOrder) -> f32 {
    let (hi, lo) = match order {
        WordOrder::Abcd => (w0, w1),
        WordOrder::Cdab => (w1, w0),
    };
    f32::from_bits(((hi as u32) << 16) | lo as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::def_for;
    use crate::model::Metric;

    fn approx(a: f32, b: f32, eps: f32) { assert!((a - b).abs() < eps, "{a} != {b}"); }

    #[test]
    fn f32_roundtrip_abcd() {
        let mut s = SolarState::default();
        s.set(Metric::Ghi, 812.5);
        let now = Utc::now();
        let bank = build_bank(&s, now, WordOrder::Abcd);
        let reg = def_for(Metric::Ghi).unwrap().register as usize;
        approx(f32_from_words(bank[reg], bank[reg + 1], WordOrder::Abcd), 812.5, 1e-3);
    }

    #[test]
    fn f32_roundtrip_cdab_swaps_words() {
        let mut s = SolarState::default();
        s.set(Metric::Dni, 690.0);
        let now = Utc::now();
        let reg = def_for(Metric::Dni).unwrap().register as usize;
        let abcd = build_bank(&s, now, WordOrder::Abcd);
        let cdab = build_bank(&s, now, WordOrder::Cdab);
        assert_eq!(abcd[reg], cdab[reg + 1]);
        assert_eq!(abcd[reg + 1], cdab[reg]);
        approx(f32_from_words(cdab[reg], cdab[reg + 1], WordOrder::Cdab), 690.0, 1e-3);
    }

    #[test]
    fn missing_metric_stays_zero() {
        let s = SolarState::default();
        let now = Utc::now();
        let bank = build_bank(&s, now, WordOrder::Abcd);
        let reg = def_for(Metric::Ghi).unwrap().register as usize;
        assert_eq!(bank[reg], 0);
        assert_eq!(bank[reg + 1], 0);
    }
}
```

Note: `server` is created in Task 13; add `pub mod server;` now and create an empty `src/sinks/modbus/server.rs` stub (`// filled in Task 13`) so this compiles.

- [ ] **Step 3: Run test to verify it fails.** Run: `cargo test sinks::modbus::tests` → FAIL before paste.

- [ ] **Step 4:** Code above is the implementation.

- [ ] **Step 5: Run tests.** Run: `cargo test sinks::modbus::tests` → 3 passed.

- [ ] **Step 6: Commit**

```bash
git add src/sinks/mod.rs src/sinks/modbus/mod.rs src/sinks/modbus/server.rs src/lib.rs
git commit -m "feat: Modbus register-bank encoding from catalog"
```

---

### Task 13: Modbus TCP frame handler + server

**Files:**
- Create: `src/sinks/modbus/frame.rs`
- Modify: `src/sinks/modbus/server.rs`

- [ ] **Step 1: Write the failing test.** Create `src/sinks/modbus/frame.rs`:

```rust
//! Pure, read-only Modbus request handling over a fixed register bank.
//! Supports FC03 (Read Holding Registers) and FC04 (Read Input Registers).

const FC_READ_HOLDING: u8 = 0x03;
const FC_READ_INPUT: u8 = 0x04;

/// Handle one full ADU (MBAP header + PDU). Returns the full response ADU.
/// `holding_mirror` = whether FC03 is served (else FC03 → illegal function).
pub fn handle_adu(bank: &[u16], holding_mirror: bool, unit_id: u8, req: &[u8]) -> Vec<u8> {
    // MBAP: [tx_hi, tx_lo, proto_hi, proto_lo, len_hi, len_lo, unit] then PDU.
    if req.len() < 8 {
        return Vec::new(); // too short to even echo a transaction id
    }
    let tx = [req[0], req[1]];
    let req_unit = req[6];
    let pdu = &req[7..];
    let fc = pdu[0];

    let make = |payload: Vec<u8>| -> Vec<u8> {
        let len = (payload.len() + 1) as u16; // +1 for unit id
        let mut out = Vec::with_capacity(7 + payload.len());
        out.extend_from_slice(&tx);
        out.extend_from_slice(&[0, 0]); // protocol id
        out.extend_from_slice(&len.to_be_bytes());
        out.push(req_unit);
        out.extend_from_slice(&payload);
        out
    };
    let exception = |code: u8| make(vec![fc | 0x80, code]);

    // Accept our configured unit id or the broadcast/wildcard 0.
    if req_unit != unit_id && req_unit != 0 {
        return Vec::new();
    }

    match fc {
        FC_READ_HOLDING if !holding_mirror => exception(0x01),
        FC_READ_HOLDING | FC_READ_INPUT => {
            if pdu.len() < 5 {
                return exception(0x03);
            }
            let start = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
            let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
            if qty == 0 || qty > 125 {
                return exception(0x03);
            }
            if start + qty > bank.len() {
                return exception(0x02);
            }
            let mut payload = vec![fc, (qty * 2) as u8];
            for w in &bank[start..start + qty] {
                payload.extend_from_slice(&w.to_be_bytes());
            }
            make(payload)
        }
        _ => exception(0x01),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_req(fc: u8, unit: u8, start: u16, qty: u16) -> Vec<u8> {
        let mut v = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x06, unit, fc];
        v.extend_from_slice(&start.to_be_bytes());
        v.extend_from_slice(&qty.to_be_bytes());
        v
    }

    #[test]
    fn reads_input_registers() {
        let bank = vec![0x1234u16, 0x5678, 0x9ABC];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 1, 0, 2));
        // MBAP(7) + fc + bytecount + 4 data bytes
        assert_eq!(resp[7], FC_READ_INPUT);
        assert_eq!(resp[8], 4); // byte count
        assert_eq!(&resp[9..13], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn holding_disabled_returns_exception() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, false, 1, &read_req(FC_READ_HOLDING, 1, 0, 1));
        assert_eq!(resp[7], FC_READ_HOLDING | 0x80);
        assert_eq!(resp[8], 0x01);
    }

    #[test]
    fn out_of_range_returns_exception_02() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 1, 3, 5));
        assert_eq!(resp[7], FC_READ_INPUT | 0x80);
        assert_eq!(resp[8], 0x02);
    }

    #[test]
    fn unknown_function_returns_exception_01() {
        let bank = vec![0u16; 4];
        let mut req = read_req(0x10, 1, 0, 1);
        req[7] = 0x10; // write multiple — unsupported
        let resp = handle_adu(&bank, true, 1, &req);
        assert_eq!(resp[7], 0x10 | 0x80);
        assert_eq!(resp[8], 0x01);
    }

    #[test]
    fn wrong_unit_id_is_ignored() {
        let bank = vec![0u16; 4];
        let resp = handle_adu(&bank, true, 1, &read_req(FC_READ_INPUT, 9, 0, 1));
        assert!(resp.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails.** Run: `cargo test sinks::modbus::frame::tests` → FAIL before paste.

- [ ] **Step 3:** Code above is the implementation.

- [ ] **Step 4: Run tests.** Run: `cargo test sinks::modbus::frame::tests` → 5 passed.

- [ ] **Step 5: Implement the TCP server.** Replace the stub in `src/sinks/modbus/server.rs`:

```rust
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::modbus::{build_bank, frame::handle_adu};
use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Run the Modbus TCP slave until the process ends. One task per connection.
pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let addr = format!("{}:{}", cfg.modbus_bind, cfg.modbus_port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("modbus TCP slave listening on {addr}");
    loop {
        let (sock, peer) = listener.accept().await?;
        let cfg = cfg.clone();
        let hub = hub.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(sock, &cfg, &hub).await {
                tracing::debug!("modbus conn {peer} ended: {e}");
            }
        });
    }
}

async fn handle_conn(mut sock: TcpStream, cfg: &Config, hub: &Hub) -> anyhow::Result<()> {
    let mut header = [0u8; 7];
    loop {
        // Read MBAP header.
        if sock.read_exact(&mut header).await.is_err() {
            return Ok(()); // client closed
        }
        let len = u16::from_be_bytes([header[4], header[5]]) as usize;
        if len == 0 || len > 253 {
            return Ok(());
        }
        // Read the remaining (len - 1) PDU bytes (len counts unit id already in header[6]).
        let mut pdu = vec![0u8; len - 1];
        sock.read_exact(&mut pdu).await?;

        let mut adu = Vec::with_capacity(7 + pdu.len());
        adu.extend_from_slice(&header);
        adu.extend_from_slice(&pdu);

        let snap = hub.snapshot().await;
        let bank = build_bank(&snap, Utc::now(), cfg.modbus_word_order);
        let resp = handle_adu(&bank, cfg.modbus_holding_mirror, cfg.modbus_unit_id, &adu);
        if !resp.is_empty() {
            sock.write_all(&resp).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Metric;
    use crate::sinks::modbus::f32_from_words;
    use crate::config::WordOrder;
    use std::collections::HashMap;

    #[tokio::test]
    async fn end_to_end_read_over_tcp() {
        // Config on an ephemeral port.
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.0".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.0".into());
        m.insert("PVHUB_MODBUS_PORT".into(), "0".into());
        let mut cfg = Config::from_map(&m).unwrap();

        // Bind manually to learn the assigned port, then hand the listener logic a spawn.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        cfg.modbus_port = port;

        let hub = Hub::new();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(Utc::now()), Some(true)).await;

        let cfg2 = cfg.clone();
        let hub2 = hub.clone();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            handle_conn(sock, &cfg2, &hub2).await.unwrap();
        });

        // Raw client: read input registers 0..2 (GHI at register 0).
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let req = [0u8, 1, 0, 0, 0, 6, 1, 0x04, 0, 0, 0, 2];
        client.write_all(&req).await.unwrap();
        let mut resp = [0u8; 13];
        client.read_exact(&mut resp).await.unwrap();
        let ghi = f32_from_words(
            u16::from_be_bytes([resp[9], resp[10]]),
            u16::from_be_bytes([resp[11], resp[12]]),
            WordOrder::Abcd,
        );
        assert!((ghi - 812.0).abs() < 1e-2, "ghi {ghi}");
    }
}
```

- [ ] **Step 6: Run tests.** Run: `cargo test sinks::modbus` → all frame + server tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/sinks/modbus/frame.rs src/sinks/modbus/server.rs
git commit -m "feat: read-only Modbus TCP slave (FC03/FC04) with e2e test"
```

---

### Task 14: Scheduler + main wiring + graceful shutdown

**Files:**
- Create: `src/scheduler.rs`
- Modify: `src/lib.rs` (add `pub mod scheduler;` and flesh out `run()`)

- [ ] **Step 1: Add `pub mod scheduler;` to `src/lib.rs`. Create `src/scheduler.rs`:**

```rust
use crate::config::Config;
use crate::hub::Hub;
use crate::providers::openmeteo::OpenMeteoProvider;
use crate::providers::Provider;
use crate::solar::SolarEngine;
use chrono::Utc;
use std::time::Duration;

/// Periodically fetch weather from the provider and apply to the hub.
pub async fn weather_loop(cfg: Config, hub: Hub) {
    let provider = OpenMeteoProvider::new(&cfg);
    let mut backoff = 1u64;
    loop {
        match provider.poll().await {
            Ok(samples) => {
                hub.apply(&samples, Some(Utc::now()), Some(true)).await;
                tracing::info!("weather updated ({} samples)", samples.len());
                backoff = 1;
                tokio::time::sleep(Duration::from_secs(cfg.poll_interval_s)).await;
            }
            Err(e) => {
                hub.record_poll_error().await;
                tracing::warn!("weather poll failed: {e}; retrying in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(cfg.poll_interval_s.max(2));
            }
        }
    }
}

/// Periodically recompute solar-derived metrics (no network).
pub async fn solar_loop(cfg: Config, hub: Hub) {
    hub.apply(&SolarEngine::site_metrics(&cfg), None, None).await;
    loop {
        let snap = hub.snapshot().await;
        let samples = SolarEngine::compute(&cfg, Utc::now(), &snap.weather_inputs());
        hub.apply(&samples, None, None).await;
        tokio::time::sleep(Duration::from_secs(cfg.solar_interval_s)).await;
    }
}
```

- [ ] **Step 2: Flesh out `run()` in `src/lib.rs`.** Replace the Task-1 `run()` with:

```rust
pub async fn run() -> Result<()> {
    let cfg = config::Config::from_env().map_err(|e| anyhow::anyhow!(e))?;
    init_tracing(&cfg.log_level);
    tracing::info!("pv-hub {} starting — site '{}' at {},{}",
        env!("CARGO_PKG_VERSION"), cfg.site_name, cfg.latitude, cfg.longitude);

    let hub = hub::Hub::new();

    // Seed site metrics + one immediate solar computation so Modbus has data at t=0.
    hub.apply(&solar::SolarEngine::site_metrics(&cfg), None, Some(false)).await;

    let mut tasks = Vec::new();
    tasks.push(tokio::spawn(scheduler::solar_loop(cfg.clone(), hub.clone())));
    tasks.push(tokio::spawn(scheduler::weather_loop(cfg.clone(), hub.clone())));
    if cfg.modbus_enable {
        let c = cfg.clone();
        let h = hub.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = sinks::modbus::server::serve(c, h).await {
                tracing::error!("modbus server error: {e}");
            }
        }));
    }

    tokio::signal::ctrl_c().await.ok();
    tracing::info!("shutdown signal received; stopping");
    for t in tasks { t.abort(); }
    Ok(())
}
```

Ensure `src/lib.rs` declares all modules used: `config, model, catalog, hub, solar, providers, sinks, scheduler`.

- [ ] **Step 3: Verify full build and test suite.**

Run: `cargo build`
Expected: clean build.

Run: `cargo test`
Expected: all unit/integration tests pass.

- [ ] **Step 4: Manual smoke test.**

Run:
```bash
PVHUB_LATITUDE=45.4642 PVHUB_LONGITUDE=9.19 PVHUB_MODBUS_PORT=1502 cargo run
```
Expected logs: "starting", "modbus TCP slave listening on 0.0.0.0:1502", and within a few seconds "weather updated (N samples)". Leave it running.

In another shell, verify a Modbus read (if `mbpoll` is available; otherwise skip — the e2e test already covers it):
```bash
mbpoll -m tcp -p 1502 -a 1 -t 4 -r 1 -c 20 -1 127.0.0.1
```
Expected: 20 input registers printed (register 1 = GHI high word, etc.). Stop the service with Ctrl-C; expect a clean "shutdown signal received" log.

- [ ] **Step 5: Commit**

```bash
git add src/scheduler.rs src/lib.rs
git commit -m "feat: scheduler wiring, seed data, graceful shutdown"
```

---

## Self-review (completed against the spec)

**Spec coverage (Plan 1 scope):**
- Config via env, fail-fast on required/invalid → Task 2 ✓
- Central SolarState + Metric catalog (single source, register map) → Tasks 3, 9 ✓
- Solar position, AOI, air mass, extraterrestrial → Task 4 ✓
- POA local (Hay-Davies) → Task 5 ✓; POA provider (Open-Meteo `global_tilted_irradiance`) + Δ% cross-check → Tasks 11, 8 ✓
- Cell temperature Faiman/NOCT → Task 6 ✓
- Clear-sky GHI + kt → Task 7 ✓
- Open-Meteo provider (pluggable trait, optional API key/base URL) → Task 11 ✓
- Modbus TCP slave, float32, word order abcd/cdab, FC04 + FC03 holding mirror, register map from catalog → Tasks 12, 13 ✓
- Resilience: solar loop network-free, provider backoff, last-known values, provider_ok/poll_errors/data_age → Tasks 8, 10, 14 ✓
- Graceful shutdown on signal → Task 14 ✓
- Tests: solar math, encoding round-trip, catalog invariants, mocked provider, e2e Modbus → all tasks ✓

**Deferred to later plans (intentionally out of Plan 1 scope):**
- HTTP API + SSE + SCADA UI → **Plan 2**
- Perez transposition variant (only Hay-Davies implemented; `Perez` enum value parses but Task 8 uses Hay-Davies — Plan 2/3 or a follow-up adds Perez; note in code) 
- Modbus Discrete Inputs (FC02) for boolean flags → optional future task
- Dockerfile / compose / README → **Plan 3**

**Placeholder scan:** No TBD/TODO left; the three `src/solar/*.rs` stubs created in Task 4 are all replaced with real code in Tasks 5-7, and `server.rs` stub is replaced in Task 13.

**Type consistency:** `Metric`, `SolarState::value/set/raw/weather_inputs`, `WeatherInputs` fields, `Config` field names, `WordOrder`, `MetricDef`/`RegKind`, `build_bank`/`f32_from_words`, `handle_adu`, `Provider::poll`, `SolarEngine::compute/site_metrics`, and `Hub::apply/snapshot/subscribe/record_poll_error` are used identically across tasks.

**Known follow-up:** `Config.transposition == Perez` currently falls through to Hay-Davies in `SolarEngine::compute`. Add a Perez implementation or reject `perez` in config validation before shipping if strict. Tracked for Plan 2.
```

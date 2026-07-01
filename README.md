# pv-hub — Solarimetro

A tiny Rust microservice that turns **free weather/solar APIs** into a **virtual solarimeter** for photovoltaic diagnostics. It fetches irradiance and weather for one location, computes derived PV quantities locally (solar position, plane-of-array irradiance, module temperature, clearness index), and exposes everything two ways:

- **Modbus TCP** (read-only slave) — for PLC/SCADA integration
- **A responsive web SCADA dashboard** (dark/light) — map, sun path, gauges, live values via SSE

One site per container; configure it with coordinates as environment variables. Single static binary, distroless image, a few MB of RAM.

![dashboard](docs/superpowers/specs/assets/2026-07-01-scada-ui-mockup.html)

---

## Quick start

```bash
# build + run with the example compose (edit coordinates first)
podman build -t pv-hub:0.1 .
docker compose up            # or: podman-compose up

# open the dashboard
xdg-open http://localhost:8080
```

Or run the binary directly:

```bash
PVHUB_LATITUDE=45.4642 PVHUB_LONGITUDE=9.19 PVHUB_SITE_NAME="My plant" \
  cargo run --release
```

The dashboard is at `http://localhost:8080`; the Modbus slave listens on `:502`.

---

## Configuration (environment variables)

| Variable | Default | Notes |
|---|---|---|
| `PVHUB_LATITUDE` / `PVHUB_LONGITUDE` | — | **required**; fail-fast if missing/invalid |
| `PVHUB_SITE_NAME` | `pv-hub` | label shown in UI/logs |
| `PVHUB_ELEVATION_M` | auto | site elevation (m) |
| `PVHUB_TILT_DEG` / `PVHUB_AZIMUTH_DEG` | `30` / `180` | panel tilt / azimuth (180 = South) |
| `PVHUB_ALBEDO` | `0.20` | ground reflectance (e.g. 0.8 snow) |
| `PVHUB_TRANSPOSITION` | `hay_davies` | `hay_davies` \| `perez` |
| `PVHUB_CELLTEMP` | `faiman` | `faiman` \| `noct` |
| `PVHUB_CELLTEMP_U0` / `_U1` | `25` / `6.84` | Faiman coefficients |
| `PVHUB_CELLTEMP_NOCT` | `45` | NOCT (°C) |
| `PVHUB_POLL_INTERVAL_S` | `600` | weather refresh cadence |
| `PVHUB_SOLAR_INTERVAL_S` | `60` | sun-position recompute cadence |
| `PVHUB_PROVIDER` | `openmeteo` | data provider (pluggable) |
| `PVHUB_OPENMETEO_BASE_URL` | Open-Meteo free API | override for self-host/commercial |
| `PVHUB_OPENMETEO_API_KEY` | — | for the commercial Open-Meteo plan |
| `PVHUB_HTTP_BIND` / `_PORT` | `0.0.0.0` / `8080` | web UI + API |
| `PVHUB_MODBUS_ENABLE` | `true` | enable the Modbus slave |
| `PVHUB_MODBUS_BIND` / `_PORT` | `0.0.0.0` / `502` | Modbus TCP endpoint |
| `PVHUB_MODBUS_UNIT_ID` | `1` | Modbus unit id |
| `PVHUB_MODBUS_WORD_ORDER` | `abcd` | `abcd` \| `cdab` (word swap for many PLCs) |
| `PVHUB_MODBUS_HOLDING_MIRROR` | `true` | also serve values as holding registers (FC03) |
| `PVHUB_DEFAULT_THEME` | `auto` | `auto` \| `dark` \| `light` |
| `PVHUB_LOG_LEVEL` | `info` | `RUST_LOG` also honored |

---

## Modbus register map

Values are **float32** across **2 registers each**, big-endian word order `abcd` by default (`cdab` swaps the words). Served as **Input Registers (FC04)** and mirrored read-only as **Holding Registers (FC03)**. `last_update_epoch` is a `u32`.

| Reg | Metric | Unit | | Reg | Metric | Unit |
|---|---|---|---|---|---|---|
| 0 | ghi | W/m² | | 50 | ambient_temp | °C |
| 2 | dni | W/m² | | 52 | module_temp | °C |
| 4 | dhi | W/m² | | 60 | wind_speed | m/s |
| 6 | poa_local | W/m² | | 62 | wind_direction | ° |
| 8 | poa_provider | W/m² | | 64 | rel_humidity | % |
| 10 | poa_delta_pct | % | | 66 | cloud_cover | % |
| 12 | clearsky_ghi | W/m² | | 68 | precipitation | mm |
| 14 | clearsky_index | – | | 70 | surface_pressure | hPa |
| 16 | extraterrestrial | W/m² | | 90 | latitude | ° |
| 30 | sun_elevation | ° | | 92 | longitude | ° |
| 32 | sun_azimuth | ° | | 94 | tilt | ° |
| 34 | sun_zenith | ° | | 96 | azimuth | ° |
| 36 | aoi | ° | | 98 | albedo | – |
| 38 | air_mass | – | | 110 | data_age | s |
| 40 | is_daytime | 0/1 | | 112 | last_update_epoch (u32) | s |
| | | | | 114 | provider_ok | 0/1 |
| | | | | 116 | poll_errors_total | – |

The full machine-readable map (with units) is served at `/api/catalog.json`.

---

## HTTP API

| Endpoint | Description |
|---|---|
| `GET /` | SCADA dashboard |
| `GET /api/state.json` | full current snapshot (all metrics + site + provider status) |
| `GET /api/stream` | Server-Sent Events; a `state` event on every update |
| `GET /api/catalog.json` | metric catalog (id, label, unit, Modbus register) |
| `GET /health` | liveness probe |

---

## What it computes

- **Solar position** (NREL/PSA algorithm): elevation, azimuth, zenith, angle of incidence, air mass, extraterrestrial irradiance.
- **POA (plane-of-array) irradiance** via Hay-Davies transposition from GHI/DNI/DHI — computed **locally** and compared against the provider's tilted value (`poa_delta_pct`) as a cross-check.
- **Module temperature** (Faiman model with wind, or NOCT).
- **Clearness index kt** = measured GHI / Haurwitz clear-sky GHI — a quick "cloudy vs. plant problem" indicator.

The solar engine needs no network, so sun position and geometry stay live even if the weather provider is temporarily unreachable.

---

## Extending it

The design centers on one `SolarState` plus a **metric catalog** (`src/catalog.rs`) that is the single source of truth. Every sink derives from it.

- **Add a metric:** add a variant to `Metric` (`src/model.rs`) and one line to `catalog()` — it then appears in the JSON API, the SSE stream, and the Modbus map automatically.
- **Add a data provider:** implement the `Provider` trait (`src/providers/mod.rs`) and register it in the scheduler.
- **Add a sink (e.g. MQTT):** read the `Hub` snapshot + catalog and publish. This is the intended next step — the architecture keeps it to a single module.

Run the tests with `cargo test`.

---

## Data attribution & license

Weather data by [Open-Meteo.com](https://open-meteo.com) (CC-BY 4.0). The free tier is for non-commercial use; set `PVHUB_OPENMETEO_API_KEY` (and/or `PVHUB_OPENMETEO_BASE_URL`) for the commercial plan. Map tiles © OpenStreetMap contributors.

Licensed under the MIT License.

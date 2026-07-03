# Design — Home Assistant sink (MQTT Discovery)

Date: 2026-07-03
Status: approved (brainstorming), pending implementation plan

## 1. Goal

Add a new output sink, `homeassistant`, that publishes pv-hub's metrics to a Home
Assistant instance over MQTT using **Home Assistant MQTT Discovery**. HA then
auto-creates one sensor entity per metric under a single device, records them, and
can graph them natively (history/recorder).

The user reads PV **active power** with a separate piece of software and publishes
it into the same Home Assistant independently. pv-hub therefore publishes **only
its own metrics** (irradiance, POA, temperatures, geometry, meteo, health). In HA
the user overlays irradiance and active power on the same chart. **No change to
pv-hub's data model** (no new `Metric` variant, no power calculation).

## 2. Non-goals

- No active-power metric or PV power model in pv-hub.
- No new input provider.
- No TLS to the broker in this iteration (plain TCP). It is an obvious later
  addition (rumqttc supports rustls); left out to keep scope tight.
- No change to `catalog.rs` / `model.rs`.

## 3. Architecture

Follows the existing sink convention exactly: a self-contained module under
`src/sinks/<name>/` exposing `pub async fn serve(cfg: Config, hub: Hub) ->
anyhow::Result<()>`, spawned as a gated tokio task in `run()` (mirrors the modbus
sink). The sink iterates `catalog()` and reads the shared `Hub` snapshot, so any
future metric appears in HA automatically.

### Files

- `src/sinks/homeassistant/mod.rs` — `serve()`: builds MQTT options, opens the
  connection (rumqttc `AsyncClient` + `EventLoop`), sets the LWT/availability,
  runs the connection/publish loop, reconnects with backoff.
- `src/sinks/homeassistant/discovery.rs` — **pure, unit-tested** helpers: topic
  builders, `unit → (unit_of_measurement, device_class, state_class)` mapping, and
  the per-metric discovery-config JSON builder. No I/O here.
- `src/sinks/mod.rs` — add `pub mod homeassistant;`.

### Reuse

The state payload reuses the same shape already produced for the HTTP sink's
`/api/state.json` (`src/sinks/http/api.rs`, `state_json`): a JSON document whose
`metrics` map is keyed by `MetricDef.id` with `{ value, unit, label, category }`
per entry. HA sensors read their value via `value_template`. If practical, the
shared builder is lifted to a small reusable function; otherwise the HA sink
constructs the same structure directly.

## 4. MQTT topics

`<disc>` = `PVHUB_HA_DISCOVERY_PREFIX` (default `homeassistant`).
`<node>` = `PVHUB_HA_NODE_ID` (default `pvhub`).

| Purpose | Topic | Retained | Payload |
|---|---|---|---|
| Discovery config (per metric) | `<disc>/sensor/<node>/<metric_id>/config` | yes | JSON discovery doc |
| State (single doc, all metrics) | `pvhub/<node>/state` | yes | JSON state document |
| Availability / LWT | `pvhub/<node>/status` | yes | `online` / `offline` |

**State-topic strategy (decision A1):** one JSON state topic for all metrics; each
sensor extracts its field with `value_template`. One publish updates every sensor.
Chosen over per-metric state topics (30+ publishes/update, more code).

## 5. Discovery payload (per metric)

For each `MetricDef d` in `catalog()`, publish a retained config to
`<disc>/sensor/<node>/<d.id>/config` with:

- `name`: `d.label`
- `unique_id`: `pvhub_<node>_<d.id>`
- `object_id`: `pvhub_<d.id>` (stable entity_id hint)
- `state_topic`: `pvhub/<node>/state`
- `value_template`: `{{ value_json.<d.id>.value }}`
- `unit_of_measurement`: normalized unit (see mapping) — omitted when empty
- `device_class`: from mapping — omitted when none
- `state_class`: from mapping (default `measurement`)
- `availability_topic`: `pvhub/<node>/status`, `payload_available: online`,
  `payload_not_available: offline`
- `device`: shared block so all sensors group under one HA device —
  `identifiers: ["<node>"]`, `name: <site_name>`, `manufacturer: "pv-hub"`,
  `model: "Solarimetro"`

### Unit → HA metadata mapping (decision B1: lives in the HA sink)

Catalog units are normalized to HA-accepted units and classed as follows. The
mapping is keyed on `unit`, with a few `id`-based special cases.

| catalog `unit` | HA `unit_of_measurement` | `device_class` | `state_class` |
|---|---|---|---|
| `W/m2` | `W/m²` | `irradiance` | `measurement` |
| `degC` | `°C` | `temperature` | `measurement` |
| `m/s` | `m/s` | `wind_speed` | `measurement` |
| `hPa` | `hPa` | `atmospheric_pressure` | `measurement` |
| `mm` | `mm` | `precipitation` | `measurement` |
| `%` | `%` | (none) | `measurement` |
| `deg` | `°` | (none) | `measurement` |
| `s` | `s` | `duration` | `measurement` |
| `` (empty) | (omitted) | (none) | `measurement` |

`id`-based special cases (override the table above):

- `rel_humidity` → `device_class: humidity` (unit `%`).
- `last_update_epoch` → no `device_class` (epoch integer, not an ISO timestamp),
  `state_class: measurement`.
- `poll_errors_total` → no unit, no `device_class`, `state_class: total_increasing`.
- `provider_ok` and `is_daytime` → published as numeric `0`/`1`, no `device_class`,
  `state_class: measurement`. (Kept as `sensor` for MVP simplicity rather than
  `binary_sensor`.)

Rationale for normalization: HA rejects/ignores a `device_class` whose
`unit_of_measurement` is incompatible, so `W/m2`→`W/m²` and `degC`→`°C` are
required for the irradiance/temperature classes to take effect.

## 6. State payload

Published (retained) to `pvhub/<node>/state` as JSON. Same structure as
`/api/state.json`:

```json
{
  "site": "...", "provider_ok": true, "timestamp": "2026-07-03T...Z",
  "metrics": {
    "ghi":  { "value": 812.3, "unit": "W/m2", "label": "GHI", "category": "irradiance" },
    "poa_local": { "value": 905.1, "unit": "W/m2", "label": "POA local", "category": "irradiance" },
    "...": { }
  }
}
```

`value_template: {{ value_json.ghi.value }}` reads each sensor. A `null` value maps
to HA "unknown" for that sensor, which is acceptable.

## 7. Lifecycle & robustness

- **Connect:** set LWT = (`pvhub/<node>/status`, `offline`, retained, QoS 1). After
  `ConnAck`: publish all discovery configs (retained), then `status=online`, then
  the current state document.
- **Steady state:** publish the state document on each `Hub` change
  (`hub.subscribe()`) and at least every `PVHUB_HA_PUBLISH_INTERVAL_S` (default 30)
  via a ticker — same `tokio::select!` pattern as `src/sinks/http/sse.rs`.
- **Disconnect / broker down:** the broker publishes the LWT `offline`, so HA marks
  entities unavailable. The sink keeps driving the rumqttc `EventLoop`, which
  reconnects automatically; on a hard error the loop logs and backs off before
  retrying (same shape as the backoff in `src/scheduler.rs`). On reconnect,
  discovery + `online` + state are re-published.
- QoS: discovery and availability at QoS 1 (AtLeastOnce), state at QoS 0 or 1
  (retained ensures late subscribers get the last value regardless).

## 8. Configuration (new `Config` fields + `from_map` parsing)

All prefixed `PVHUB_`, parsed with the existing `str_or` / `u16_or` / `u64_or` /
`bool_or` helpers. The sink is spawned only when `ha_enable` is true.

| Field | Env var | Type | Default |
|---|---|---|---|
| `ha_enable` | `PVHUB_HA_ENABLE` | bool | `false` |
| `ha_mqtt_host` | `PVHUB_HA_MQTT_HOST` | String | `localhost` |
| `ha_mqtt_port` | `PVHUB_HA_MQTT_PORT` | u16 | `1883` |
| `ha_mqtt_username` | `PVHUB_HA_MQTT_USERNAME` | Option<String> | none |
| `ha_mqtt_password` | `PVHUB_HA_MQTT_PASSWORD` | Option<String> | none |
| `ha_mqtt_client_id` | `PVHUB_HA_MQTT_CLIENT_ID` | String | `pvhub` |
| `ha_discovery_prefix` | `PVHUB_HA_DISCOVERY_PREFIX` | String | `homeassistant` |
| `ha_node_id` | `PVHUB_HA_NODE_ID` | String | `pvhub` |
| `ha_publish_interval_s` | `PVHUB_HA_PUBLISH_INTERVAL_S` | u64 | `30` |

## 9. Wiring (`src/lib.rs`, `run()`)

Add one gated spawn block mirroring the modbus one:

```rust
if cfg.ha_enable {
    let c = cfg.clone();
    let h = hub.clone();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = sinks::homeassistant::serve(c, h).await {
            tracing::error!("home assistant sink error: {e}");
        }
    }));
}
```

## 10. Dependency

Add `rumqttc` (async MQTT client, pure Rust, tokio-based, rustls-capable — matches
the existing `reqwest`/rustls stack). Current `0.24.x`. No other new dependency.

## 11. Testing

Pure builders in `discovery.rs` are unit-tested without a broker (consistent with
the project having no MQTT test broker; HTTP tests use wiremock only):

- **Mapping:** each unit family maps to the expected `(unit_of_measurement,
  device_class, state_class)`, including the `id`-based special cases
  (`rel_humidity`, `poll_errors_total`, `provider_ok`, `last_update_epoch`).
- **Coverage:** iterating `catalog()` yields exactly one discovery entry per metric
  (count == 32), each with a unique `unique_id` and the correct config topic.
- **Sample payload:** for `ghi`, assert config topic
  `homeassistant/sensor/pvhub/ghi/config`, `unit_of_measurement: "W/m²"`,
  `device_class: "irradiance"`, `state_class: "measurement"`,
  `state_topic: "pvhub/pvhub/state"`, `value_template: "{{ value_json.ghi.value }}"`,
  `unique_id: "pvhub_pvhub_ghi"`, and presence of the shared `device` block.
- **Config:** `from_map` defaults and overrides for the new `PVHUB_HA_*` vars
  (extend the existing `config::tests`).

Manual/acceptance check (documented, not automated): with Mosquitto + HA running,
enabling the sink makes a "Solarimetro" device with all sensors appear via
discovery and irradiance graphs populate.

## 12. Documentation

Update `README.md`: add the Home Assistant sink to the sinks section, document the
`PVHUB_HA_*` env vars, and add a `docker-compose` snippet pointing pv-hub at a
Mosquitto broker (and a note that HA's Mosquitto add-on is the typical broker).

## 13. File-change summary

- `Cargo.toml` — add `rumqttc`.
- `src/config.rs` — new `PVHUB_HA_*` fields + parsing + tests.
- `src/sinks/mod.rs` — `pub mod homeassistant;`.
- `src/sinks/homeassistant/mod.rs` — `serve()` + connection/publish loop (new).
- `src/sinks/homeassistant/discovery.rs` — pure builders + mapping + tests (new).
- `src/lib.rs` — gated spawn in `run()`.
- `README.md` — docs + compose example.
- No changes to `src/model.rs` or `src/catalog.rs`.

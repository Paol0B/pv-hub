# Home Assistant Sink Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `homeassistant` output sink that publishes pv-hub's metrics to Home Assistant over MQTT using MQTT Discovery, so HA auto-creates one sensor per metric under a single device and can graph them natively.

**Architecture:** A new self-contained sink module `src/sinks/homeassistant/` follows the existing sink convention (`serve(cfg, hub)` spawned as a gated tokio task in `run()`, mirroring modbus). Pure discovery/topic/mapping builders live in `discovery.rs` (unit-tested, no I/O); `mod.rs` owns the MQTT connection (rumqttc), availability/LWT, and the publish loop (reusing `sinks::http::api::state_json` for the state payload). No change to `catalog.rs` / `model.rs` — active power is published to HA by the user's separate software.

**Tech Stack:** Rust, tokio, rumqttc (async MQTT client), serde_json, chrono. Home Assistant MQTT Discovery.

Reference spec: `docs/superpowers/specs/2026-07-03-home-assistant-sink-design.md`.

---

### Task 1: Config — add `PVHUB_HA_*` fields

**Files:**
- Modify: `src/config.rs` (struct `Config`, `from_map` construction, `tests` module)

- [ ] **Step 1: Write the failing test**

Add this test to the `tests` module in `src/config.rs` (it uses the existing `base()` helper):

```rust
    #[test]
    fn ha_defaults_and_overrides() {
        let c = Config::from_map(&base()).unwrap();
        assert!(!c.ha_enable);
        assert_eq!(c.ha_mqtt_host, "localhost");
        assert_eq!(c.ha_mqtt_port, 1883);
        assert_eq!(c.ha_mqtt_client_id, "pvhub");
        assert_eq!(c.ha_discovery_prefix, "homeassistant");
        assert_eq!(c.ha_node_id, "pvhub");
        assert_eq!(c.ha_publish_interval_s, 30);
        assert!(c.ha_mqtt_username.is_none());
        assert!(c.ha_mqtt_password.is_none());

        let mut m = base();
        m.insert("PVHUB_HA_ENABLE".into(), "true".into());
        m.insert("PVHUB_HA_MQTT_HOST".into(), "broker.local".into());
        m.insert("PVHUB_HA_MQTT_PORT".into(), "8883".into());
        m.insert("PVHUB_HA_MQTT_USERNAME".into(), "user".into());
        m.insert("PVHUB_HA_MQTT_PASSWORD".into(), "secret".into());
        m.insert("PVHUB_HA_NODE_ID".into(), "roof".into());
        m.insert("PVHUB_HA_PUBLISH_INTERVAL_S".into(), "15".into());
        let c = Config::from_map(&m).unwrap();
        assert!(c.ha_enable);
        assert_eq!(c.ha_mqtt_host, "broker.local");
        assert_eq!(c.ha_mqtt_port, 8883);
        assert_eq!(c.ha_mqtt_username.as_deref(), Some("user"));
        assert_eq!(c.ha_mqtt_password.as_deref(), Some("secret"));
        assert_eq!(c.ha_node_id, "roof");
        assert_eq!(c.ha_publish_interval_s, 15);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib config::tests::ha_defaults_and_overrides`
Expected: FAIL to **compile** — `no field ha_enable on type Config` (fields don't exist yet).

- [ ] **Step 3: Add the fields to the `Config` struct**

In `src/config.rs`, add these fields to `pub struct Config` (place them after the `modbus_*` fields, before `default_theme`):

```rust
    pub ha_enable: bool,
    pub ha_mqtt_host: String,
    pub ha_mqtt_port: u16,
    pub ha_mqtt_username: Option<String>,
    pub ha_mqtt_password: Option<String>,
    pub ha_mqtt_client_id: String,
    pub ha_discovery_prefix: String,
    pub ha_node_id: String,
    pub ha_publish_interval_s: u64,
```

- [ ] **Step 4: Parse the fields in `from_map`**

In the `Ok(Config { ... })` block in `from_map`, add these lines (after `modbus_holding_mirror: ...`, before `default_theme: ...`):

```rust
            ha_enable: bool_or("PVHUB_HA_ENABLE", false),
            ha_mqtt_host: str_or("PVHUB_HA_MQTT_HOST", "localhost"),
            ha_mqtt_port: u16_or("PVHUB_HA_MQTT_PORT", 1883)?,
            ha_mqtt_username: env.get("PVHUB_HA_MQTT_USERNAME").cloned(),
            ha_mqtt_password: env.get("PVHUB_HA_MQTT_PASSWORD").cloned(),
            ha_mqtt_client_id: str_or("PVHUB_HA_MQTT_CLIENT_ID", "pvhub"),
            ha_discovery_prefix: str_or("PVHUB_HA_DISCOVERY_PREFIX", "homeassistant"),
            ha_node_id: str_or("PVHUB_HA_NODE_ID", "pvhub"),
            ha_publish_interval_s: u64_or("PVHUB_HA_PUBLISH_INTERVAL_S", 30)?,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib config::tests::ha_defaults_and_overrides`
Expected: PASS. Then `cargo test --lib config` — all config tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add PVHUB_HA_* settings for Home Assistant sink"
```

---

### Task 2: Discovery/topic/mapping builders (pure, unit-tested)

**Files:**
- Create: `src/sinks/homeassistant/mod.rs` (module root — only declares `discovery` in this task)
- Create: `src/sinks/homeassistant/discovery.rs`
- Modify: `src/sinks/mod.rs` (register the module)

- [ ] **Step 1: Register the new module**

In `src/sinks/mod.rs`, add the module declaration (keep the existing `http` and `modbus` lines):

```rust
pub mod homeassistant;
```

- [ ] **Step 2: Create the module root**

Create `src/sinks/homeassistant/mod.rs` with exactly this content for now (the `serve` function is added in Task 4):

```rust
pub mod discovery;
```

- [ ] **Step 3: Write the failing tests + builder signatures**

Create `src/sinks/homeassistant/discovery.rs` with the implementation AND tests below. (Writing both together: the test module references the pure functions in the same file; this compiles only once the functions exist, matching TDD's "write test, then make it pass" within one file creation.)

```rust
use crate::catalog::MetricDef;
use crate::config::Config;
use serde_json::{json, Value};

/// Single JSON state topic all sensors read via `value_template`.
pub fn state_topic(node: &str) -> String {
    format!("pvhub/{node}/state")
}

/// Availability (LWT) topic: `online` / `offline`.
pub fn status_topic(node: &str) -> String {
    format!("pvhub/{node}/status")
}

/// HA MQTT-discovery config topic for one metric.
pub fn config_topic(prefix: &str, node: &str, id: &str) -> String {
    format!("{prefix}/sensor/{node}/{id}/config")
}

/// Map a metric to Home Assistant metadata:
/// `(unit_of_measurement, device_class, state_class)`.
///
/// Catalog units are normalized to HA-accepted units (e.g. `W/m2` -> `W/m²`,
/// `degC` -> `°C`) because HA ignores a `device_class` whose unit is incompatible.
/// A few metrics are special-cased by `id`.
pub fn ha_meta(d: &MetricDef) -> (Option<&'static str>, Option<&'static str>, &'static str) {
    match d.id {
        "rel_humidity" => return (Some("%"), Some("humidity"), "measurement"),
        "poll_errors_total" => return (None, None, "total_increasing"),
        "provider_ok" | "is_daytime" => return (None, None, "measurement"),
        "last_update_epoch" => return (Some("s"), None, "measurement"),
        _ => {}
    }
    match d.unit {
        "W/m2" => (Some("W/m²"), Some("irradiance"), "measurement"),
        "degC" => (Some("°C"), Some("temperature"), "measurement"),
        "m/s" => (Some("m/s"), Some("wind_speed"), "measurement"),
        "hPa" => (Some("hPa"), Some("atmospheric_pressure"), "measurement"),
        "mm" => (Some("mm"), Some("precipitation"), "measurement"),
        "%" => (Some("%"), None, "measurement"),
        "deg" => (Some("°"), None, "measurement"),
        "s" => (Some("s"), Some("duration"), "measurement"),
        _ => (None, None, "measurement"),
    }
}

/// Build the HA MQTT-discovery config payload for one metric.
pub fn discovery_payload(d: &MetricDef, cfg: &Config) -> Value {
    let node = cfg.ha_node_id.as_str();
    let (unit, device_class, state_class) = ha_meta(d);
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), json!(d.label));
    obj.insert("unique_id".into(), json!(format!("pvhub_{node}_{}", d.id)));
    obj.insert("object_id".into(), json!(format!("pvhub_{}", d.id)));
    obj.insert("state_topic".into(), json!(state_topic(node)));
    // The state topic carries the full state_json document, which nests metric
    // values under a top-level `metrics` key — hence `value_json.metrics.<id>.value`.
    obj.insert(
        "value_template".into(),
        json!(format!("{{{{ value_json.metrics.{}.value }}}}", d.id)),
    );
    if let Some(u) = unit {
        obj.insert("unit_of_measurement".into(), json!(u));
    }
    if let Some(dc) = device_class {
        obj.insert("device_class".into(), json!(dc));
    }
    obj.insert("state_class".into(), json!(state_class));
    obj.insert("availability_topic".into(), json!(status_topic(node)));
    obj.insert("payload_available".into(), json!("online"));
    obj.insert("payload_not_available".into(), json!("offline"));
    obj.insert(
        "device".into(),
        json!({
            "identifiers": [node],
            "name": cfg.site_name,
            "manufacturer": "pv-hub",
            "model": "Solarimetro",
        }),
    );
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{catalog, def_for};
    use crate::model::Metric;
    use std::collections::{HashMap, HashSet};

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[test]
    fn topics_use_node_and_prefix() {
        assert_eq!(state_topic("pvhub"), "pvhub/pvhub/state");
        assert_eq!(status_topic("roof"), "pvhub/roof/status");
        assert_eq!(
            config_topic("homeassistant", "pvhub", "ghi"),
            "homeassistant/sensor/pvhub/ghi/config"
        );
    }

    #[test]
    fn unit_mapping_normalizes_and_classes() {
        assert_eq!(ha_meta(def_for(Metric::Ghi).unwrap()), (Some("W/m²"), Some("irradiance"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::AmbientTemp).unwrap()), (Some("°C"), Some("temperature"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::WindSpeed).unwrap()), (Some("m/s"), Some("wind_speed"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::SurfacePressure).unwrap()), (Some("hPa"), Some("atmospheric_pressure"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::Precipitation).unwrap()), (Some("mm"), Some("precipitation"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::SunAzimuth).unwrap()), (Some("°"), None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::DataAge).unwrap()), (Some("s"), Some("duration"), "measurement"));
    }

    #[test]
    fn special_cases_by_id() {
        assert_eq!(ha_meta(def_for(Metric::RelHumidity).unwrap()), (Some("%"), Some("humidity"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::CloudCover).unwrap()), (Some("%"), None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::PollErrorsTotal).unwrap()), (None, None, "total_increasing"));
        assert_eq!(ha_meta(def_for(Metric::ProviderOk).unwrap()), (None, None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::IsDaytime).unwrap()), (None, None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::LastUpdateEpoch).unwrap()), (Some("s"), None, "measurement"));
    }

    #[test]
    fn empty_unit_metric_has_no_unit() {
        // clearsky_index has an empty catalog unit
        assert_eq!(ha_meta(def_for(Metric::ClearskyIndex).unwrap()), (None, None, "measurement"));
        let p = discovery_payload(def_for(Metric::ClearskyIndex).unwrap(), &cfg());
        assert!(p.get("unit_of_measurement").is_none());
        assert!(p.get("device_class").is_none());
    }

    #[test]
    fn one_entry_per_metric_with_unique_ids() {
        let cfg = cfg();
        let mut ids = HashSet::new();
        for d in catalog() {
            let uid = discovery_payload(d, &cfg)["unique_id"].as_str().unwrap().to_string();
            assert!(ids.insert(uid), "duplicate unique_id for {}", d.id);
        }
        assert_eq!(ids.len(), catalog().len());
    }

    #[test]
    fn ghi_payload_shape() {
        let p = discovery_payload(def_for(Metric::Ghi).unwrap(), &cfg());
        assert_eq!(p["state_topic"], "pvhub/pvhub/state");
        assert_eq!(p["value_template"], "{{ value_json.metrics.ghi.value }}");
        assert_eq!(p["unit_of_measurement"], "W/m²");
        assert_eq!(p["device_class"], "irradiance");
        assert_eq!(p["state_class"], "measurement");
        assert_eq!(p["unique_id"], "pvhub_pvhub_ghi");
        assert_eq!(p["object_id"], "pvhub_ghi");
        assert_eq!(p["availability_topic"], "pvhub/pvhub/status");
        assert_eq!(p["payload_not_available"], "offline");
        assert_eq!(p["device"]["identifiers"][0], "pvhub");
        assert_eq!(p["device"]["model"], "Solarimetro");
        assert_eq!(p["device"]["manufacturer"], "pv-hub");
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib sinks::homeassistant::discovery`
Expected: PASS (all 6 tests). If it fails to compile because `serve` is referenced anywhere, ensure `mod.rs` contains only `pub mod discovery;` at this stage.

- [ ] **Step 5: Confirm the whole lib still builds and tests green**

Run: `cargo test --lib`
Expected: PASS (existing tests + the new discovery tests). Warnings about unused `pub` functions are acceptable here — they are consumed in Task 4.

- [ ] **Step 6: Commit**

```bash
git add src/sinks/mod.rs src/sinks/homeassistant/mod.rs src/sinks/homeassistant/discovery.rs
git commit -m "feat(ha): discovery payload + topic + unit->device_class builders"
```

---

### Task 3: Add the rumqttc dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add rumqttc to `[dependencies]`**

In `Cargo.toml`, add this line at the end of the `[dependencies]` section (after `thiserror = "1"`):

```toml
rumqttc = "0.24"
```

Note: default features are kept (pulls a rustls-based TLS transport). MVP connects over plain TCP; broker TLS is a later iteration and needs no code change beyond setting the transport.

- [ ] **Step 2: Verify it resolves and builds**

Run: `cargo build`
Expected: rumqttc (and its deps) download and the crate builds. No code uses it yet — build succeeds.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add rumqttc dependency for the Home Assistant sink"
```

---

### Task 4: The sink itself — `serve()` + connection/publish loop + wiring

**Files:**
- Modify: `src/sinks/homeassistant/mod.rs` (add `serve` + helpers)
- Modify: `src/lib.rs:39-47` (add a gated spawn block in `run()`)

There is no automated test for `serve()` — it requires a live broker and is verified manually (Step 4). The pure logic it depends on is already tested in Task 2.

- [ ] **Step 1: Implement `serve()` in `src/sinks/homeassistant/mod.rs`**

Replace the entire contents of `src/sinks/homeassistant/mod.rs` with:

```rust
pub mod discovery;

use crate::catalog::catalog;
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::http::api::state_json;
use discovery::{config_topic, discovery_payload, state_topic, status_topic};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use std::time::Duration;

/// Request-channel capacity. Must exceed the connect burst (one discovery
/// message per metric + availability + state) so publishing on ConnAck never
/// blocks before the event loop can drain it.
const REQUEST_CAP: usize = 256;

/// Publish a retained discovery config for every metric in the catalog.
async fn publish_discovery(client: &AsyncClient, cfg: &Config) {
    for d in catalog() {
        let topic = config_topic(&cfg.ha_discovery_prefix, &cfg.ha_node_id, d.id);
        let payload = discovery_payload(d, cfg).to_string();
        if let Err(e) = client.publish(topic, QoS::AtLeastOnce, true, payload).await {
            tracing::warn!("ha discovery publish failed for {}: {e}", d.id);
        }
    }
}

/// Publish the current state document (retained) to the single state topic.
async fn publish_state(client: &AsyncClient, cfg: &Config, hub: &Hub) {
    let payload = state_json(hub, cfg).await.to_string();
    if let Err(e) = client
        .publish(state_topic(&cfg.ha_node_id), QoS::AtLeastOnce, true, payload)
        .await
    {
        tracing::warn!("ha state publish failed: {e}");
    }
}

pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let status = status_topic(&cfg.ha_node_id);

    let mut opts = MqttOptions::new(
        cfg.ha_mqtt_client_id.as_str(),
        cfg.ha_mqtt_host.as_str(),
        cfg.ha_mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_last_will(LastWill::new(
        status.clone(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));
    if let Some(user) = &cfg.ha_mqtt_username {
        opts.set_credentials(user.clone(), cfg.ha_mqtt_password.clone().unwrap_or_default());
    }

    let (client, mut eventloop) = AsyncClient::new(opts, REQUEST_CAP);
    tracing::info!(
        "home assistant sink -> mqtt {}:{} (node '{}')",
        cfg.ha_mqtt_host,
        cfg.ha_mqtt_port,
        cfg.ha_node_id
    );

    // Publisher task: push state on every hub change and at least every
    // `ha_publish_interval_s`. Enqueues into the shared request channel; the
    // event loop below actually sends them.
    {
        let client = client.clone();
        let hub = hub.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let mut rx = hub.subscribe();
            let mut ticker = tokio::time::interval(Duration::from_secs(cfg.ha_publish_interval_s));
            loop {
                tokio::select! {
                    _ = rx.recv() => {},
                    _ = ticker.tick() => {},
                }
                publish_state(&client, &cfg, &hub).await;
            }
        });
    }

    // Connection loop: drive the event loop; on each (re)connect republish the
    // retained discovery configs + availability=online + current state. rumqttc
    // reconnects automatically on subsequent polls; back off on hard errors.
    let mut backoff = 1u64;
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                tracing::info!("home assistant mqtt connected");
                publish_discovery(&client, &cfg).await;
                if let Err(e) = client
                    .publish(status.clone(), QoS::AtLeastOnce, true, "online")
                    .await
                {
                    tracing::warn!("ha availability publish failed: {e}");
                }
                publish_state(&client, &cfg, &hub).await;
                backoff = 1;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("ha mqtt event loop error: {e}; reconnecting in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}
```

- [ ] **Step 2: Wire the sink into `run()`**

In `src/lib.rs`, add a gated spawn block immediately after the modbus block (after the closing `}` of `if cfg.modbus_enable { ... }` at line 47, before the `{ let c = cfg.clone(); ... http ... }` block):

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

- [ ] **Step 3: Verify it compiles and all tests pass**

Run: `cargo build && cargo test`
Expected: build succeeds; all tests PASS (no new automated tests in this task, but nothing regresses).

- [ ] **Step 4: Manual acceptance check (documented, not automated)**

Start a local broker and run pv-hub against it:

```bash
# terminal 1 — a throwaway MQTT broker
podman run --rm -p 1883:1883 eclipse-mosquitto:2 \
  mosquitto -c /mosquitto-no-auth.conf

# terminal 2 — watch what pv-hub publishes
mosquitto_sub -h localhost -t 'homeassistant/#' -t 'pvhub/#' -v

# terminal 3 — run pv-hub with the sink enabled
PVHUB_LATITUDE=45.4642 PVHUB_LONGITUDE=9.19 \
PVHUB_HA_ENABLE=true PVHUB_HA_MQTT_HOST=localhost \
  cargo run
```

Expected in terminal 2: ~32 retained `homeassistant/sensor/pvhub/<id>/config` messages, `pvhub/pvhub/status online`, and a `pvhub/pvhub/state {...}` JSON document that refreshes at least every 30 s. Stopping pv-hub (Ctrl-C, terminal 3) makes the broker deliver `pvhub/pvhub/status offline` (the LWT). In a real Home Assistant with MQTT configured, a "Solarimetro" device with all sensors appears automatically and irradiance sensors graph over time.

- [ ] **Step 5: Commit**

```bash
git add src/sinks/homeassistant/mod.rs src/lib.rs
git commit -m "feat(ha): MQTT-discovery Home Assistant sink (serve + wiring)"
```

---

### Task 5: Documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add the config rows**

In `README.md`, in the "Configuration (environment variables)" table, add these rows immediately after the `PVHUB_MODBUS_HOLDING_MIRROR` row (line 59):

```markdown
| `PVHUB_HA_ENABLE` | `false` | enable the Home Assistant MQTT sink |
| `PVHUB_HA_MQTT_HOST` / `_PORT` | `localhost` / `1883` | MQTT broker (HA's Mosquitto add-on) |
| `PVHUB_HA_MQTT_USERNAME` / `_PASSWORD` | — | broker credentials (optional) |
| `PVHUB_HA_MQTT_CLIENT_ID` | `pvhub` | MQTT client id |
| `PVHUB_HA_DISCOVERY_PREFIX` | `homeassistant` | HA MQTT-discovery prefix |
| `PVHUB_HA_NODE_ID` | `pvhub` | topic namespace + HA device id (use a unique id per site) |
| `PVHUB_HA_PUBLISH_INTERVAL_S` | `30` | max interval between state publishes |
```

- [ ] **Step 2: Add a "Home Assistant sink" section**

In `README.md`, add this section immediately after the "HTTP API" section (after line 103, before "## What it computes"):

```markdown
## Home Assistant sink (MQTT Discovery)

Set `PVHUB_HA_ENABLE=true` and point pv-hub at an MQTT broker (Home Assistant's
Mosquitto add-on is the usual one). pv-hub publishes **MQTT Discovery** configs so
Home Assistant auto-creates one sensor per metric under a single **Solarimetro**
device, with correct units and `device_class`/`state_class` (irradiance,
temperature, wind speed, …) so history graphs work out of the box.

- Discovery: `homeassistant/sensor/<node>/<metric>/config` (retained)
- State (one JSON doc, all metrics): `pvhub/<node>/state` (retained)
- Availability (LWT): `pvhub/<node>/status` → `online` / `offline`

`<node>` is `PVHUB_HA_NODE_ID` (default `pvhub`); give each site a unique node id.

pv-hub publishes only its own metrics (irradiance, POA, temperatures, geometry,
meteo, health). Publish your PV **active power** to the same Home Assistant from
your own software, then overlay it with irradiance on one HA chart.

Example compose service:

```yaml
services:
  pv-hub:
    image: pv-hub:0.1
    environment:
      PVHUB_LATITUDE: "45.4642"
      PVHUB_LONGITUDE: "9.19"
      PVHUB_SITE_NAME: "My plant"
      PVHUB_HA_ENABLE: "true"
      PVHUB_HA_MQTT_HOST: "mosquitto"   # broker hostname/service
      PVHUB_HA_MQTT_PORT: "1883"
      PVHUB_HA_NODE_ID: "roof"
    ports:
      - "8080:8080"
```
```

- [ ] **Step 3: Update the "Extending it" note**

In `README.md`, in the "Extending it" list, replace the "Add a sink (e.g. MQTT)" bullet (line 122) with:

```markdown
- **Add a sink:** read the `Hub` snapshot + catalog and publish. See the Modbus, HTTP, and Home Assistant (MQTT) sinks under `src/sinks/` for the pattern — each is a single module.
```

- [ ] **Step 4: Verify the markdown reads correctly**

Run: `git diff README.md`
Expected: the three edits above, no accidental table misalignment. (Optional: preview in an editor.)

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: document the Home Assistant MQTT sink and its env vars"
```

---

## Done criteria

- `cargo test` passes (config + discovery unit tests included).
- With a broker running and `PVHUB_HA_ENABLE=true`, discovery + state + availability messages appear (Task 4 Step 4), and a Solarimetro device with graphable irradiance sensors shows up in Home Assistant.
- README documents the new sink and all `PVHUB_HA_*` variables.
- No changes to `src/model.rs` or `src/catalog.rs`.

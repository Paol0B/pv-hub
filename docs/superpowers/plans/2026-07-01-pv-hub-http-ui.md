# pv-hub — Plan 2: HTTP API + SSE + SCADA UI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. Depends on Plan 1 being complete.

**Goal:** Add an embedded HTTP server (axum) exposing the central state as JSON + SSE, and a responsive dark/light SCADA dashboard (map, sun path, gauges) — all assets embedded in the binary.

**Architecture:** A new `sinks::http` module runs axum alongside the Modbus sink, reading the same `Hub`. State is serialized from the catalog (so new metrics appear automatically). The browser loads embedded HTML/CSS/JS, fetches `/api/state.json` once, then subscribes to `/api/stream` (SSE) for live updates. Leaflet (vendored, no external JS) renders the map; tiles come from a configurable OSM URL.

**Tech Stack:** axum 0.7, tower-http (compression), rust-embed (asset embedding), futures, tokio-stream. Vendored Leaflet.

---

## File structure (Plan 2 additions)

```
src/sinks/http/mod.rs      axum app, router, server task
src/sinks/http/api.rs      state_json() / catalog_json() serialization (+ tests)
src/sinks/http/sse.rs      SSE stream from hub broadcast
assets/index.html          dashboard markup (from approved mockup)
assets/styles.css          semantic-token theme (dark/light)
assets/app.js              fetch + SSE + Leaflet + gauges + theme toggle
assets/vendor/leaflet.css  vendored Leaflet
assets/vendor/leaflet.js   vendored Leaflet
```

---

### Task 1: Dependencies + asset embedding scaffold

**Files:** `Cargo.toml`, `src/sinks/http/mod.rs`, `src/sinks/mod.rs`, `assets/.gitkeep`

- [ ] **Step 1:** Add to `Cargo.toml`:

```toml
axum = "0.7"
tower-http = { version = "0.6", features = ["compression-gzip"] }
rust-embed = "8"
futures = "0.3"
tokio-stream = "0.1"
```

- [ ] **Step 2:** Add `pub mod http;` to `src/sinks/mod.rs`.

- [ ] **Step 3:** Create `src/sinks/http/mod.rs` with the embed struct and module wiring:

```rust
pub mod api;
pub mod sse;

use crate::config::Config;
use crate::hub::Hub;
use axum::response::{Html, IntoResponse, Response};
use axum::http::{header, StatusCode};
use axum::{routing::get, Router};
use rust_embed::RustEmbed;
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub cfg: Arc<Config>,
}

fn asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(f) => {
            let mime = mime_for(path);
            ([(header::CONTENT_TYPE, mime)], f.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".css") { "text/css" }
    else if path.ends_with(".js") { "application/javascript" }
    else if path.ends_with(".html") { "text/html; charset=utf-8" }
    else if path.ends_with(".png") { "image/png" }
    else { "application/octet-stream" }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(String::from_utf8_lossy(&Assets::get("index.html").unwrap().data).into_owned()) }))
        .route("/assets/*path", get(|axum::extract::Path(p): axum::extract::Path<String>| async move { asset(&p) }))
        .route("/api/state.json", get(api::state_handler))
        .route("/api/catalog.json", get(api::catalog_handler))
        .route("/api/stream", get(sse::stream_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
}

pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let addr = format!("{}:{}", cfg.http_bind, cfg.http_port);
    let state = AppState { hub, cfg: Arc::new(cfg) };
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("http/SCADA UI listening on {addr}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
```

- [ ] **Step 4:** Create `assets/.gitkeep` (empty) so the folder exists for the build. Real asset files are added in Task 4.

- [ ] **Step 5:** `cargo build` (will fail until `api.rs`/`sse.rs` exist — created next tasks). Commit after Task 3 compiles.

---

### Task 2: State/catalog JSON serialization

**Files:** `src/sinks/http/api.rs`

- [ ] **Step 1: Write the failing test.** Create `src/sinks/http/api.rs`:

```rust
use crate::catalog::catalog;
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::http::AppState;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde_json::{json, Value};

/// Build the full state document from the catalog + current snapshot.
pub async fn state_json(hub: &Hub, cfg: &Config) -> Value {
    let now = Utc::now();
    let snap = hub.snapshot().await;
    let mut metrics = serde_json::Map::new();
    for d in catalog() {
        metrics.insert(
            d.id.to_string(),
            json!({
                "value": snap.value(d.metric, now),
                "unit": d.unit,
                "label": d.label,
                "category": d.category,
            }),
        );
    }
    json!({
        "site": {
            "name": cfg.site_name,
            "latitude": cfg.latitude,
            "longitude": cfg.longitude,
            "tilt": cfg.tilt_deg,
            "azimuth": cfg.azimuth_deg,
            "albedo": cfg.albedo,
            "default_theme": cfg.default_theme,
        },
        "provider": { "name": cfg.provider, "ok": snap.provider_ok },
        "timestamp": now.timestamp(),
        "metrics": Value::Object(metrics),
    })
}

pub fn catalog_json() -> Value {
    let items: Vec<Value> = catalog().iter().map(|d| json!({
        "id": d.id, "label": d.label, "unit": d.unit,
        "category": d.category, "register": d.register,
    })).collect();
    json!({ "metrics": items })
}

pub async fn state_handler(State(s): State<AppState>) -> Json<Value> {
    Json(state_json(&s.hub, &s.cfg).await)
}

pub async fn catalog_handler() -> Json<Value> {
    Json(catalog_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Metric;
    use std::collections::HashMap;

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[tokio::test]
    async fn state_json_has_metrics_and_site() {
        let hub = Hub::new();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(Utc::now()), Some(true)).await;
        let v = state_json(&hub, &cfg()).await;
        assert_eq!(v["metrics"]["ghi"]["value"], 812.0);
        assert_eq!(v["metrics"]["ghi"]["unit"], "W/m2");
        assert_eq!(v["site"]["latitude"], 45.4642);
        assert_eq!(v["provider"]["ok"], true);
    }

    #[test]
    fn catalog_json_lists_registers() {
        let v = catalog_json();
        assert!(v["metrics"].as_array().unwrap().len() >= 32);
    }
}
```

- [ ] **Step 2-4:** Run `cargo test sinks::http::api::tests` (fails until code pasted → then passes). 2 tests pass.

---

### Task 3: SSE stream

**Files:** `src/sinks/http/sse.rs`

- [ ] **Step 1:** Create `src/sinks/http/sse.rs`:

```rust
use crate::sinks::http::api::state_json;
use crate::sinks::http::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;

/// Emit the current state immediately, then on every hub broadcast, plus a
/// periodic tick so the UI clock/sun position refreshes even without provider updates.
pub async fn stream_handler(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut rx = s.hub.subscribe();
        // initial snapshot
        let v = state_json(&s.hub, &s.cfg).await;
        yield Ok(Event::default().event("state").data(v.to_string()));
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = rx.recv() => {},
                _ = ticker.tick() => {},
            }
            let v = state_json(&s.hub, &s.cfg).await;
            yield Ok(Event::default().event("state").data(v.to_string()));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

Add `async-stream = "0.3"` to `Cargo.toml`.

- [ ] **Step 2:** `cargo build` → clean. `cargo test` → all Plan 1 + api tests pass.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock src/sinks/http assets/.gitkeep src/sinks/mod.rs
git commit -m "feat: http API (state/catalog JSON) + SSE stream"
```

---

### Task 4: SCADA UI assets

**Files:** `assets/index.html`, `assets/styles.css`, `assets/app.js`, `assets/vendor/leaflet.{css,js}`

- [ ] **Step 1: Vendor Leaflet** (no external CDN at runtime):

```bash
mkdir -p assets/vendor
curl -sL https://unpkg.com/leaflet@1.9.4/dist/leaflet.css -o assets/vendor/leaflet.css
curl -sL https://unpkg.com/leaflet@1.9.4/dist/leaflet.js  -o assets/vendor/leaflet.js
```
(If offline, the map degrades gracefully — `app.js` guards on `window.L`.)

- [ ] **Step 2: `assets/index.html`** — port the approved mockup structure from `docs/superpowers/specs/assets/2026-07-01-scada-ui-mockup.html`, but:
  - Move CSS into `assets/styles.css` (`<link rel="stylesheet" href="/assets/styles.css">`), add `<link rel="stylesheet" href="/assets/vendor/leaflet.css">`.
  - Replace hardcoded values with elements carrying `data-metric="<id>"` (e.g. `<span data-metric="ghi">--</span>`), so `app.js` fills them generically from the catalog.
  - Replace the fake `.map` div with `<div id="map"></div>`.
  - Give the POA gauge / sun-path SVG elements stable ids (`#poa-arc`, `#kt-arc`, `#sun-dot`, `#sun-elev`, `#sun-az`, etc.) for JS updates.
  - Add `<script src="/assets/vendor/leaflet.js"></script>` and `<script src="/assets/app.js"></script>` before `</body>`.

- [ ] **Step 3: `assets/styles.css`** — the full semantic-token stylesheet from the approved mockup (`:root[data-theme="dark"]` / `[data-theme="light"]` blocks, layout grid, cards, gauges, tiles, responsive `@media(max-width:900px)` single-column and `@media(max-width:560px)` phone tuning with ≥44px touch targets).

- [ ] **Step 4: `assets/app.js`** — behavior:

```js
// theme: auto (prefers-color-scheme) + toggle persisted in localStorage
(function initTheme(){
  const saved = localStorage.getItem('pvhub-theme');
  const sys = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  document.documentElement.setAttribute('data-theme', saved || sys);
})();
function toggleTheme(){
  const r = document.documentElement;
  const next = r.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
  r.setAttribute('data-theme', next);
  localStorage.setItem('pvhub-theme', next);
}

let map, marker;
function initMap(lat, lon){
  if (!window.L || map) return;
  map = L.map('map', {zoomControl:false, attributionControl:true}).setView([lat,lon], 11);
  const url = window.PVHUB_TILE_URL || 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
  L.tileLayer(url, {maxZoom:19, attribution:'© OpenStreetMap'}).addTo(map);
  marker = L.circleMarker([lat,lon], {radius:9, color:'#ffc24b', fillColor:'#ffc24b', fillOpacity:.9}).addTo(map);
}

function fmt(v, unit){
  if (v === null || v === undefined) return '--';
  const n = Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(1);
  return unit && unit !== '' ? `${n}` : `${n}`;
}

function render(state){
  const m = state.metrics;
  // generic fill of every [data-metric]
  document.querySelectorAll('[data-metric]').forEach(el => {
    const id = el.getAttribute('data-metric');
    if (m[id]) el.textContent = fmt(m[id].value, m[id].unit);
  });
  // gauges
  setArc('poa-arc', m.poa_local?.value, 1200);
  setArc('kt-arc', m.clearsky_index?.value, 1.0);
  // sun path
  updateSun(m.sun_elevation?.value, m.sun_azimuth?.value);
  // badges
  setBadge('provider-badge', state.provider.ok);
  // map
  initMap(state.site.latitude, state.site.longitude);
}

function setArc(id, value, max){ /* set stroke-dasharray on the SVG arc by fraction */ }
function updateSun(elev, az){ /* position #sun-dot along the sky arc; write #sun-elev/#sun-az */ }
function setBadge(id, ok){ const e=document.getElementById(id); if(e) e.classList.toggle('bad', !ok); }

async function boot(){
  const r = await fetch('/api/state.json'); render(await r.json());
  const es = new EventSource('/api/stream');
  es.addEventListener('state', e => render(JSON.parse(e.data)));
}
document.addEventListener('DOMContentLoaded', boot);
```

Fill in `setArc`/`updateSun` with the same geometry used in the mockup SVG (270° arc: `dash = fraction * arcLength`; sun dot along the quadratic path via `t = (az-90)/180` clamped, `y` from elevation).

- [ ] **Step 5: Manual verification.**

Run: `PVHUB_LATITUDE=45.4642 PVHUB_LONGITUDE=9.19 PVHUB_MODBUS_PORT=1502 cargo run`
Open `http://localhost:8080`. Expected: dashboard loads, values populate within seconds, sun path + gauges render, theme toggle flips and persists on reload, map shows the marker, layout collapses to one column when the window is narrowed.

- [ ] **Step 6: Commit**

```bash
git add assets
git commit -m "feat: responsive dark/light SCADA dashboard (map, sun path, gauges, SSE)"
```

---

### Task 5: Wire HTTP server into run()

**Files:** `src/lib.rs`

- [ ] **Step 1:** In `run()`, after spawning the Modbus task, add:

```rust
{
    let c = cfg.clone();
    let h = hub.clone();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = sinks::http::serve(c, h).await {
            tracing::error!("http server error: {e}");
        }
    }));
}
```

- [ ] **Step 2:** `cargo build` → clean. `cargo test` → all pass.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "feat: run HTTP/SCADA server alongside Modbus sink"
```

---

## Self-review

- HTTP JSON + SSE from catalog (auto-includes new metrics) → Tasks 2, 3 ✓
- Responsive dark/light SCADA UI with map, sun path, gauges, theme persistence → Task 4 ✓
- Assets embedded (rust-embed), no runtime file deps; Leaflet vendored, tiles configurable → Tasks 1, 4 ✓
- Runs alongside Modbus, same Hub → Task 5 ✓
- **Note:** `setArc`/`updateSun` geometry must match the mockup SVG; verify visually in Task 4 Step 5.
```

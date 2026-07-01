# pv-hub — Plan 3: Container + compose + README

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. Depends on Plans 1 & 2.

**Goal:** Package pv-hub as a tiny static container image, provide a ready-to-edit `docker-compose.yml`, and document usage in `README.md`.

**Architecture:** Multi-stage build — a Rust builder compiles a static `x86_64-unknown-linux-musl` binary; the runtime stage is `gcr.io/distroless/static` (non-root) containing only the binary (assets are embedded via rust-embed, so no extra files). Compose maps HTTP + Modbus ports and passes site config as env.

**Tech Stack:** Docker/Podman multi-stage build, musl target, distroless.

---

### Task 1: musl static build

**Files:** `Dockerfile`, `.dockerignore`

- [ ] **Step 1:** Add the musl target locally: `rustup target add x86_64-unknown-linux-musl`. Verify a static build works:

```bash
cargo build --release --target x86_64-unknown-linux-musl
```
Expected: `target/x86_64-unknown-linux-musl/release/pv-hub` exists. (rustls is used — no OpenSSL system dep.)

- [ ] **Step 2:** Create `.dockerignore`:

```
target
.git
.superpowers
docs
**/*.md
```

- [ ] **Step 3:** Create `Dockerfile`:

```dockerfile
# ---- builder ----
FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release --target x86_64-unknown-linux-musl

# ---- runtime ----
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/pv-hub /pv-hub
EXPOSE 8080 502
USER nonroot
ENTRYPOINT ["/pv-hub"]
```

- [ ] **Step 4: Build the image.**

```bash
podman build -t pv-hub:0.1 .
```
Expected: image builds; `podman images pv-hub` shows a small image (tens of MB).

- [ ] **Step 5: Smoke test the container.**

```bash
podman run --rm -e PVHUB_LATITUDE=45.4642 -e PVHUB_LONGITUDE=9.19 \
  -e PVHUB_MODBUS_PORT=1502 -p 8080:8080 -p 1502:1502 pv-hub:0.1 &
sleep 8
curl -sf http://localhost:8080/health && echo " <- health ok"
curl -s http://localhost:8080/api/state.json | head -c 200; echo
podman stop -l
```
Expected: `ok <- health ok`, and JSON containing site + metrics (weather values may still be null in the first seconds — solar geometry should already be present).

- [ ] **Step 6: Commit**

```bash
git add Dockerfile .dockerignore
git commit -m "build: multi-stage musl/distroless container image"
```

---

### Task 2: docker-compose

**Files:** `docker-compose.yml`

- [ ] **Step 1:** Create `docker-compose.yml`:

```yaml
services:
  solarimetro-milano:
    image: pv-hub:0.1
    build: .
    restart: unless-stopped
    ports:
      - "8080:8080"     # SCADA web UI
      - "502:502"       # Modbus TCP
    # Binding 502 needs the net-bind capability when running non-root;
    # alternatively set PVHUB_MODBUS_PORT=1502 and map "1502:1502".
    cap_add:
      - NET_BIND_SERVICE
    environment:
      PVHUB_SITE_NAME: "Impianto Demo — Milano"
      PVHUB_LATITUDE: "45.4642"
      PVHUB_LONGITUDE: "9.1900"
      PVHUB_TILT_DEG: "30"
      PVHUB_AZIMUTH_DEG: "180"       # 180 = South
      PVHUB_ALBEDO: "0.20"
      # PVHUB_CELLTEMP: "faiman"
      # PVHUB_POLL_INTERVAL_S: "600"
      # PVHUB_MODBUS_WORD_ORDER: "abcd"   # or cdab for many PLCs
      # PVHUB_OPENMETEO_API_KEY: ""       # for the commercial Open-Meteo plan
      PVHUB_DEFAULT_THEME: "auto"
    healthcheck:
      test: ["CMD", "/pv-hub", "--healthcheck"]   # see Task 3
      interval: 30s
      timeout: 3s
      retries: 3
```

- [ ] **Step 2: Verify compose config parses.** `podman-compose config` (or `docker compose config`) → prints the resolved config without error.

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "build: docker-compose example for a single site"
```

---

### Task 3: `--healthcheck` flag (distroless has no curl)

**Files:** `src/main.rs`

- [ ] **Step 1:** distroless/static has no shell or curl, so the healthcheck must be the binary itself. Add a `--healthcheck` mode to `src/main.rs` that hits the local HTTP `/health` and exits 0/1:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--healthcheck") {
        let port = std::env::var("PVHUB_HTTP_PORT").unwrap_or_else(|_| "8080".into());
        let url = format!("http://127.0.0.1:{port}/health");
        let ok = reqwest::get(&url).await.map(|r| r.status().is_success()).unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }
    pv_hub::run().await
}
```

- [ ] **Step 2:** `cargo build` → clean. Rebuild image (`podman build -t pv-hub:0.1 .`) and confirm `podman run ... pv-hub:0.1 --healthcheck` exits non-zero when the server is down and 0 when up (test by running the healthcheck inside a started container via `podman exec`).

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: --healthcheck mode for container health probe"
```

---

### Task 4: README

**Files:** `README.md` (replace the empty `Readme`)

- [ ] **Step 1:** Write `README.md` covering:
  - What pv-hub is (one-paragraph pitch: free-API solarimetro → Modbus + SCADA).
  - Quick start: `podman build` + `docker-compose up`, then open `http://localhost:8080`.
  - Full `PVHUB_*` env var table (from the spec §7).
  - Modbus register map table (from the spec §5) + word-order note.
  - API endpoints (`/api/state.json`, `/api/stream`, `/api/catalog.json`, `/health`).
  - Screenshot/description of the dashboard.
  - "How to extend" — add a metric (enum + catalog line), add a provider (impl `Provider`), add a sink (read Hub + catalog); MQTT noted as the next sink.
  - Data attribution: "Weather data by Open-Meteo.com (CC-BY 4.0); free tier is non-commercial — set `PVHUB_OPENMETEO_API_KEY` for the commercial plan."
  - License.

- [ ] **Step 2:** Remove the old empty `Readme` file (`git rm Readme`) to avoid two readme files.

- [ ] **Step 3: Commit**

```bash
git add README.md
git rm Readme
git commit -m "docs: project README with usage, env, Modbus map, extension guide"
```

---

## Self-review

- Tiny static image (musl → distroless, non-root) → Task 1 ✓
- Compose with env config, port mapping, 502 capability note, healthcheck → Task 2 ✓
- Healthcheck works without a shell (binary self-probe) → Task 3 ✓
- README: usage, env, register map, API, extension guide, data attribution/license → Task 4 ✓
- **Verify:** image size and container smoke test (Task 1 Steps 4-5) are the acceptance gate for "finished product".
```

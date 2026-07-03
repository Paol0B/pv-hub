---
name: verify
description: Build, run and visually verify the pv-hub dashboard end-to-end
---

# Verifying pv-hub

## Build & run

```bash
cargo build
PVHUB_LATITUDE=44.913 PVHUB_LONGITUDE=8.617 PVHUB_SITE_NAME="Alessandria Test" \
  PVHUB_HTTP_PORT=8091 PVHUB_MODBUS_PORT=15020 ./target/debug/pv-hub &
```

Debug builds serve `assets/` live from disk (rust-embed); release/container builds
embed them at compile time, so a rebuild is needed there after asset changes.

## Drive the surface

```bash
curl -s http://127.0.0.1:8091/api/state.json | head -c 400   # live metrics
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8091/ # dashboard
```

Screenshots: no native browser on this host — use **flatpak Chromium**:

```bash
flatpak run --filesystem=$PWD org.chromium.Chromium --headless --disable-gpu \
  --hide-scrollbars --force-prefers-reduced-motion --window-size=1512,1100 \
  --timeout=30000 --screenshot=$PWD/.superpowers/shots/desktop.png http://127.0.0.1:8091/
```

Gotchas learned the hard way:

- **Flatpak namespaces `/tmp` privately** — a `--screenshot` path under `/tmp` silently
  vanishes. Write inside the repo (`.superpowers/` is gitignored) with `--filesystem=$PWD`.
- **Only one Chromium instance runs at a time** — a hung headless instance makes later
  invocations exit ~instantly (code 144). `pkill -f chromium` first.
- **JS starts very late headless (SwiftShader cold start, up to ~25s)** — use
  `--timeout=30000`, and pass `--force-prefers-reduced-motion` so the 0.5s CSS
  transitions (sun dot, gauge arcs) don't get captured mid-flight.
- SSE keeps the page loading forever; `--timeout` is what actually fires the capture.
- Mobile check: `--window-size=390,1700` (sidebar → burger, cards stack).

At night the site shows: kt `--` (null), POA ~0–3 W/m², grey sun dot on the horizon
at the azimuth compass point, "notte" label. That's correct, not a rendering bug.

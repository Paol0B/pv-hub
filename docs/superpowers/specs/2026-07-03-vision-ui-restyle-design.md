# Vision UI restyle — design note (2026-07-03)

User request: restyle the dashboard to match the **Vision UI Dashboard** (Creative Tim,
React/MUI) dark glassmorphic style, "uguale in tutto e per tutto" (reference screenshot
provided). Executed autonomously; this note records the mapping and the decisions.

Screenshot of the result: [`assets/2026-07-03-vision-ui-dashboard.png`](assets/2026-07-03-vision-ui-dashboard.png)

## Decisions

- **Plain HTML/CSS/JS kept** — the reference is built with React + MUI, but pv-hub serves
  static assets from a Rust binary; the *style* is reproduced 1:1 in CSS without adding a
  JS build pipeline. All SSE/render logic and `data-metric` hooks preserved.
- **Dark-only** — Vision UI has no light mode; the light theme and toggle were removed.
  `PVHUB_DEFAULT_THEME` stays in the config/API for compatibility but is ignored.
- **Font vendored** — Plus Jakarta Sans (variable, latin subset, woff2) self-hosted at
  `assets/vendor/fonts/`, matching the Leaflet vendoring pattern; works on LAN-only sites.
- **Emoji icons → inline SVG sprite** (Lucide-style strokes), per UI/UX guidance.
- **Palette** — Vision UI tokens: blue `#0075FF`, cyan `#21D4FD`, green `#01B574`, amber
  `#FFB547`, red `#E31A1A`, text grey `#A0AEC0`, body tri-gradient `#0F123B→#090D2E→#020515`,
  card gradient `rgba(6,11,40,.94)→rgba(10,14,35,.49)` + `backdrop-filter: blur(60px)`,
  radius 20/15/12. Data-color pair (blue+cyan) CVD/contrast validated against the dark
  surface (dataviz skill validator); green/amber/red reserved as status colors.

## Layout mapping (Vision UI → pv-hub)

| Vision UI | pv-hub |
|---|---|
| Sidebar brand + nav + "Need help?" card | PV-HUB brand, Dashboard/API links, README help card |
| Navbar breadcrumb + search/user | breadcrumb + Provider/Modbus/Live/data-age pills |
| 4 mini stat cards (Today's Money…) | POA locale (+Δ%), Temp. modulo, Vento, Nuvole |
| Welcome card (jellyfish image) | Site card — info left, satellite map right |
| Satisfaction Rate (ring + overlay pill) | Serenità del cielo — kt ring, 0…1 overlay pill |
| Referral Tracking (tiles + green ring) | Irraggiamento POA — provider/extra/air-mass tiles + green POA ring |
| Sales overview (area chart) | Percorso del sole — gridlines, gradient area, glowing sun dot |
| Active Users (bars panel + 4 mini stats) | Componenti irraggiamento — GHI/DNI/DHI/POA white bars + mini stats |
| Projects table | Condizioni meteo table (sensor / value / level bar) |
| Orders overview timeline | Salute sistema timeline (update age, provider, errors, Modbus, SSE) |

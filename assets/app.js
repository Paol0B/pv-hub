"use strict";

// ---- sidenav (mobile) ----
function toggleSidenav(force) {
  document.body.classList.toggle("nav-open", force);
}
window.toggleSidenav = toggleSidenav;

// ---- number formatting ----
function fmt(v) {
  if (v === null || v === undefined || Number.isNaN(v)) return "--";
  const a = Math.abs(v);
  if (a >= 100) return v.toFixed(0);
  if (a >= 10) return v.toFixed(1);
  return v.toFixed(2);
}
function cardinal(deg) {
  const dirs = ["N", "NE", "E", "SE", "S", "SO", "O", "NO"];
  return dirs[Math.round(((deg % 360) / 45)) % 8];
}
function fmtAge(sec) {
  if (sec === null || sec === undefined) return "--";
  sec = Math.round(sec);
  if (sec < 60) return sec + "s fa";
  const m = Math.floor(sec / 60), s = sec % 60;
  if (m < 60) return `${m}m ${s}s fa`;
  return `${Math.floor(m / 60)}h ${m % 60}m fa`;
}

// ---- SVG helpers ----
// 270° ring gauge (rotate 135): fill the arc and park the glow knob on its end.
function setArc(id, value, max) {
  const el = document.getElementById(id);
  if (!el || value === null || value === undefined) return;
  const r = +el.getAttribute("r");
  const arc270 = 2 * Math.PI * r * 0.75;
  const frac = Math.max(0, Math.min(1, value / max));
  el.setAttribute("stroke-dasharray", `${(frac * arc270).toFixed(1)} 9999`);
  el.style.opacity = frac < 0.01 ? "0" : "1"; // hide stray cap-dot near zero
  const knob = document.getElementById(id + "-knob");
  if (knob) {
    const cx = +el.getAttribute("cx"), cy = +el.getAttribute("cy");
    const a = (135 + frac * 270) * Math.PI / 180; // screen coords, y-down
    knob.setAttribute("cx", (cx + r * Math.cos(a)).toFixed(1));
    knob.setAttribute("cy", (cy + r * Math.sin(a)).toFixed(1));
    knob.setAttribute("opacity", frac < 0.01 ? "0" : "1");
  }
}
function setBar(id, value, max, offset) {
  const el = document.getElementById(id);
  if (!el || value === null || value === undefined) return;
  const frac = ((value - (offset || 0)) / (max - (offset || 0))) * 100;
  el.style.width = Math.max(0, Math.min(100, frac)).toFixed(0) + "%";
}
function setVBar(id, value, max) {
  const el = document.getElementById(id);
  if (!el || value === null || value === undefined) return;
  el.style.height = Math.max(0, Math.min(100, (value / max) * 100)).toFixed(0) + "%";
}

// Sky dome: x from azimuth (E=left, S=middle, W=right), y from elevation
// (horizon at y=150). At night the sun sits dimmed on the horizon.
function updateSun(elev, az) {
  const dot = document.getElementById("sun-dot");
  const glow = document.getElementById("sun-glow");
  const et = document.getElementById("sun-elev");
  const at = document.getElementById("sun-az");
  const label = document.getElementById("sun-label");
  if (et) et.textContent = fmt(elev);
  if (at) at.textContent = fmt(az);
  if (!dot || elev === null || elev === undefined) return;

  const day = elev > 0;
  const fx = Math.max(0, Math.min(1, ((az ?? 180) - 90) / 180)); // 90°E→0 .. 270°W→1
  const x = 20 + fx * 480;
  const y = day ? 150 - (Math.min(90, elev) / 90) * 130 : 150;

  for (const e of [dot, glow]) {
    if (!e) continue;
    e.setAttribute("cx", x.toFixed(1));
    e.setAttribute("cy", y.toFixed(1));
  }
  dot.setAttribute("fill", day ? "#FFB547" : "#718096");
  dot.setAttribute("r", day ? "9" : "6");
  if (day) dot.setAttribute("filter", "url(#glowAmber)");
  else dot.removeAttribute("filter");
  glow.style.opacity = day ? ".4" : "0";
  if (label) label.textContent = day ? "giorno" : "notte";
}

// ---- Leaflet map ----
let map, marker;
function initMap(lat, lon) {
  if (!window.L || map || lat === undefined || lon === undefined) return;
  map = L.map("map", {
    zoomControl: true, attributionControl: false, scrollWheelZoom: false,
  }).setView([lat, lon], 16);
  // Satellite base (Esri World Imagery — free, no API key). Attribution is
  // shown discreetly in the page footer, not over the map.
  L.tileLayer(
    "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}",
    { maxZoom: 19 }
  ).addTo(map);
  // Place & road labels overlay → complete hybrid view
  L.tileLayer(
    "https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}",
    { maxZoom: 19, opacity: 0.9 }
  ).addTo(map);
  marker = L.circleMarker([lat, lon], {
    radius: 9, color: "#0075FF", weight: 3, fillColor: "#0075FF", fillOpacity: 0.9,
  }).addTo(map);
}

// ---- render ----
function num(m, id) {
  return m[id] ? m[id].value : null;
}
function render(state) {
  const m = state.metrics;

  document.querySelectorAll("[data-metric]").forEach((el) => {
    const id = el.getAttribute("data-metric");
    if (m[id]) el.textContent = fmt(m[id].value);
  });

  // site
  document.getElementById("site-name").textContent = state.site.name;
  document.getElementById("site-coords").textContent =
    `${state.site.latitude.toFixed(4)}° · ${state.site.longitude.toFixed(4)}° · tilt ${state.site.tilt}°`;
  const az = num(m, "azimuth");
  const azf = document.getElementById("azimuth-field");
  if (azf && az !== null) azf.textContent = `${az.toFixed(0)}° · ${cardinal(az)}`;

  // POA delta: green when local ≥ provider, red otherwise
  const delta = num(m, "poa_delta_pct");
  const dEl = document.getElementById("poa-delta");
  if (dEl && delta !== null) dEl.className = delta < 0 ? "down" : "up";

  // wind direction (cardinal)
  const wd = num(m, "wind_direction");
  for (const id of ["wind-dir", "wind-dir-cell"]) {
    const el = document.getElementById(id);
    if (el && wd !== null) el.textContent = cardinal(wd);
  }

  // gauges
  setArc("poa-arc", num(m, "poa_local"), 1200);
  setArc("kt-arc", num(m, "clearsky_index"), 1.0);

  // irradiance bars + tracks
  setVBar("vb-ghi", num(m, "ghi"), 1000);
  setVBar("vb-dni", num(m, "dni"), 1000);
  setVBar("vb-dhi", num(m, "dhi"), 400);
  setVBar("vb-poa", num(m, "poa_local"), 1200);
  setBar("tk-ghi", num(m, "ghi"), 1000);
  setBar("tk-dni", num(m, "dni"), 1000);
  setBar("tk-dhi", num(m, "dhi"), 400);
  setBar("tk-poa", num(m, "poa_local"), 1200);

  // weather table tracks
  setBar("tb-hum", num(m, "rel_humidity"), 100);
  setBar("tb-cloud", num(m, "cloud_cover"), 100);
  setBar("tb-wind", num(m, "wind_speed"), 20);
  setBar("tb-rain", num(m, "precipitation"), 10);
  setBar("tb-press", num(m, "surface_pressure"), 1050, 950);

  // kt label
  const kt = num(m, "clearsky_index");
  const ktl = document.getElementById("kt-label");
  if (ktl) {
    if (kt === null) ktl.textContent = "—";
    else if (kt >= 0.75) ktl.textContent = "Cielo sereno";
    else if (kt >= 0.4) ktl.textContent = "Parzialmente nuvoloso";
    else ktl.textContent = "Coperto";
  }

  // sun path
  updateSun(num(m, "sun_elevation"), num(m, "sun_azimuth"));

  // health / provider
  const ok = state.provider.ok;
  document.getElementById("provider-badge").classList.toggle("bad", !ok);
  document.getElementById("provider-field").textContent =
    `${state.provider.name} · ${ok ? "operativo" : "errore"}`;
  const age = num(m, "data_age");
  document.getElementById("dataage-field").textContent = fmtAge(age);

  // map
  initMap(state.site.latitude, state.site.longitude);
}

// ---- boot ----
async function boot() {
  try {
    const r = await fetch("/api/state.json");
    render(await r.json());
  } catch (e) { /* ignore, SSE will fill in */ }
  const es = new EventSource("/api/stream");
  es.addEventListener("state", (e) => {
    try { render(JSON.parse(e.data)); } catch (_) {}
  });
}
document.addEventListener("DOMContentLoaded", boot);

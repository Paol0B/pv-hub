"use strict";

// ---- theme: auto (prefers-color-scheme) + toggle persisted ----
(function initTheme() {
  const saved = localStorage.getItem("pvhub-theme");
  const sys = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  const theme = saved || sys;
  document.documentElement.setAttribute("data-theme", theme);
  setToggleIcon(theme);
})();
function setToggleIcon(theme) {
  const b = document.getElementById("tg");
  if (b) b.textContent = theme === "dark" ? "🌙" : "☀️";
}
function toggleTheme() {
  const r = document.documentElement;
  const next = r.getAttribute("data-theme") === "dark" ? "light" : "dark";
  r.setAttribute("data-theme", next);
  localStorage.setItem("pvhub-theme", next);
  setToggleIcon(next);
}
window.toggleTheme = toggleTheme;

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
function setArc(id, value, max) {
  const el = document.getElementById(id);
  if (!el || value === null || value === undefined) return;
  const r = +el.getAttribute("r");
  const arc270 = 2 * Math.PI * r * 0.75;
  const frac = Math.max(0, Math.min(1, value / max));
  el.setAttribute("stroke-dasharray", `${(frac * arc270).toFixed(1)} 9999`);
}
function setBar(id, value, max) {
  const el = document.getElementById(id);
  if (!el || value === null || value === undefined) return;
  el.style.width = Math.max(0, Math.min(100, (value / max) * 100)).toFixed(0) + "%";
}
// quadratic bezier P0(20,150) P1(260,-38) P2(500,150)
function bezier(t) {
  const mt = 1 - t;
  const x = mt * mt * 20 + 2 * mt * t * 260 + t * t * 500;
  const y = mt * mt * 150 + 2 * mt * t * -38 + t * t * 150;
  return [x, y];
}
function updateSun(elev, az) {
  const dot = document.getElementById("sun-dot");
  const glow = document.getElementById("sun-glow");
  const et = document.getElementById("sun-elev");
  const at = document.getElementById("sun-az");
  if (et) et.textContent = fmt(elev);
  if (at) at.textContent = fmt(az);
  if (!dot) return;
  if (elev === null || elev === undefined) return;
  const visible = elev > 0;
  const t = Math.max(0, Math.min(1, ((az ?? 180) - 90) / 180));
  const [x, y] = bezier(t);
  for (const e of [dot, glow]) {
    if (!e) continue;
    e.setAttribute("cx", x.toFixed(1));
    e.setAttribute("cy", y.toFixed(1));
    e.style.opacity = visible ? (e.id === "sun-glow" ? ".35" : "1") : "0";
  }
}

// ---- Leaflet map ----
let map, marker;
function initMap(lat, lon) {
  if (!window.L || map || lat === undefined || lon === undefined) return;
  map = L.map("map", { zoomControl: false, attributionControl: true }).setView([lat, lon], 12);
  L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
    maxZoom: 19, attribution: "© OpenStreetMap",
  }).addTo(map);
  marker = L.circleMarker([lat, lon], {
    radius: 9, color: "#ffc24b", weight: 3, fillColor: "#ffc24b", fillOpacity: 0.9,
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

  // gauges + bars
  setArc("poa-arc", num(m, "poa_local"), 1200);
  setArc("kt-arc", num(m, "clearsky_index"), 1.0);
  setBar("bar-ghi", num(m, "ghi"), 1000);
  setBar("bar-dni", num(m, "dni"), 1000);
  setBar("bar-dhi", num(m, "dhi"), 400);

  // kt label
  const kt = num(m, "clearsky_index");
  const ktl = document.getElementById("kt-label");
  if (ktl) {
    if (kt === null) ktl.textContent = "—";
    else if (kt >= 0.75) ktl.textContent = "Cielo sereno";
    else if (kt >= 0.4) ktl.textContent = "Parz. nuvoloso";
    else ktl.textContent = "Coperto";
  }

  // sun path
  updateSun(num(m, "sun_elevation"), num(m, "sun_azimuth"));

  // health / provider
  const ok = state.provider.ok;
  document.getElementById("provider-badge").classList.toggle("bad", !ok);
  document.getElementById("provider-field").textContent = state.provider.name;
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

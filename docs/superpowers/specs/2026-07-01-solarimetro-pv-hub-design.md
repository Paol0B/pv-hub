# pv-hub — Solarimetro microservice · Design

**Data:** 2026-07-01
**Stato:** Approvato in brainstorming, in attesa di review finale prima del piano di implementazione
**Mockup UI approvato:** [`assets/2026-07-01-scada-ui-mockup.html`](assets/2026-07-01-scada-ui-mockup.html)

---

## 1. Obiettivo

`pv-hub` è un microservizio containerizzato (Docker/Podman) estremamente leggero che:

1. Interroga API meteo/solari gratuite (a partire da **Open-Meteo**) per una singola posizione geografica.
2. Calcola in locale grandezze utili alla **diagnosi di un impianto fotovoltaico** (posizione solare, irraggiamento sul piano inclinato/POA, temperatura di modulo, indice di serenità).
3. Espone tutti i valori via **Modbus TCP** (slave/server) per PLC/SCADA.
4. Serve una **dashboard SCADA web** professionale, moderna, responsive (desktop + smartphone), con mappa, posizione solare e tutti i valori in tempo reale.

La configurazione di un'istanza è essenzialmente **le coordinate** (più orientamento pannello), passate come **variabili d'ambiente** nel compose. **Un sito per container**; più siti = più container.

### Principio architetturale portante

Tutti i dati raccolti/calcolati confluiscono in **un'unica struttura dati centralizzata** (`SolarState`) descritta da un **catalogo di metriche**. Ogni "sink" (Modbus oggi, MQTT domani, SSE per la UI) **deriva dal catalogo**: aggiungere una metrica o un canale di uscita è una modifica localizzata. Questo è il requisito di scalabilità/espandibilità richiesto.

---

## 2. Decisioni prese (brainstorming)

| Tema | Decisione |
|---|---|
| Set di grandezze | **Completo diagnostico**: GHI, DNI, DHI, POA, posizione solare, temp ambiente/modulo, vento, umidità, nuvole, precipitazioni, alba/tramonto |
| Configurazione impianto | **Singolo orientamento** (un tilt + azimuth) |
| Interfaccia Modbus | **Modbus TCP slave** |
| Licenza fonti | **Design agnostico** con API key/endpoint opzionali via env (free di default, piano commerciale attivabile) |
| Calcolo POA | **Locale (trasposizione) + valore provider**, mostrati insieme con Δ% come cross-check diagnostico |
| Storico | **Nessuna persistenza**: solo stato istantaneo in RAM (Open-Meteo fornisce comunque ore passate se servissero) |
| Realtime UI | **Server-Sent Events (SSE)** |
| Stack | **Rust** (`axum` + `tokio` + `tokio-modbus`), asset UI embeddati, immagine `distroless/static` |
| UI | Command-center a griglia, **responsive/mobile-first**, palette premium **emerald + oro solare**, **dark/light** con default `auto` + toggle memorizzato |

---

## 3. Architettura

```
PROVIDERS (pluggable)          HUB CENTRALE                 SINK (derivano dal catalogo)
┌───────────────────┐          ┌────────────────────┐        ┌────────────────────────┐
│ OpenMeteoProvider │──poll──▶ │ SolarState         │ ──────▶│ ModbusServer (TCP)     │
│ (+ NASA/PVGIS fut.)│         │ Arc<RwLock> + bcast│ ──────▶│ HttpApi (SSE+JSON+UI)  │
│ SolarEngine (calc) │──calc─▶ │ + Metric Catalog   │ ┈┈┈┈┈▶ │ MqttSink (futuro)      │
└───────────────────┘          └────────────────────┘        └────────────────────────┘
        ▲                                ▲
     Scheduler: meteo ogni POLL_INTERVAL_S · sole/POA ogni SOLAR_INTERVAL_S
```

### Componenti

- **`model` — `SolarState`**: snapshot corrente. Ogni grandezza è `Option<f64>` (assente finché ignota), più `updated_at` per gruppo, flag di qualità (`data_stale`), esiti provider (`provider_ok`, `poll_errors_total`). Vive dietro `Arc<RwLock<SolarState>>`. Un `tokio::sync::broadcast` notifica i sink ad ogni aggiornamento (alimenta l'SSE).
- **`catalog` — Metric Catalog**: tabella statica che descrive **una volta sola** ogni metrica: `id`, etichetta, unità, categoria, encoding Modbus (registro base, tipo `f32`/`u32`, scala), futuro sub-topic MQTT, funzione di estrazione dal `SolarState`. Modbus, UI e MQTT non hanno liste proprie: derivano da qui. Un test verifica assenza di collisioni tra registri.
- **`providers` — trait `Provider`**: `async fn poll(&self) -> Result<Vec<Sample>>`. `OpenMeteoProvider` lo implementa. Aggiungerne uno = nuovo file + registrazione.
- **`solar` — SolarEngine**: matematica pura (nessuna rete), gira ogni `SOLAR_INTERVAL_S`. Produce posizione solare, geometria, POA locale, temperatura modulo, clear-sky/kt.
- **`sinks`**: `ModbusServer` e `HttpApi` (axum: JSON, SSE, UI statica embeddata). Ogni sink legge Hub + catalogo.
- **`scheduler`**: due task periodici (meteo lento, sole veloce) → aggiornano l'Hub → broadcast.

### Perché scala / è leggero

- Nuova metrica = 1 riga nel catalogo → appare in Modbus, UI, MQTT.
- Nuovo sink = 1 modulo che legge Hub + catalogo.
- Nuovo provider = implementa un trait.
- Un solo binario statico, stato in RAM, nessun DB, UI embeddata → immagine ~10-20 MB, RAM di pochi MB.

---

## 4. Motore solare (matematica)

1. **Posizione solare** — algoritmo NREL SPA (crate `spa`) da lat/lon/quota + timestamp UTC → zenith, elevazione, azimuth. Derivati: AOI sul pannello, air mass, irradianza extraterrestre, alba/mezzogiorno/tramonto, `is_daytime`.
   - `cos(AOI) = cos(zenith)·cos(β) + sin(zenith)·sin(β)·cos(γ_sole − γ_pannello)`, con β = tilt, γ = azimuth pannello.
2. **Componenti da provider** — Open-Meteo fornisce GHI, DNI, DHI separati → nessun modello di decomposizione necessario.
3. **POA locale — trasposizione Hay-Davies** (default; Perez opzionale):
   - Diretta: `DNI · cos(AOI)`
   - Diffusa cielo: Hay-Davies (circumsolare anisotropa + isotropa)
   - Riflessa suolo: `GHI · albedo · (1 − cos β)/2`
   - `POA = diretta + diffusa + riflessa`
4. **Cross-check provider** — richiesta a Open-Meteo di `global_tilted_irradiance` (tilt/azimuth) → si espongono `POA_local`, `POA_provider`, `POA_delta_pct`.
5. **Temperatura di modulo** — modello **Faiman** (default): `T_cell = T_amb + POA / (U0 + U1·vento)`, U0=25, U1=6.84 configurabili; fallback **NOCT**.
6. **Indice di serenità (kt)** — `GHI_misurato / GHI_clear-sky` (modello Haurwitz), indicatore diagnostico "nuvoloso vs problema d'impianto".

---

## 5. Mappa registri Modbus

- **Modbus TCP slave**, default `:502`, unit id `1` (configurabili).
- Ogni grandezza = **float32 IEEE-754 su 2 registri**, ordine parole **`abcd`** default, **`cdab`/word-swap configurabile**.
- Valori come **Input Register (FC04)** e **mirror sola-lettura Holding Register (FC03)** agli stessi offset (disattivabile) per massima compatibilità.
- **Generata dal catalogo**, con "buchi" tra i blocchi per espandere senza rimappare.

| Off | Metrica | Unità | | Off | Metrica | Unità |
|---|---|---|---|---|---|---|
| **A · Irradianza** | | | | **D · Meteo** | | |
| 0 | GHI | W/m² | | 60 | wind_speed | m/s |
| 2 | DNI | W/m² | | 62 | wind_direction | ° |
| 4 | DHI | W/m² | | 64 | rel_humidity | % |
| 6 | POA_local | W/m² | | 66 | cloud_cover | % |
| 8 | POA_provider | W/m² | | 68 | precipitation | mm |
| 10 | POA_delta_pct | % | | 70 | surface_pressure | hPa |
| 12 | clearsky_GHI | W/m² | | **E · Sito (echo config)** | | |
| 14 | clearsky_index (kt) | – | | 90 | latitude | ° |
| 16 | extraterrestrial | W/m² | | 92 | longitude | ° |
| **B · Geometria solare** | | | | 94 | tilt | ° |
| 30 | sun_elevation | ° | | 96 | azimuth | ° |
| 32 | sun_azimuth | ° | | 98 | albedo | – |
| 34 | sun_zenith | ° | | **F · Salute/stato** | | |
| 36 | aoi | ° | | 110 | data_age | s |
| 38 | air_mass | – | | 112 | last_update_epoch (u32) | s |
| 40 | is_daytime (0/1) | – | | 114 | provider_ok (0/1) | – |
| **C · Temperatura** | | | | 116 | poll_errors_total | – |
| 50 | ambient_temp | °C | | | | |
| 52 | module_temp | °C | | | | |

**Bonus opzionale:** flag booleani (`is_daytime`, `provider_ok`, `data_stale`) anche come **Discrete Input (FC02)**.

---

## 6. Interfaccia SCADA (web)

Riferimento visivo: [`assets/2026-07-01-scada-ui-mockup.html`](assets/2026-07-01-scada-ui-mockup.html).

### Layout (command-center, responsive/mobile-first)

- **Top bar**: logo, nome sito, coordinate, badge di stato live (provider, Modbus, live/SSE, freschezza meteo), **toggle tema 🌙/☀️**.
- **Colonna sx**: mappa (Leaflet + OpenStreetMap, marker sulle coordinate; JS/CSS Leaflet bundlati localmente, tile URL configurabile), scheda impianto (tilt/azimuth/albedo/AOI), gauge **indice serenità kt**.
- **Colonna centrale**: hero **POA** (gauge radiale) con confronto **locale vs provider + Δ%**, bar-meter GHI/DNI/DHI, **percorso del sole** (arco cielo con posizione attuale, alba/mezzogiorno/tramonto).
- **Colonna dx**: temperature (ambiente + modulo stimato), tile meteo, scheda salute.
- **Footer**: link a `/api/state.json`, `/api/stream` (SSE), mappa registri Modbus.

### Responsive

- Desktop = 3 colonne; tablet = 2 colonne; **smartphone = colonna singola** con riordino (POA + percorso sole → mappa → meteo/salute), tile full-width, target touch ≥44px.
- Aggiornamenti via SSE identici su tutte le dimensioni; sole/POA si animano ogni minuto.
- Opzionale: manifest PWA ("aggiungi a home").

### Design system (token semantici, dark/light)

Un solo accento saturo (**emerald**) su base desaturata; **oro** riservato al sole/irradianza (data-viz). Numeri tabular monospace, radius 16px, ombre morbide. Default tema `auto` (`prefers-color-scheme`) + toggle manuale memorizzato in `localStorage`.

Token di riferimento (dal mockup approvato):

```
dark : bg #0a0e15 · surface #121a26 · surface-2 #182231 · inner #0e1622
       text #e9eef5 · dim #8b98ab · border rgba(255,255,255,.07)
       accent #10d992 · gold #ffc24b→#ff8a3d · good/warn/bad #10d992/#ffc24b/#ff6b6b
light: bg #f4f5f2 (off-white caldo) · surface #ffffff · surface-2 #f5f6f8
       text #0f1b2d · dim #5b6675 · border rgba(15,23,42,.08)
       accent #059669 · gold #d9861a · good/warn/bad #059669/#d9861a/#e5484d
```

### API HTTP

- `GET /` → dashboard (HTML/CSS/JS embeddati).
- `GET /api/state.json` → snapshot completo del `SolarState` (+ metadati catalogo).
- `GET /api/stream` → SSE, un evento ad ogni aggiornamento dell'Hub.
- `GET /api/catalog.json` → descrizione metriche (unità, registri) — utile a integratori.
- `GET /health` → liveness/readiness.

---

## 7. Configurazione (env var)

| Env | Default | Note |
|---|---|---|
| `PVHUB_LATITUDE` / `PVHUB_LONGITUDE` | — | **obbligatori**, fail-fast |
| `PVHUB_SITE_NAME` | `pv-hub` | etichetta |
| `PVHUB_ELEVATION_M` | auto | altrimenti dal provider |
| `PVHUB_TILT_DEG` / `PVHUB_AZIMUTH_DEG` | `30` / `180` | 180 = Sud |
| `PVHUB_ALBEDO` | `0.20` | |
| `PVHUB_TRANSPOSITION` | `hay_davies` | `hay_davies\|perez` |
| `PVHUB_CELLTEMP` / `_U0` / `_U1` | `faiman` / `25` / `6.84` | `faiman\|noct` (+ `_NOCT`) |
| `PVHUB_POLL_INTERVAL_S` / `_SOLAR_INTERVAL_S` | `600` / `60` | |
| `PVHUB_PROVIDER` | `openmeteo` | pluggable |
| `PVHUB_OPENMETEO_BASE_URL` / `_API_KEY` | free / — | key opzionale (commerciale) |
| `PVHUB_HTTP_BIND` / `_PORT` | `0.0.0.0` / `8080` | |
| `PVHUB_MODBUS_ENABLE` / `_PORT` / `_UNIT_ID` | `true` / `502` / `1` | |
| `PVHUB_MODBUS_WORD_ORDER` / `_HOLDING_MIRROR` | `abcd` / `true` | `abcd\|cdab` |
| `PVHUB_DEFAULT_THEME` | `auto` | `auto\|dark\|light` |
| `PVHUB_LOG_LEVEL` | `info` | tracing |

---

## 8. Container & deploy

- Build multi-stage → target `x86_64-unknown-linux-musl` → **binario statico** in `distroless/static` (o `scratch`), utente non-root, immagine ~10-20 MB.
- Espone `8080` (HTTP) e `502` (Modbus). La 502 è privilegiata: nel compose `cap_add: [NET_BIND_SERVICE]` **oppure** `PVHUB_MODBUS_PORT=1502`.
- `HEALTHCHECK` su `/health`.
- Consegnati `Dockerfile` + `docker-compose.yml` d'esempio (con blocco env commentato).

---

## 9. Resilienza & error handling

- `SolarEngine` non usa rete → **sole/POA sempre vivi anche col provider giù**.
- Fetch meteo: timeout + backoff esponenziale con jitter. Su errore: si mantengono gli ultimi valori buoni, `data_stale=true`, `data_age` cresce, `provider_ok=0`, `poll_errors_total++`. Modbus continua a servire l'ultimo stato noto (mai crash).
- Env obbligatori mancanti → fail-fast con messaggio chiaro.
- Shutdown pulito su SIGTERM (chiusura socket Modbus/HTTP).

---

## 10. Testing

- **Unit** — posizione solare vs valori di riferimento NREL SPA pubblicati; sanity trasposizione (POA≈GHI a tilt 0; POA cresce puntando il sole); monotonìa temperatura modulo; **round-trip encoding float32 Modbus** con entrambi i word-order.
- **Integrità catalogo** — nessuna collisione/overlap di registri; ogni metrica ha estrattore + unità.
- **Integrazione** — Open-Meteo mockato (wiremock): `SolarState` popolato, registri leggibili via client Modbus, SSE emette all'aggiornamento.
- **Golden** — snapshot della mappa registri.

---

## 11. Struttura sorgenti (Rust)

```
src/
  main.rs            avvio, wiring, shutdown
  config.rs          parsing env → Config (validazione, fail-fast)
  model.rs           SolarState, Sample, flag qualità
  catalog.rs         Metric[]: id, unità, registro, topic, estrattore
  hub.rs             Arc<RwLock<SolarState>> + broadcast
  scheduler.rs       task periodici meteo/sole
  providers/
    mod.rs           trait Provider
    openmeteo.rs
  solar/
    mod.rs           orchestrazione SolarEngine
    position.rs      NREL SPA (crate spa)
    transposition.rs Hay-Davies / Perez
    celltemp.rs      Faiman / NOCT
    clearsky.rs      Haurwitz + kt
  sinks/
    modbus.rs        server TCP, encoding da catalogo
    http.rs          axum: JSON, SSE, /health, UI statica
  assets/            index.html, app.js, styles.css, leaflet (bundlato)
Dockerfile
docker-compose.yml
```

---

## 12. Fuori scope (v1) / espansioni future

- MQTT (previsto come sink futuro, ~1 modulo grazie al catalogo).
- Multi-array / più orientamenti.
- Modbus RTU (seriale).
- Storico/persistenza (SQLite o TSDB) e grafici temporali storici.
- Energia teorica attesa (kWh/kWp), soiling, stime spettrali (era l'opzione "Massimo").
- Provider aggiuntivi (NASA POWER, PVGIS).
- Manifest PWA.

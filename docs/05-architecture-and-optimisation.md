# 05 · Architecture and per-file optimisation rules

## 1. Process model

```text
launcher.exe (Tauri 2 / Rust)                  msedgewebview2.exe ×3–5 (shared WebView2 runtime)
├─ tokio runtime (UDP A2S fan-out, HTTP, SQLite)   └─ Svelte 5 UI, one window, virtualised table
├─ Steam thread (steamworks Client, run_callbacks 50 ms)
└─ launch supervisor (spawns DayZ_BE.exe, watches exit)
```

One window, no tray icon by default, no background service. When the window is minimised, refresh timers pause.

## 2. Rust module layout (`src-tauri/src`)

| Module | Responsibility | Key crates |
|---|---|---|
| `steam/locate.rs` | registry → SteamPath, libraries, game folder, exe version | `winreg`, `keyvalues-parser` |
| `steam/workshop.rs` | `appworkshop_221100.acf`, `meta.cpp`, junction inventory | `keyvalues-parser`, `junction`, `notify` |
| `steam/sdk.rs` | Steamworks client thread: server list, UGC subscribe/download/progress | `steamworks` |
| `a2s/codec.rs` | packet build/parse, split-packet reassembly, challenge | `bytes` |
| `a2s/dayz_rules.rs` | fragment join + unescape + payload decode (spec in [03](03-server-discovery-and-a2s.md)) | – |
| `a2s/client.rs` | bounded-concurrency UDP fan-out, timeouts, retries, RTT | `tokio` |
| `browser/` | server store, refresh scheduler, filters, cache | `rusqlite` (bundled, WAL) |
| `launch/` | `-mod=` builder, junction creation, `DayZ_BE.exe` spawn, exit watch | `std::process` |
| `commands.rs` / `events.rs` | Tauri IPC surface (typed, camelCase) | `tauri`, `serde` |
| `settings.rs` | JSON settings in `%APPDATA%\<app>\settings.json` | `serde_json` |

## 3. Data flow

1. Startup: read cache from SQLite → emit `servers:snapshot` → UI renders in < 100 ms.
2. Steam thread: `internet_server_list(221100)` streams `GameServerItem`s → batch every 100 ms → `servers:batch` event.
3. A2S worker: INFO for every listed server (ping + live players/keywords) with concurrency 256, 2 s timeout, 1 retry; results coalesced to the UI every 100 ms.
4. RULES on demand: selected row, favourites, filters that need mods, join.
5. Join: RULES → diff mods against workshop inventory → subscribe/download with progress → junctions → spawn.

## 4. IPC rules

- Commands are request/response; anything that streams uses events with **batched arrays**, never one event per server.
- Server row payload: `{ id:"ip:qport", name, map, players, max, queue, ping, time, tags:[…], ver, pw, be, fpp, dlc }`. Mods only travel on demand.
- Never send more than ~200 KB in one event; the WebView2 IPC bridge serialises to string.

## 5. Frontend state (Svelte 5 runes)

- `servers = $state(new Map())`, `filtered = $derived(...)` recomputed only when filters/sort or a batch changes; batches mutate the map in one tick.
- Virtual list: own component, fixed 36 px rows, renders viewport + 10 overscan rows; total DOM nodes stay < 1 000.
- Sorting 20 000 rows with a precomputed key array takes single-digit ms; do it in the main thread inside `requestIdleCallback`.
- No global stores library, no UI kit, no CSS framework. Inline SVG sprite for icons.

## 6. Performance budgets (measure before every release)

| Metric | Budget | M0 baseline (2026-09-21, D-023) | How to measure |
|---|---|---|---|
| Installer size (NSIS, downloadBootstrapper) | < 15 MB | M0 1.32 MB; M4 2.26 MB; M7 3.23 MB (exe 8.54 MB with SQLite, Steamworks and the updater's TLS stack); v0.1.3+flags 3.83 MB (exe 10.5 MB: +1.79 MB GeoIP table, +145 KB flag sprite, D-073); v0.1.6 4.43 MB (4 433 490 B, exe 12.4 MB: + notification plugin and reqwest/rustls for the DZSA fallback, D-086/D-089; LAN and Friends tabs, D-092/D-094); v0.1.8 4.52 MB (4 524 390 B, exe 12.7 MB: + opener and window-state plugins, news module, D-098/D-099); v0.1.12 4.68 MB (4 675 803 B, exe 13.1 MB: + `image` jpeg/png for the news thumbnails, D-111); **v0.1.19 4.69 MB** (4 687 251 B, exe 13.2 MB) | file size |
| Host process (`dayz-launcher.exe`) private bytes, idle | < 90 MB with list loaded and Steamworks initialised | M0 5.9 MB; M4 65–79 MB; M7 70 MB flat over 7 min (82 during refresh); **v0.1.5 70.3 MB at 7 min** with the GeoIP table read in place (D-082; 81–94 MB with the copied table); v0.1.6 68.7 MB at 200 s (external), 82.4 MB in the in-app sample taken during the start-up refresh; **v0.1.19 73.0–73.6 MB flat from 80 s to 12 min** (94 MB at 40 s, during the start-up refresh), D-136. `steam_api64.dll` loads `steamclient64.dll` and `gameoverlayrenderer64.dll` in-process; releasing Steamworks when idle does not lower this (D-057) | `Get-Process dayz-launcher \| select PrivateMemorySize64`; Diagnostics → Performance |
| All processes private bytes (commit), idle | < 330 MB with list loaded | M0 171.7 MB; M4 294–315 MB; M7 282–310 MB (host 70, browser 41, gpu 85–91, renderer 65–72, utility 11.5 + 7.5, crashpad 2.2); two earlier runs grew past 400 MB and are recorded as unexplained (Q18, D-060). **v0.1.4+perf, external at 72–96 s after a cold start: host 87.6–94.4 + WebView 222–230 (browser 38, gpu 97, renderer 65–73, utilities 18.5, crashpad 2.7) = 310–325 MB**; the in-app sample at 34 s (during the start-up refresh) read WebView 342 MB, to be re-checked against an external reading taken at the same instant (D-078). **v0.1.6, external at 200 s after a cold start, window maximised on a 2560×1440 display: host 68.7 + WebView 265.3 (6 processes) = 334 MB**, 1 % over budget; the in-app sample during the refresh read 340.5 MB. The extra WebView memory against v0.1.4 (222–230 MB) coincides with the maximised window (larger GPU and renderer surfaces); to be re-measured at 1280×800 (Q18 monitoring). **v0.1.11 (home page with 4K originals): 348–372 MB** (WebView 254–279), over budget (D-111); v0.1.12 (640 px thumbnails), 970 px window: 305 MB at 48 s, 297 MB at 108 s, 235 MB at 168 s (host 70–82, WebView 165–223), D-113. **v0.1.19, 12-minute idle run after a cold start (970 px window, News page open, 20 s samples): 294–316 MB steady** (host 73, WebView 221–242 across 6 processes) with one transient sample at 347 MB at t = 8 min, i.e. 5 % over budget for one 20 s sample and inside it for every other; the app's own Diagnostics read 318.9 MB at the same time. The fluctuation follows the News page's decoded thumbnails, D-136 | sum of `PrivateMemorySize64` over host + `msedgewebview2.exe` whose command line contains `com.dayzlauncher`; now also Diagnostics → Performance |
| Cold start to first painted view (the home page since D-101; the server list before) | < 1.0 s | v0.1.4+perf 795 ms (process start → first frame with rows, 33 480 cached rows, fresh process after install; a warm second instance showed 314 ms); v0.1.6 814 ms (27 249 cached rows, first start after the installer); **v0.1.19 427 ms** (19 123 cached rows, cold start after the installer), D-136 | Diagnostics → Performance (`perf_first_paint`, D-078) |
| Populated-server refresh | first rows in seconds, whole pass < 60 s | Criterion replaced at M2 acceptance: "20 000 servers in 15 s" is neither reachable nor needed, because throughput is bounded by the consumer NAT flow table and Steamworks already supplies the list with pings (D-037). Measured: M3 ≈ 38 s; **v0.1.19: 2 379 servers from Steam in 36 s, then 2 365 verified, 10 fake, 4 offline**; the manual Refresh continues into the empty servers over minutes and reached 19 088 rows (D-141) | the Servers header line; app log |
| Idle CPU, window focused | < 0.5 % | M0 ≈ 0.6 % (startup included); M4 0.1 % over 20 s; M7 0.15 % of one core averaged over 7 min (1.2 → 1.8 s cumulative); **v0.1.19 0.38 %** of one core over 90 s, all seven processes summed (browser 0.16, host 0.14, renderer 0.05, gpu 0.02, utilities 0.02), D-136 | per-process `TotalProcessorTime` deltas over 90 s. **Do not take screenshots during the window**: capturing the window forces redraws and reads 1–2 % instead of 0.4 % |
| Idle CPU, window minimised or unfocused | ≈ 0 % | **v0.1.19: 0.02 %** of one core with the window visible but behind another app, **0.2 %** minimised (`ShowWindow SW_MINIMIZE`, 120 s), D-136 | same |
| Frontend JS bundle (gzip) | < 120 KB | M0 14.44 KB JS + 0.97 KB CSS; v0.1.3+flags 44.4 KB JS + 4.4 KB CSS (plus the 145 KB flag sprite as a separate asset); v0.1.6 54.2 KB JS + 5.8 KB CSS (165.8 kB / 31.6 kB raw; LAN and Friends views, DZSA, profiles, two-column Settings and Diagnostics); v0.1.8 57.7 KB JS + 7.0 KB CSS (178.2 kB / 38.4 kB raw; home page, opener); **v0.1.19 58.5 KB JS + 7.7 KB CSS gzip** (182.1 kB / 43.8 kB raw; the raw CSS is over its 40 kB figure, see §7), D-136 | `vite build` report, or gzip the files in `dist/assets` |

**Metric note.** "Sum of working sets" is misleading for WebView2: the runtime's DLL pages are mapped into every helper process and counted each time (331 MB summed WS at M0 vs 172 MB commit). Budgets therefore use private bytes. The six WebView2 helper processes are the runtime's fixed cost; our job is to keep the host and renderer from growing on top of them.

## 7. Per-file optimisation checklist

Apply whenever a file of that type is created or patched; note deviations in [09-decisions-log.md](09-decisions-log.md).

### Rust (`*.rs`, `Cargo.toml`)
- `[profile.release]`: `opt-level = 3` (or `"s"` if size wins), `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- Zero-copy parsing with `&[u8]` slices / `bytes::Bytes`; no `String` allocation per field unless it leaves the module.
- Bounded channels and semaphores for every fan-out; no unbounded `Vec` growth from network input (cap rules payload at 64 KB).
- `#[serde(rename_all = "camelCase")]` on IPC types; `skip_serializing_if = "Option::is_none"`.
- No `unwrap()` on I/O or network paths; typed errors with `thiserror`.
- Every parser has a unit test with the live fixture bytes captured by `tools/a2s_probe.js`.

### Svelte (`*.svelte`)
- Runes only (`$state`, `$derived`, `$effect`); no legacy stores.
- One component per concern; the table row is a plain component with no effects.
- No layout thrash: read DOM sizes in one `$effect`, write in the next frame.
- `{#key}` blocks avoided in the row list; keyed `{#each}` by server id.

### CSS
- Design tokens as CSS custom properties on `:root`; themes switch by `[data-theme]`.
- System font stack (`"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`): no web fonts shipped.
- `content-visibility: auto` on off-screen panels; `prefers-reduced-motion` respected; transitions ≤ 150 ms.
- No CSS framework; **gzip is the binding budget: < 8 KB** (7.7 kB at v0.1.19). The raw figure is now a guide rather than a gate: 43.8 kB at v0.1.19 against the old 40 kB line (31.6 kB at v0.1.6, D-094; 30 kB before that). Raw growth comes from Svelte's per-component scoped styles across eleven views, which is the chosen architecture; deduplicating every identical rule block into globals was measured at 4.7 kB recovered (the largest single items: `:focus-visible` outlines 15×, the accent button 3×, `:disabled` 8×), so it is the lever to pull if the gzip budget is ever threatened, not a reason to refactor eleven components now (D-136).

### TypeScript / Vite
- `build.target: "esnext"` (WebView2 is Chromium 153, no polyfills), `minify: "oxc"` (Vite 8 default; esbuild is not bundled, D-020), `sourcemap: false` in release.
- No runtime dependencies beyond `@tauri-apps/api` and its plugin bindings; check with `npm ls --omit=dev`.
- Route-level code splitting for Settings/Diagnostics.

### Tauri (`tauri.conf.json`, `capabilities/*.json`)
- `app.withGlobalTauri: false`; strict CSP (`default-src 'self'`); only the plugins used, each with the narrowest permission set.
- `bundle.targets: ["nsis"]`, `windows.webviewInstallMode: { type: "downloadBootstrapper" }`, `nsis.installMode: "currentUser"`.
- Updater with a signing key pair; the public key committed, the private key never.
- Resources: `steam_api64.dll` only.

### Assets
- SVG icons in one sprite, optimised with SVGO; raster only for map thumbnails (WebP, ≤ 40 KB each); app icon via `tauri icon`.

## 8. Security and privacy
- No telemetry, no accounts, no remote code. Outbound traffic: Steam (Steamworks), UDP A2S to game servers, optional DZSA/BattleMetrics/news HTTP that the user can switch off.
- Update artifacts are signature-checked by the updater plugin.
- Never write outside `%APPDATA%\<app>` except the `!Workshop` junctions in the game folder.

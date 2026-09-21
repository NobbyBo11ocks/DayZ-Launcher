# 01 · Competitor analysis

Researched 2026-09-21. Sources are listed in [08-sources.md](08-sources.md) (S-01 … S-07). Screenshots were taken in the built-in browser the same day.

## Summary table

| Launcher | Platform / size | Tech (observed or inferred) | Server data source | Mod handling | Launch method | Standout UX |
|---|---|---|---|---|---|---|
| **DZSA Launcher** (dayzsalauncher.com, Maca134) | Windows, portable build 0.0.6.3 | Classic Win32-style dark table UI (dated) | Own backend at `dayzsalauncher.com/api/v1` (servers must be registered via "Check Server"; server-side DZSA package optional) | Reads server mod list, subscribes via Workshop, shows missing mods | Runs `DayZ_BE.exe … -exe DayZ_x64.exe -mod=…` directly (log excerpt, S-13) | De-facto standard; its API is reused by dayz-ctl and the Linux CLI launcher |
| **launchZ** (launch-z.com) | Windows zip, v1.3.0 | Dark navy UI, master–detail layout | Not disclosed | Mod checklist with installed ticks | Not disclosed | Hourly population bar chart, country flag + ping, queue size, "Anti Fake Pop", day/night calculator, region/map/mod filters, favourites |
| **DayZ Beans Launcher** (dayzbeanslauncher.com) | Win 10/11 + Linux AppImage, ~100 MB, auto-update | ~100 MB strongly implies Electron/Chromium | Own backend (claims 50,432 servers, 72 h population history) | "Click a server → resolves mods, syncs them, drops you on the loading screen" | Not disclosed | Four free themes, direct-connect by IP, filters for ping/perspective/map/mods/freshly-wiped/time-accel/day-night, server-owner listing with paid visibility boosts |
| **dayz-ctl** (WoozyMasta, GitHub) | Linux, single Bash script (Apache-2.0) | fzf/gum terminal UI, jq | Full DZSA list `GET /api/v1/launcher/servers/dayz` cached with TTL; BattleMetrics for extra detail | SteamCMD (auto) or manual Workshop subscribe; symlinks `@<id>` into game dir | `steam -applaunch 221100 -nolauncher -name X -mod=@id;@id -connect=IP -port=PORT [-password=…]` | Favourites, last-10 history, offline mode, GeoIP per server |

## What each one does well (adopt)

- **launchZ**: the master–detail layout (list left, rich detail pane right) is the most efficient way to browse. Population-by-hour chart and queue size are real decision inputs for players. Country + ping in the row is essential.
- **Beans**: one-click join that resolves and syncs mods with no manual Workshop ID copying is the bar to clear. Direct connect by IP:port. Multiple themes. Freshly-wiped and day/night filters. Transparent "no ads, no tracking" stance.
- **DZSA**: the "Check Server" self-registration model and a stable JSON API made it the ecosystem's data source. Its single-server query endpoint returns exactly the fields a launcher needs (verified live, see [03](03-server-discovery-and-a2s.md)).
- **dayz-ctl**: caching with TTL, favourites + history stored as small JSON files, deriving mod folder names from `meta.cpp`, and building the whole launch line from data rather than UI state.

## What to avoid

- **Electron-class footprint** (Beans ~100 MB download, Chromium RAM). Our target is < 15 MB installer and < 120 MB idle RAM across all processes.
- **Hard dependency on a third-party backend** for the basic server list. DZSA and Beans are both single points of failure. We can get the list key-free from Steam itself (Steamworks `ISteamMatchmakingServers`) and query each server directly (A2S), then use DZSA as an optional enrichment source.
- **Dated Win32 table UI** (DZSA). Modern dark UI with proper typography, but without heavy animation.
- **Requiring server registration** to appear in the list. Every DayZ server that reports to Steam must appear.

## Feature matrix to beat

| Feature | DZSA | launchZ | Beans | dayz-ctl | Ours (target) |
|---|---|---|---|---|---|
| Full server list without registration | ✗ | ? | ✓ | ✗ (uses DZSA) | ✓ (Steam master list) |
| Real ping (UDP RTT) | ✓ | ✓ | ✓ | ✓ | ✓ |
| Mod list per server | ✓ | ✓ | ✓ | ✓ | ✓ (A2S_RULES direct, DZSA fallback) |
| One-click mod subscribe/sync | ✓ | ? | ✓ | ✓ (steamcmd) | ✓ (Steamworks UGC) |
| Installed / needs-update mod state | ✓ | ✓ | ✓ | ✓ | ✓ (appworkshop ACF + UGC item_state) |
| Direct connect by IP | ✓ | ? | ✓ | ✓ | ✓ |
| Favourites / history | ✓ | ✓ | ✓ | ✓ | ✓ |
| Queue size, time accel, day/night | partial | ✓ | ✓ | ✓ | ✓ (from A2S keywords) |
| Population history | ✗ | ✓ (hourly) | ✓ (72 h) | ✗ | ✓ local-only (SQLite, no backend) |
| Country flag | ✗ | ✓ | ✓ | ✓ (geoiplookup) | ✓ (offline DB, see open questions) |
| Themes | ✗ | ✗ | 4 | n/a | ≥ 2 (dark/light) + accent |
| Auto-update | ✗ | ✗ | ✓ | n/a | ✓ (tauri-plugin-updater, signed) |
| Version mismatch warning | ✓ | ? | ✓ | ✗ | ✓ (exe ProductVersion vs A2S version) |
| Idle RAM | low (Win32) | ? | high (Electron) | n/a | < 120 MB target |

## Visual notes from screenshots (2026-09-21)

- **DZSA**: single dense table, red header bar, left filter column with map/mod dropdowns and checkboxes (first-person only, password, modded only, DLC). Row: name, time, players, map, ping, favourite/join icons. Functional but 2015-era.
- **launchZ**: dark navy (#0f1724-ish) with light-blue accent. Left: sortable list with flag, name, players, ping. Right: header with server name + Play button, hourly bar chart "Hours per day & night", details grid (players, perspective, acceleration, map, time, game/query address), mods list with green ticks for installed.
- **Beans**: near-black background, warm gold/amber accent, card layout, big rounded buttons, stat tiles (servers, online now, players online). Marketing site shows the app: top search bar with filter chips, virtualised table with map/time/ping/players columns and per-row Join/favourite buttons, left rail with profile.

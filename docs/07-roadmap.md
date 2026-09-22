# 07 · Roadmap and acceptance criteria

Each milestone ends with its verification steps run and recorded in [09-decisions-log.md](09-decisions-log.md). No milestone starts before the sources it depends on are re-checked ([08](08-sources.md)).

| # | Milestone | Deliverables | Acceptance |
|---|---|---|---|
| M0 ✅ 2026-09-21 | Toolchain and scaffold | rustup stable 1.98.1, plain Svelte 5 + Vite 8 + TS 6.0 frontend, Tauri 2.11.6 core, release profile, `.gitignore`, git init (no commits yet) | **Done**: NSIS installer 1.32 MB (< 15 MB); idle private bytes 172 MB total / 5.9 MB host; IPC round trip proven in `tauri dev` smoke test (D-021, D-023, D-024) |
| M1 ✅ 2026-09-21 | Steam and game discovery | `steam/{registry,vdf,locate,workshop,cpp,version,diagnostics}.rs`, `commands::diagnostics`, Diagnostics view, fixtures in `src-tauri/tests/fixtures` | **Done**: live inventory matches [02](02-dayz-launch-mechanics.md) (path, 2 libraries, DayZ 1.29.163709 build 24689949, 4 items, 20 junctions / 16 dangling) in 7 ms; 9 unit tests on live fixtures; clippy and svelte-check clean (D-025…D-029) |
| M2 ✅ 2026-09-21 | A2S engine | `a2s/{reader,packet,info,rules,players,tags,client}.rs`; fixtures from three live servers (12 / 121 / 105 mods) via `tools/a2s_capture.js`; `tests/live_sweep.rs` benchmark | **Done**: all three RULES fixtures decode with 0 trailing bytes; 30 unit tests; live sweep of 18 678 addresses answered 12 561 with p99 RTT 251 ms (D-037). The "20 000 in 15 s" criterion was withdrawn: throughput is bounded by consumer NAT flow tables, and Steamworks supplies the list with pings so the fan-out only serves visible rows and on-demand queries. Findings D-033…D-038 (single large datagrams, anonymous PLAYER entries, content hashes, fake population) |
| M3 ✅ 2026-09-21 | Server list via Steamworks | `steam/sdk.rs` thread (partitioned `internet_server_list`, 100 ms batches, persisted refresh throttle, master-throttling guard), `browser/{model,cache}.rs` (rusqlite WAL, schema v2), `servers_cached`/`servers_refresh`/`steam_status` commands, Servers view with rule R0 "Hide inflated" | **Done**: cached list renders ≈ 0.7 s after launch even with 33 575 rows; Steam init ≈ 0.5 s; automatic refresh = populated servers in ≈ 38 s; full refresh measured at 473 s with 27 037 fakes flagged (D-040…D-046) |
| M4 ✅ 2026-09-21 | Browser UI + population trust | Frameless title bar, virtualised grid, filter bar, details pane (live INFO/RULES/PLAYER, mods with installed ticks), Settings (themes/accents), `browser/verify.rs` rules R2–R5, automatic PLAYER pass after each refresh, on-demand checks for visible rows | **Done**: 2 858 populated servers verified in 33 s (2 840 verified, 8 fake, 6 unverifiable, 4 offline); cached list in 0.75 s; host CPU 0.1 % idle; budgets revised with attribution (D-047…D-050). Join button arrives with M5 |
| M5 ✅ 2026-09-21 | Mod sync and launch | Steam-thread sync jobs (`steam/sdk.rs`), `launch/{args,mods,process}.rs`, `settings.rs`, commands `join_plan`/`mods_sync`/`launch_game`/`settings_*`, Join dialog with per-mod progress, Mods inventory view, launch settings | **Done**: live test downloaded a missing mod in 1.3 s, created its junction, and started DayZ through `DayZ_BE.exe` with the official argument form (alive after 30 s); 47 unit tests incl. real NTFS junctions; official junctions untouched (D-051, D-052) |
| M6 ✅ 2026-09-21 | Favourites, history, population, direct connect, import | `favourites`/`history`/`population` tables (survive schema bumps), star toggle + `F` key, Favourites and Recent views, "Join again", 72 h sparkline from verified samples, direct connect with game-port matching, import of the official `FavouriteServers.xml` (`steam/official.rs`) | **Done**: live test imported this machine's official favourite (server answered in 138 ms), direct connect resolved game port 2402 to query port 27017 despite a sibling server on 27016, samples round-trip; 50 unit tests (D-053, D-054) |
| M7 ✅ 2026-09-21 | Polish and release | Signed updater (plugin + minisign key, GitHub releases endpoint; v0.1.0 published 2026-09-21, D-062), NSIS installer verified by silent install/run/uninstall, generated icon, Welcome overlay, diagnostics export, perf pass (Q16/Q17 measured and decided), unsigned installer by decision | **Done** on this machine (no VM available): install → Welcome → Join is three clicks; budgets in [05 §6](05-architecture-and-optimisation.md) updated with M7 numbers; D-055…D-059. Open: Q18 renderer growth check, release host for the updater endpoint |

Out of scope for v1: Linux, server-owner listing/boosts, accounts, telemetry. (The LAN browser, once listed here, shipped in v0.1.6 via `lan_server_list`.)

## After v0.1.0 (all 2026-09-21, released as v0.1.1–v0.1.5)

| Release | Added | Decisions |
|---|---|---|
| v0.1.1 | Details pane no longer re-queries in a loop | D-065 |
| v0.1.2 | Installer defaults to `%LOCALAPPDATA%\Programs\DayZ Launcher` (custom NSIS template) | D-067 |
| v0.1.3 | Collapsible details pane, no fill bars under counts, UI preferences in `settings.json` | D-069, D-070 |
| v0.1.4 | Country flags (offline GeoIP), wait for a free slot when joining, mod management (update / unsubscribe) | D-073, D-074, D-075 |
| v0.1.5 | Find servers by mod, Performance section in Diagnostics, Steam idle release setting, tidied details pane, single instance, favourite alerts, store lifetime fix | D-077–D-084 |
| v0.1.6 | Windows toasts for favourite alerts, launch profiles, DZSA list fallback, LAN tab, Friends tab, dangling-junction cleanup in Diagnostics, slim title bar, views that fit the window without scrolling, code-signing guide (docs/12) | D-086–D-094 |
| v0.1.7 | Join a friend's session from the Friends tab: Steam's game-server address first, rich-presence `connect` as fallback, presence re-requested every poll; LAN scans no longer count as a list refresh | D-096 |
| v0.1.8 | Home page (welcome band with Steam name and avatar, latest DayZ updates with pictures, unread badge, update alerts), window size and position remembered, external links through the opener plugin | D-098–D-102 |
| v0.1.9 | Server and friend counts in the title bar, tidier news bar, presence reads no longer block the idle release | D-103 |
| v0.1.10 | Title-bar server count instant from the cache and live during refresh, globe and person icons | D-105, D-106 |
| v0.1.11 | Servers page header: two compact rows, direct-connect popover, one control style; checkpoints after joins and favourites | D-108, D-110 |
| v0.1.12 | News pictures as host-made 640 px thumbnails (memory and bandwidth); main window created after setup (start-up race) | D-111–D-114 |
| v0.1.13 | Friend avatars, cache counts in Diagnostics, Mods empty state | D-115 |
| v0.1.14 | Join-plan warning when Steam's `ActiveProcess` pid is stale, high priority with step-down while DayZ runs, elevation matched to Steam | D-118, D-119 |
| v0.1.15 | Short accent "Join again" button on the home page, no news Refresh button | D-121 |
| v0.1.16 | Stale-registry note removed from the join dialog | D-123 |
| v0.1.17 | Steam thread retries init every 10 s when Steam starts after the launcher | D-125 |
| v0.1.18 | Friends marker on server rows and a "Friends" quick filter | D-128 |
| v0.1.19 | "Clear list" on the Recent view (confirmed), twelve accent colours picked as dots with an `--accent-fg` token so light-theme accents stay readable, lime as the new default accent with a one-time move off amber, news summaries no longer opening with a picture address, README rewritten with fresh screenshots, MIT licence file | D-130–D-135 |
| v0.1.20 | One Refresh button on the Servers page (it continues into the empty servers), Mods view reworked into a mod manager (search, filters, multi-select, bulk update and unsubscribe, reveal folder), readable mod names, centred Recent empty state, capped news hero | D-141–D-143 |
| v0.1.21 | Mods count column in the server browser, sortable, with unscanned servers marked apart from vanilla ones | D-146 |
| infra | GitHub Actions CI and tag-driven release workflow, release tools (`make_latest.js`, `verify_update_sig.js`, `nsis_template_check.js`) | D-063, D-071 |

Candidates, not scheduled: code signing (Q11), a "friends on this server" marker in the browser once Q21 shows how DayZ reports servers to Steam.

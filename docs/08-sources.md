# 08 · Sources log

Rule: every fact in `docs/` cites an S-number. Before any patch that touches protocol, launch, Steam or version behaviour, re-open the relevant source and record the check in [09-decisions-log.md](09-decisions-log.md). Confidence: **A** = primary/official or verified live, **B** = reputable secondary or two independent implementations agree, **C** = single unofficial claim.

| ID | Accessed | Source | What was taken | Conf. |
|---|---|---|---|---|
| S-01 | 2026-09-21 | https://dayzbeanslauncher.com/ (fetched + 4 screenshots) | Features, size ~100 MB, Win/Linux, themes, monetisation, UI style | A (self-described) |
| S-02 | 2026-09-21 | https://dayzsalauncher.com/ (browser; SPA) | Portable 0.0.6.3, UI style, "Check Server" registration | A |
| S-03 | 2026-09-21 | https://launch-z.com/ (fetched + screenshot) | v1.3.0, feature list, UI layout | A |
| S-04 | 2026-09-21 | https://github.com/WoozyMasta/dayz-ctl | Architecture, features, config keys, dependencies | A |
| S-05 | 2026-09-21 | https://raw.githubusercontent.com/WoozyMasta/dayz-ctl/master/dayz-ctl | DZSA endpoints, JSON fields, launch line, `@<id>` symlinks, BattleMetrics search URL | A |
| S-06 | 2026-09-21 | https://github.com/bastimeyer/dayz-linux-cli-launcher (README) | Uses DZSA query API, `-applaunch` arg-length bug note | A |
| S-07 | 2026-09-21 | https://raw.githubusercontent.com/bastimeyer/dayz-linux-cli-launcher/master/dayz-launcher.sh | `api/v1/query/{ip}/{port}`, `.result.mods[].steamWorkshopId`, `meta.cpp` name regex, `steam -applaunch 221100 -mod=… -connect=… -nolauncher -world=empty -name=…` | A |
| S-08 | 2026-09-21 | https://velvetcache.org/2024/05/23/dayz-server-browsers/ | A2S_INFO layout, EDF bits, AppID overflow, keywords | B |
| S-09 | 2026-09-21 | https://velvetcache.org/2026/07/09/dayz-server-browsers-part-two/ | RULES chunking, 127/128-byte chunk note, packed layout, plain rules list. Its escape table is wrong (see S-12/S-14) | B |
| S-10 | 2026-09-21 | https://github.com/WoozyMasta/a2s (README) | A3SB wrapper, game id 221100 | A |
| S-11 | 2026-09-21 | https://raw.githubusercontent.com/WoozyMasta/a2s/master/pkg/a3sb/docs/README.md | Escape table, 124-byte fragments, 1400-byte UDP, 8192 buffer | A |
| S-12 | 2026-09-21 | WoozyMasta/a2s `pkg/a3sb/{rules,rules_dayz,mods,flags,dlc,signatures,version}.go`, `internal/bread/sequences.go` | 2-byte fragment keys `[page, count]`, unescape, id-length byte (1/4/8, 19 = Arma creator DLC), DayZ DLC bits, signatures, protocol v2 vs v3 | A |
| S-13 | 2026-09-21 | Web search result quoting a DZSA/launcher log (scribd doc 467660479) | `"DayZ_BE.exe" 0 1 1 -exe DayZ_x64.exe …` | C, superseded by S-41 `Launcher.log` `GameExecutor` lines (A) |
| S-14 | 2026-09-21 | https://raw.githubusercontent.com/Yepoleb/dayzquery/master/dayzquery.py | Independent DayZ RULES parser: same escape table, read order version/overflow/dlc_flags/dlcs/mods/signatures/description | A |
| S-15 | 2026-09-21 | https://developer.valvesoftware.com/wiki/Server_queries (browser; blocks plain fetchers) | Data types, split-packet header, challenge, request/response formats | A |
| S-16 | 2026-09-21 | https://gist.github.com/Decicus/5d6eb057da4b5f228a4f1c2334ce1e2f | Steam Web API GetServerList URL and field list | B |
| S-17 | 2026-09-21 | https://github.com/WoozyMasta/smsq, Steam community "master server query" thread, ribasco master query docs | Master Server Query Protocol basics, filter keys | B |
| S-18 | 2026-09-21 | https://dayzsalauncher.com/api/v1/query/51.81.8.81/27017 (browser, live) | Full JSON shape of a single-server query | A |
| S-19 | 2026-09-21 | https://dayzsalauncher.com/api/v1/launcher/servers/dayz | Exists; response > 10 MB (fetch aborted at the 10 MB limit) | A |
| S-20 | 2026-09-21 | https://crates.io/api/v1/crates/tauri | 2.11.6 stable (2026-09-19); 3.0.0-alpha.1 pre-release | A |
| S-21 | 2026-09-21 | https://registry.npmjs.org/@tauri-apps/cli/latest | 2.11.5 | A |
| S-22 | 2026-09-21 | https://registry.npmjs.org/@tauri-apps/api/latest | 2.11.1 | A |
| S-23 | 2026-09-21 | Web search (endoflife.date/rust, Wikipedia); confirmed by `rustup` output `rustc 1.98.1 (48a229cea 2026-09-01)` | Rust 1.98.1 released 2026-09-03 | A |
| S-24 | 2026-09-21 | https://crates.io/api/v1/crates/tokio | 1.53.1 (2026-07-20) | A |
| S-25 | 2026-09-21 | https://crates.io/api/v1/crates/steamworks + docs.rs `Client`, `MatchmakingServers`, `UGC` pages | 0.13.1; `Client::init_app`, `run_callbacks`, `matchmaking_servers().internet_server_list(app, &[HashMap])`, `ping_server`, `server_rules`, `UGC::{subscribe_item, download_item, item_state, item_install_info, item_download_info, subscribed_items}` | A |
| S-26 | 2026-09-21 | https://crates.io/api/v1/crates/junction | 2.0.0 (2026-05-01) | A |
| S-27 | 2026-09-21 | https://crates.io/api/v1/crates/winreg | 0.56.0 (2026-03-14) | A |
| S-28 | 2026-09-21 | https://crates.io/api/v1/crates/keyvalues-parser | 0.2.4 (2026-05-17) | A |
| S-29 | 2026-09-21 | https://registry.npmjs.org/svelte/latest | 5.57.1 | A |
| S-30 | 2026-09-21 | https://registry.npmjs.org/vite/latest | 8.3.0, engines `^20.19.0 \|\| >=22.12.0` | A |
| S-31 | 2026-09-21 | https://registry.npmjs.org/@sveltejs/vite-plugin-svelte/latest | 7.3.0, peers vite ^8, svelte ^5.46.4 | A |
| S-32 | 2026-09-21 | https://registry.npmjs.org/typescript/latest | 7.0.2 | A |
| S-33 | 2026-09-21 | https://v2.tauri.app/distribute/windows-installer/ | NSIS vs MSI, WebView2 install modes and sizes, per-user vs per-machine | A |
| S-34 | 2026-09-21 | https://v2.tauri.app/plugin/ | 30 official plugins, all Windows | A |
| S-35 | 2026-09-21 | Web search: rustify.rs, pkgpulse.com, gethopp.app, buildmvpfast.com (Tauri vs Electron 2026) | Bundle 3–10 MB vs 85–200 MB, idle RAM ~42 vs ~168 MB, cold start 0.38 vs 1.42 s | B |
| S-36 | 2026-09-21 | Web search: tech-insider.org, pkgpulse.com, arc.dev (Svelte 5 / Solid / React 19, 2026) | Baseline bundle 47 KB Svelte vs 156 KB React; Solid ~7 KB runtime | B |
| S-37 | 2026-09-21 | https://pkg.go.dev/github.com/woozymasta/a2s/pkg/keywords + DayZ forum "Server tags demystified" | Tag meanings: lqs, etm, entm, no3rd, shard, privHive, battleye | B |
| S-38 | 2026-09-21 | https://www.battlemetrics.com/developers/documentation | Rate limits 60/min, 15/s unauthenticated | A |
| S-39 | 2026-09-21 | https://partner.steamgames.com/doc/webapi/isteamremotestorage + TF2 wiki | `POST ISteamRemoteStorage/GetPublishedFileDetails/v1/` with `itemcount`, `publishedfileids[i]`; returns title, file_size, time_updated; no key required | B |
| S-40 | 2026-09-21 | Steam community threads on `!Workshop` | `!Workshop` is a proxy folder of links to `steamapps\workshop\content\221100\<id>` | B (confirmed locally, S-41) |
| S-41 | 2026-09-21 | **Local machine** (Windows 11 IoT LTSC 26200): registry `HKCU\Software\Valve\Steam`, `libraryfolders.vdf`, `appmanifest_221100.acf`, `appworkshop_221100.acf`, `!Workshop` junctions, `meta.cpp`, `mod.cpp`, `%LOCALAPPDATA%\DayZ\*.RPT`, `DayZLauncher.exe.config`, `Launcher\Config.json`, `%LOCALAPPDATA%\DayZ Launcher\*` | Real file layouts, official launcher `-mod=` format, app id, versions | A |
| S-42 | 2026-09-21 | [tools/a2s_probe.js](../tools/a2s_probe.js) run against 51.81.8.81:27017 | Live INFO/RULES/PLAYER decode, matched DZSA's 12 mods | A |
| S-43 | 2026-09-21 | https://v2.tauri.app/release/tauri/all-versions/ | 2.11.x changelog entries | A |
| S-44 | 2026-09-21 | https://vincibean.github.io/my-dayz-guide/guide/launch-parameters.html + web search | Server params; client params `-connect -port -password -name -mod -nosplash -skipintro -nolauncher` | B |
| S-45 | 2026-09-21 | https://feedback.bistudio.com/T170180 | Not retrievable (403); Bohemia ticket on A2S_RULES docs | n/a |
| S-46 | 2026-09-21 | `npm create tauri-app@4.7.4 -- … --template svelte-ts` output | Template is SvelteKit (adapter-static 3.0.10, kit 2.65.1, vite 8.0.16, svelte 5.56.3, typescript ~6.0.3); Cargo release profile it ships | A |
| S-47 | 2026-09-21 | `npm view svelte-check version peerDependencies` | 4.7.6, peers `typescript ^5.0.0 \|\| ^6.0.0`, `svelte ^4 \|\| ^5` | A |
| S-48 | 2026-09-21 | https://api.github.com/repos/Noxime/steamworks-rs/contents/steamworks-sys/lib/steam/redistributable_bin/win64 + README | `steam_api64.dll` (317 080 B) and `.lib` vendored; SDK 1.64; MSRV 1.80; features `serde`, `image` | A |
| S-49 | 2026-09-21 | https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe (+ `.sha256`) | rustup-init.exe SHA-256 `6f4bef66…db7e` matched the published checksum; file is not Authenticode-signed | A |
| S-51 | 2026-09-21 | https://github.com/DataGoblin/dayz-server-fake-players | Memory opcode patch that inflates the A2S-reported player count; dynamic (+N over real); addresses change per build | A (primary, the tool itself) |
| S-52 | 2026-09-21 | https://feedback.bistudio.com/T186766 (via search summaries; site behind a Cloudflare challenge) + Steam threads 600766396226366202, 4632610189255644362 | "Millions of fake players": ~9 000 fake servers (Jan 2025), redirect to paid-item servers, one farm resolved Oct 2024, 255/255 patterns | B |
| S-53 | 2026-09-21 | https://steamcommunity.com/app/221100/discussions/0/570371410647592682/ | Claim that Steam's *concurrent player* statistic is trustworthy while A2S counts are self-reported; no per-server verified source | B |
| S-54 | 2026-09-21 | [tools/spoof_probe.js](../tools/spoof_probe.js) run on 128 addresses | 55 inflated (PLAYER empty), 25 unverifiable (no PLAYER reply), 13 genuine populated, 33 genuine empty; farm IPs | A |
| S-50 | 2026-09-21 | https://developer.valvesoftware.com/wiki/Master_Server_Query_Protocol (browser) + `Resolve-DnsName` on system, 1.1.1.1, 8.8.8.8 + Steam community thread 599667160179247033 (Oct 2025) | Legacy MSQP format (0x31, region, seed, filter; reply `FF FF FF FF 66 0A`, big-endian ports); `hl2master`/`hl1master.steampowered.com` = NXDOMAIN; `MasterServer2.vdf` no longer shipped in `Steam\config` | A (retired) |
| S-55 | 2026-09-21 | https://v2.tauri.app/plugin/updater/ (curl) | Static GitHub endpoint form `https://github.com/user/repo/releases/latest/download/latest.json`; dynamic endpoints may use `{{target}}`, `{{arch}}`, `{{current_version}}`; static manifest requires `version`, `platforms.<target>.url`, `platforms.<target>.signature` (`notes`, `pub_date` optional); Windows target key `windows-x86_64` | high |
| S-57 | 2026-09-21 | https://db-ip.com/db/download/ip-to-country-lite (page) + https://download.db-ip.com/free/dbip-country-lite-2026-09.csv.gz (4.5 MB gz, 717 170 rows) | Free "IP to Country Lite" database, CC BY 4.0, attribution "IP Geolocation by DB-IP"; updated monthly; CSV rows `start_ip,end_ip,country` (IPv4 and IPv6 mixed, IPv4 rows 357 325 covering 246 codes incl. `ZZ` unknown); rows are contiguous and already merged per country | high |
| S-58 | 2026-09-21 | https://github.com/lipis/flag-icons (README, LICENSE), npm `flag-icons` 7.5.0 | MIT; `flags/4x3/<cc>.svg` for 257 two-letter codes plus subdivisions; SVGs use `viewBox 0 0 640 480` and repeat ids (`clipPath id="a"`), so a composite sprite must prefix ids per flag | high |
| S-59 | 2026-09-21 | npm `@resvg/resvg-js` 2.6.2 (MPL-2.0, prebuilt native binary) | Dev-only SVG rasteriser used by `tools/flags_build.js`; nested `<svg x y width height>` elements render correctly | high |
| S-56 | 2026-09-21 | https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.11.5/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi (curl, 977 lines) | `.onInit`: currentUser default `$INSTDIR = $LOCALAPPDATA\${PRODUCTNAME}`, then `RestorePreviousInstallLocation` reads `HKCU\Software\<manufacturer>\<product>` default value; uninstall deletes that key only when "delete app data" is checked (lines 870–879); `RMDir $INSTDIR` is non-recursive; app-data removal targets `$APPDATA\<bundleId>` and `$LOCALAPPDATA\<bundleId>`; hooks NSIS_HOOK_PREINSTALL/POSTINSTALL/PREUNINSTALL/POSTUNINSTALL only | high |

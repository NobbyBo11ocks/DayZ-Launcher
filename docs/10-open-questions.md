# 10 · Open questions

Resolve each with a primary source or a local experiment, then move the answer to [09-decisions-log.md](09-decisions-log.md) and delete it here.

| ID | Question | Blocks | How to resolve |
|---|---|---|---|
| ~~Q1~~ | Resolved 2026-09-21 → D-010: official launcher log shows `DayZ_BE.exe` with `0 1 1 -exe DayZ_x64.exe "-mod=…"` | – | – |
| Q2 | Does `-nolauncher` change what Steam's own "Play" does on Windows, and does running `DayZ_x64.exe` directly (without `DayZ_BE.exe`) still start BattlEye? | M5 | Read `installscript.vdf` and Steam's launch config for 221100 (`appinfo.vdf` via `steamworks Apps::launch_command_line`), try both spawns against a BE server |
| ~~Q3~~ | Resolved 2026-09-21 → D-034: one anonymous entry per player (empty name, score 0, real duration) | – | – |
| Q4 | Side effects of `Client::init_app(221100)` while the game runs (overlay, "In-Game" status, two Steamworks clients) | M3/M5 | Official launcher already runs as 221100 alongside the game; confirm with steamworks-rs issue tracker and a local test |
| Q5 | GeoIP source and licence for country flags (DB-IP Lite CC-BY 4.0 vs MaxMind GeoLite2 account vs ip-api rate limits) | M4 | Compare DB sizes (< 5 MB target) and licence text; prefer an offline DB updated with the app |
| ~~Q6~~ | Resolved 2026-09-21 → D-018: `steamworks-sys` vendors `steam_api64.dll`/`.lib` under `lib/steam/redistributable_bin/win64`; bundle the DLL as a resource | – | – |
| Q7 | Do relative `-mod=!Workshop\@CF` paths work on Windows? (only needed if the 32 KB limit is ever approached) | none | Optional experiment |
| Q8 | Meaning of the per-mod u32 hash and the `overflow`/flag bytes | none | Not needed for matching; ignore unless Bohemia documents it (S-45) |
| Q9 | Do live servers ever send `idLen` 1/2/8 instead of 4? | robustness | Log unexpected values from the fan-out; parser already handles 1–8 |
| Q10 | Rate limits / servers that ignore RULES (official launcher logs "Server failed to respond to rules request") | M2 | Measure failure rate on a full sweep; keep DZSA fallback |
| Q12 | `rusqlite` (bundled) vs `tauri-plugin-sql` for the local DB | M3 | Check current versions on crates.io; prefer `rusqlite` for a single in-process DB with no JS surface |
| ~~Q13~~ | Resolved 2026-09-21 → D-052: the persona comes from Steamworks (`friends().name()`), no VDF parsing | – | – |
| ~~Q14~~ | Resolved 2026-09-21 → D-058: the NSIS installer puts `steam_api64.dll` beside `dayz-launcher.exe`; the installed app loaded it from there; silent install and uninstall leave nothing behind | – | – |
| ~~Q15~~ | Resolved 2026-09-21 → D-045: `noplayers` + `map` for the 10 maps with most empties, then a capped catch-all; the cap now only truncates fake servers | – | – |
| ~~Q16~~ | Resolved 2026-09-21 → D-057: releasing Steamworks after idle unloads the DLL but host private bytes stay at 61 MB; feature kept opt-in (`DAYZ_STEAM_IDLE_SECS`), default off | – | – |
| ~~Q17~~ | Resolved 2026-09-21 → D-056: removing `will-change: transform` from the virtual window lowered GPU commit 98 → 92 MB with no scrolling regression; `contain: strict` kept | – | – |
| Q18 | Two idle runs showed renderer memory and host CPU growing (up to 196 MB / 35 s in 8 min); two later runs were flat (65–72 MB, 0.15 % CPU) and thread attribution pointed outside our modules. Cause unexplained (D-060) | monitoring | Ship with a periodic self-measurement in Diagnostics later; if a user reports it, capture `Get-Process` thread CPU and Steam logs at the time |
| ~~Q19~~ | Resolved 2026-09-21 → D-067: custom NSIS template (stock 2.11.5 template with the currentUser default changed to `$LOCALAPPDATA\Programs\${PRODUCTNAME}`), verified by a silent install with no remembered location | – | – |
| Q11 | Code-signing certificate: **user decided 2026-09-21 to ship unsigned for now**. Windows SmartScreen will show "unknown publisher" on first run of the installer; revisit when a certificate is available | release | – |

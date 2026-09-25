<div align="center">

<img src="docs/art/banner.png" width="880" alt="DZSA CrayZ Launcher — a fast, honest server browser for DayZ Standalone. Player counts verified against the servers themselves; Workshop mods synced and the game started in one click; no accounts, no API keys, no telemetry.">

<br><br>

[![Latest release](https://img.shields.io/github/v/release/NobbyBo11ocks/DayZ-Launcher?display_name=tag&label=release&color=a3e635&labelColor=1d2530)](https://github.com/NobbyBo11ocks/DayZ-Launcher/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/NobbyBo11ocks/DayZ-Launcher/ci.yml?branch=main&label=CI&labelColor=1d2530)](https://github.com/NobbyBo11ocks/DayZ-Launcher/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/NobbyBo11ocks/DayZ-Launcher/total?color=2ea44f&labelColor=1d2530)](https://github.com/NobbyBo11ocks/DayZ-Launcher/releases)
[![Platform](https://img.shields.io/badge/Windows-10%20%2F%2011%20x64-0078d4?labelColor=1d2530)](#install)
[![Installer](https://img.shields.io/badge/installer-7.5%20MB-8957e5?labelColor=1d2530)](#install)
[![Telemetry](https://img.shields.io/badge/telemetry-none-2ea44f?labelColor=1d2530)](#what-it-talks-to)

### [⬇ Download for Windows](https://github.com/NobbyBo11ocks/DayZ-Launcher/releases/latest)

<sub>

[Screens](#a-look-around) · [What it does](#what-it-does) · [How it works](#how-it-works) · [Install](#install) · [Privacy](#what-it-talks-to) · [Build](#build-from-source) · [Docs](docs/00-README.md)

</sub>

<br>

<img src="docs/screenshots/servers.png" width="900" alt="The server browser: country flags, verified player counts, filter bar, and a details pane with the server's mods and population history">

</div>

---

## The problem it solves

DayZ's server list is full of servers that say they have forty players and have none. They do it because the list sorts by population, and an empty server nobody can see stays empty. Every launcher shows you that number.

This one asks the server directly, counts the players itself, and compares the answer with Steam's authenticated session count. A full sweep of this machine's list flagged **6 309 of 13 380 servers** — 47 % — and hid them by default.

That number is also why the automatic refresh asks Steam only for servers that have players: it arrives in about forty seconds and it skips the partitions where almost all of the fakes live. Of the 3 446 populated servers in the current cache, 25 did not verify and 20 of those are flagged and hidden — the difference is the five that refuse player queries while Steam vouches that someone is there. That vouch keeps a row visible only once a real head-count has been taken; until then the details pane says "Player count unconfirmed". Press **Refresh** when you want the empty ones too — a server you can be first on, or your own at an off-hour — and the list will say so when they are missing.

<table>
<tr><td width="33%" align="center">

### ✓ Verified

Every populated server queried directly with A2S and cross-checked against Steam — and against the previous check. Inflated, fabricated and re-drawn counts are flagged and hidden.

</td><td width="33%" align="center">

### ⚡ Quick

Rust host, plain Svelte 5, a virtualised table and a SQLite cache. First screen under half a second, installer 7.5 MB, a third of it the setup's own artwork.

</td><td width="33%" align="center">

### ⊘ Quiet

No accounts, no API keys, no telemetry. Every destination it contacts is listed below, and the news page can be switched off entirely.

</td></tr>
</table>

| Installer | Cold start to first frame | Refresh, then verify | Idle CPU | Memory, whole app |
|:-:|:-:|:-:|:-:|:-:|
| **7.5 MB** | **0.43 s** | **≈ 35 s + ≈ 25 s** | **0.2 %** of one core | **≈ 306 MB** |

<sub>Installer measured at v0.1.49, 2.71 MB of it the themed setup's artwork (D-258); memory and idle CPU at v0.1.23; cold start at v0.1.19 (D-136). The refresh figure is the list arriving, with verified counts following it: five real passes on 2026-09-23 ran 31.9–36.3 s for 2 273–2 619 servers, then 19.6–41.4 s to verify them. Budgets, method and history in [docs/05 §6](docs/05-architecture-and-optimisation.md).</sub>

---

## A look around

<table>
<tr>
<td width="33%" valign="top">

<img src="docs/screenshots/home.jpg" alt="Home: the latest DayZ updates with pictures and video previews">

**Home** — the latest DayZ posts with pictures and video previews, an unread badge on the rail, and a notification when an update lands. Switch the page off in Settings and nothing is ever fetched.

</td>
<td width="33%" valign="top">

<img src="docs/screenshots/settings.png" alt="Settings: appearance, server browser, news, Steam, launch options, launch profiles and updates">

**Settings** — a dark and a light theme, twelve accents, what the browser hides, how DayZ is started, saved launch profiles, and the Steam session with its idle release.

</td>
<td width="33%" valign="top">

<img src="docs/screenshots/logs.png" alt="Logs: what the launcher has been doing, with per-area mute chips and a recording switch">

**Logs** — what the launcher has been doing, both halves of it, with the file one click away, a chip per area to mute and a switch to stop recording altogether. Nothing leaves the machine.

</td>
</tr>
</table>

---

## What it does

<table>
<tr><th align="left" width="50%">Browse</th><th align="left" width="50%">Join</th></tr>
<tr valign="top"><td>

- Steam's own matchmaking list — no API key, the same list the in-game browser sees
- **Official or community hive**, perspective, map, country, mod, queue, password, daytime, version, ping and friends
- **PVE, PVP or RP** — read out of what the server calls itself, and labelled as the claim it is (48 % of the list says PVE)
- Search matches the description as well as the name, so "trader" finds servers whose name never says it
- **Maps by the name people use** — "Livonia" finds `enoch`, "Frostline" finds `sakhal`
- Country flags from an offline table, no lookups
- A details pane with the server's mods and installed ticks, 72 h population history, and the other servers at the same address
- Find every server running a given mod
- **Other servers at this address** in the details pane — half the list has a sibling, and 478 of those groups span more than one map
- Favourites, a clearable Recent list, and import of the official launcher's favourites
- LAN discovery

</td><td>

- Missing mods subscribed, downloaded and linked automatically, with **the rate and how long is left** — the median server needs 26 mods and the worst in this cache needs 139
- **Wait for a free slot** on a full server, then start the moment one opens
- Launch profiles: saved sets of launch options, picked in the join dialog
- Direct connect by address; the game port is resolved to the query port
- Workshop updates read from the **running Steam client**, not from a file it refreshes when it feels like it
- Joins that would be rejected are stopped before the game starts

</td></tr>
<tr><th align="left">Stay in touch</th><th align="left">Stay in control</th></tr>
<tr valign="top"><td>

- Home page with the latest DayZ updates, pictures and video previews
- An unread badge, and a notification when an update lands
- Friends' servers from Steam's game info and rich presence
- Friend markers on server rows, and a Friends filter

</td><td>

- Signed automatic updates
- Per-user installer; the launcher runs at the same elevation as Steam
- **Steam idle release**, so it does not count as playtime while it sits open
- A Logs page with per-area mutes and an off switch
- Confirmed clean-up of dangling `!Workshop` junctions
- A slim frameless window that remembers where it was, a dark and a light theme, twelve accent colours
- DZSA list fallback when Steam is unavailable

</td></tr>
</table>

---

## How it works

```mermaid
flowchart LR
  Steam[("Steam client")] -- matchmaking list --> Cache[("SQLite cache")]
  Servers["Game servers"] -- "A2S info · rules · players" --> Verify{"Trust rules<br/>R0–R12"}
  Verify -- verified --> Cache
  Verify -- inflated / fake --> Hidden["Hidden by default"]
  Cache --> UI["Server browser"]
  UI -- Join --> Sync["Workshop sync<br/>+ junctions"] --> BE["DayZ_BE.exe"]

  style Verify fill:#1d2530,stroke:#a3e635,color:#e6e9ee
  style Hidden fill:#1d2530,stroke:#f85149,color:#e6e9ee
  style BE fill:#1d2530,stroke:#a3e635,color:#e6e9ee
```

1. **List.** The Steamworks matchmaking API supplies the server list with pings, the same list the in-game browser sees. It is cached in SQLite so the previous list appears instantly and is refreshed in the background.
2. **Verify.** Populated servers are queried directly with A2S (INFO, RULES, PLAYER). The advertised count is checked against the player list, against Steam, and against the previous check: real sessions carry over between checks advanced by the gap, while a fabricated list is re-drawn on every query. A claim above the engine's 127-slot ceiling and an exact copy of a verified server's name on another address count against a row as well. Servers that fail the trust rules are marked inflated or fake. The rules, and what each one caught, are in [docs/11](docs/11-fake-population-detection.md).
3. **Join.** The join plan compares the server's mod list with your Workshop items, subscribes and downloads what is missing, creates `!Workshop\@<mod>` junctions matched by Workshop ID, and starts `DayZ_BE.exe` with the official argument form.
4. **Friends and news.** Friends' servers come from Steam's game info and rich presence. News comes from Steam's news feed for DayZ; pictures are shrunk on the host — 360 px for the cards, 640 for the featured one — so the window stays light.

> [!NOTE]
> Every protocol and launch fact is tied to a primary source in [docs/08](docs/08-sources.md), and every design decision to [docs/09](docs/09-decisions-log.md) — including the ones that turned out to be wrong and what replaced them.

---

## Install

1. Download `DZSA CrayZ Launcher_<version>_x64-setup.exe` from the [latest release](https://github.com/NobbyBo11ocks/DayZ-Launcher/releases/latest).
2. Run it. It installs per user to `%LOCALAPPDATA%\Programs\DZSA CrayZ Launcher` and adds a Start menu entry. WebView2 is installed silently if Windows does not have it.
3. Start Steam, then the launcher. Updates are automatic: each release is signed with a minisign key and the app verifies the signature before installing.

> [!WARNING]
> **SmartScreen.** The installer is not code-signed yet, so Windows shows "Windows protected your PC" on first run. Click **More info**, then **Run anyway**. Code signing is tracked in [docs/12](docs/12-code-signing.md).

**Requirements:** Windows 10 or 11, 64-bit · Steam running and signed in · DayZ installed through Steam.

---

## What it talks to

No accounts, no telemetry, no third-party analytics. In full:

| Destination | What for | When |
|---|---|---|
| **Steam, on your machine** | The server list, the Workshop, your friends | Always |
| **The game servers** | A2S queries for ping, mods and who is really on them | Refresh and verification |
| **GitHub** | The update manifest and the installer | Update checks |
| **Steam's news feed and image CDN** | The home page | Only with the News page on |
| **YouTube** | Preview images, and `youtube-nocookie.com` while a video plays | Only with the News page on |
| **DZSA's public list** | A fallback server list | Only when Steam is unavailable and you ask |
| **Microsoft** | The WebView2 runtime, if the installer finds none | Installing, once |

Your cache, favourites, history and settings stay in your own app-data folder. The Logs page shows exactly what the launcher has been doing, and you can copy it, mute it by area, or switch it off. One thing to know before pasting a report somewhere public: the join dialog's command line names your Steam profile and your library paths (the password is masked).

---

## Build from source

Prerequisites: Node 24, Rust 1.98 or newer via rustup, Visual Studio 2022 Build Tools with the C++ x64 workload, and WebView2 (already part of Windows 11).

```bash
npm install
```

```bash
npm run tauri dev
```

The seven checks CI runs, in order — `cargo fmt --check` is the one that catches people out:

```bash
npm run check
```

```bash
npm run build
```

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

```bash
node tools/nsis_template_check.js
```

```bash
node tools/workflow_check.js
```

<details>
<summary><b>Installer and updater signing</b></summary>

```bash
npm run tauri build
```

Output: `src-tauri/target/release/bundle/nsis/` (`DZSA CrayZ Launcher_<version>_x64-setup.exe` plus a `.sig` for the updater).

`src-tauri/nsis/installer.nsi` is a copy of Tauri's stock template carrying **eight deviations in fifteen marked blocks**, plus one new file (`nsis/hooks.nsh`) on Tauri's own extension point. Each deviation is bracketed by `; >>> dzl-change:` and carries the upstream lines it replaces, so `node tools/nsis_template_check.js` puts them all back and compares against the real thing rather than keeping a second copy of what we wrote (D-205):

1. The per-user default install directory is `%LOCALAPPDATA%\Programs\<product>` rather than `%LOCALAPPDATA%\<product>`, which collided with the official DayZ Launcher's data folder under this app's original name (D-067).
2. An options page before anything is written, so the news question is answered before the launcher has ever run (D-206), and `/NONEWS` for silent installs.
3. A straight upgrade skips the reinstall page entirely. Upstream shows it on every reinstall with *uninstall* pre-selected, and choosing it runs the previous version's uninstaller in the middle of the install — which is where the "delete application data" checkbox lives (D-207).
4. The uninstaller an upgrade does run is passed `/UPDATE /S`, as belt to that braces (D-205).
5. The Add/Remove Programs entry survives an upgrade, so an interrupted one still leaves a way back (D-214).
6. The install waits for the old binary's lock to clear before overwriting it, because WebView2's children outlive the host (D-214).
7. `AllowSkipFiles off`: NSIS otherwise skips a locked file, and a silent install exited 0 having changed nothing while writing the new version to Add/Remove Programs (D-205).
8. The launcher's own look on every page, uninstaller included: its dark surfaces, light text and lime accent, the logo beside the welcome and finish pages, the gas mask in the header, and a splash for an interactive install that silent, passive and update runs never show. Each picture comes in seven sizes, one per common display scale, and the installer loads the one made for the screen it is on (D-258).

Auto-updates were never affected by any of this — the updater always passes `/UPDATE`.

After upgrading `@tauri-apps/cli`, run the check (add `--write` to refresh the copy from the new tag and re-apply every marked change).

The logo is cut out of its source picture (`docs/art/logo-source.jpg`) by `node tools/logo_cutout.js`, and everything else is made from the result: the app icon and its gas-mask emblem by `tools/make_icon.js`, the installer's artwork by `tools/nsis_art.js`, the README banner by `tools/readme_banner.js`. So none of them can drift from the others.

Updater artifacts are signed with a minisign key. The private key lives outside the repository (`%USERPROFILE%\.tauri\dayz-launcher.key`); set it before building:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dayz-launcher.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build -- --ci
```

`--ci` stops the CLI from prompting for the key password (the key has none). Run this from Git Bash; PowerShell drops empty environment variables, and without the password variable the CLI waits on a prompt forever.

The public key is in `src-tauri/tauri.conf.json`; the updater polls `https://github.com/NobbyBo11ocks/DayZ-Launcher/releases/latest/download/latest.json`.

</details>

<details>
<summary><b>Releasing</b></summary>

### Through GitHub Actions (preferred)

Bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, commit, then push a matching tag:

```bash
git tag vX.Y.Z && git push origin main vX.Y.Z
```

The [release workflow](.github/workflows/release.yml) builds the signed installer on a clean Windows runner, creates the release as a draft with generated notes, uploads the installer, its `.sig` and `latest.json`, smoke-installs the result, publishes the draft only when that passes, and verifies the published manifest; a draft a failed run leaves is deleted. It needs **one** repository secret, set once from the machine that holds the key. In PowerShell:

```powershell
(Get-Content "$env:USERPROFILE\.tauri\dayz-launcher.key" -Raw).Trim() | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo NobbyBo11ocks/DayZ-Launcher
```

or from Git Bash:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo NobbyBo11ocks/DayZ-Launcher < ~/.tauri/dayz-launcher.key
```

`--repo` lets the command run from any directory; without it `gh` reads the repository from the current folder's git remote and fails outside a checkout.

`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is not needed: our key has no password and the workflow builds with `--ci`, so the CLI never prompts. PowerShell also cannot pass it, because it drops an empty `""` argument and has no `<` redirection.

Run the workflow manually from the Actions tab for a build-only dry run.

### By hand

1. Bump `version` in the three files above, then run the signed build.
2. Create the GitHub release with the installer and its signature:

   ```bash
   gh release create vX.Y.Z "src-tauri/target/release/bundle/nsis/DZSA CrayZ Launcher_<version>_x64-setup.exe" "src-tauri/target/release/bundle/nsis/DZSA CrayZ Launcher_<version>_x64-setup.exe.sig" --title vX.Y.Z --notes-file notes.md
   ```

3. Generate the update manifest from the uploaded asset (GitHub renames spaces in asset names to dots, so the URL must come from the API) and upload it:

   ```bash
   node tools/make_latest.js vX.Y.Z --notes notes.md
   ```

   ```bash
   gh release upload vX.Y.Z src-tauri/target/release/bundle/nsis/latest.json --clobber
   ```

4. Check the release the way the app will see it (fetches the manifest through the endpoint, downloads the installer, verifies the minisign signature against the public key):

   ```bash
   node tools/verify_update_sig.js
   ```

</details>

<details>
<summary><b>Repository map</b></summary>

```text
src/                 Svelte 5 front end: views in src/lib, runes state in src/lib/state
src-tauri/src/       Rust host
  a2s/               Query protocol: packets, INFO, RULES (mods), PLAYER, client
  browser/           Server model, SQLite cache, population trust rules
  steam/             Registry, library VDFs, Workshop junctions, Steamworks thread
  launch/            Argument builder, mod links, DayZ_BE.exe process
  news.rs            Steam news feed and host-made thumbnails
  proc.rs            Priority classes and elevation matching
  log.rs             The app's own log: memory ring plus a rotating file
  geoip.rs           Offline IP-to-country lookup read in place from the image
  http.rs            Capped response bodies for every outbound fetch
src-tauri/nsis/      Installer template (Tauri's stock template with seven marked deviations)
                     plus the header and sidebar bitmaps the setup wizard uses
docs/                Research, architecture, budgets, sources, decisions log
site/                The landing page
tools/               Node scripts: A2S probe and capture, GeoIP and flag builders, release helpers
```

</details>

<details>
<summary><b>Tools</b></summary>

| Command | Purpose |
|---|---|
| `node tools/a2s_probe.js <ip> <query-port>` | Query one server: INFO, RULES (mods) and PLAYER |
| `node tools/rules_failure_rate.js [sample] [concurrency]` | Sample live servers from the cache and report how many answer RULES |
| `node tools/verify_update_sig.js` | Fetch the live update manifest and verify the installer signature |
| `node tools/make_latest.js vX.Y.Z --notes notes.md` | Build `latest.json` from the uploaded release asset |
| `node tools/nsis_template_check.js` | Diff our NSIS template against the installed Tauri CLI's |
| `node tools/geoip_build.js` · `node tools/flags_build.js` | Rebuild the offline GeoIP table and the flag sprite |
| `node tools/logo_cutout.js` | Cut the logo out of `docs/art/logo-source.jpg` into `docs/art/logo.png`; only needed for a new source picture. Needs `sharp`, which is deliberately not a project dependency: `npm i --no-save sharp` first, and note that `npm ci` removes it again. The next three need it too |
| `node tools/make_icon.js` | Cut the gas mask out of the logo as the app icon and write `icon.ico` plus the PNG sizes |
| `node tools/nsis_art.js` · `node tools/readme_banner.js` | Rebuild the installer's artwork and the README banner from the logo |

</details>

---

## Credits

- IP geolocation by [DB-IP](https://db-ip.com) (IP to Country Lite, CC BY 4.0), compacted into `src-tauri/resources/geoip-v4.bin` by `tools/geoip_build.js`.
- Flag images from [flag-icons](https://github.com/lipis/flag-icons) (MIT), rasterised into `src/assets/flags.png` by `tools/flags_build.js`.
- Built with [Tauri](https://tauri.app), [Svelte](https://svelte.dev) and the [steamworks](https://crates.io/crates/steamworks) crate.

DayZ is a trademark of Bohemia Interactive. This project is not affiliated with or endorsed by Bohemia Interactive or Valve.

## License

**All rights reserved** ([LICENSE](LICENSE)). The source is public so you can read it, audit it and build it for your own use. Selling it, rebranding it, or redistributing it in any form needs written permission. Third-party components keep their own licences.

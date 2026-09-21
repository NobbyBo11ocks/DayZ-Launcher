# DayZ Launcher

Windows-only launcher for DayZ Standalone: key-free server list from Steam, direct A2S queries for ping and mods, one-click Workshop mod sync, launch through `DayZ_BE.exe`.

Stack: Tauri 2 (Rust) + Svelte 5 + Vite. Design and research live in [docs/](docs/00-README.md).

## Features

- Home page: a welcome band with your Steam name and avatar, quick facts and "Join again", then the latest DayZ posts from Steam with pictures and video previews, refreshed automatically, with an unread badge and an alert when an update lands.
- Server browser fed by Steam's own matchmaking list, with a virtualised table, filters (perspective, map, country, mod, queue, password, BattlEye, daytime, version, ping) and sorting.
- Player counts verified directly with each server and cross-checked against Steam, so inflated and fabricated counts are flagged or hidden.
- Country flags from an offline table; mods, population history and connected-session summary per server.
- One-click join: missing Workshop mods are subscribed and downloaded through Steam, `!Workshop` junctions are created the way the official launcher does it, and DayZ starts through BattlEye. Full servers can be waited for.
- Find servers by mod; mod management with updates and unsubscribe.
- Favourites with alerts (free slot, back online; Windows toast when the launcher is in the background), recent servers, direct connect, import of the official launcher's favourites.
- LAN tab (Steam's LAN discovery) and Friends tab: who is in DayZ, on which server, and a Join button.
- Launch profiles: save sets of launch options and pick one in the join dialog.
- If Steam is unavailable, the public DZSA list can be loaded instead so you can still browse.
- Signed automatic updates, a 4 MB per-user installer, a Diagnostics view with a Performance section and a confirmed clean-up of dangling `!Workshop` junctions, and a Steam idle release so the launcher does not count as playtime while it sits open.
- Slim frameless window that remembers its size and position; every view except the server list fits the window without scrolling.

## Develop

```bash
npm install
npm run tauri dev
```

## What it looks like

![Server browser with the details pane](docs/screenshots/servers.png)

Country flags, verified player counts, a mod filter, and a details pane with the server's mods, population history and connected sessions.

## Build installer

```bash
npm run tauri build
```

Output: `src-tauri/target/release/bundle/nsis/` (`DayZ Launcher_<version>_x64-setup.exe` plus a `.sig` for the updater).

The per-user installer defaults to `%LOCALAPPDATA%\Programs\DayZ Launcher`. Tauri's stock default, `%LOCALAPPDATA%\DayZ Launcher`, is the official DayZ Launcher's data folder, so `src-tauri/nsis/installer.nsi` is a copy of the stock template with that one line changed. After upgrading `@tauri-apps/cli`, run `node tools/nsis_template_check.js` (add `--write` to refresh the copy from the new tag).

Updater artifacts are signed with a minisign key. The private key lives outside the repository (`%USERPROFILE%\.tauri\dayz-launcher.key`); set it before building:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dayz-launcher.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build -- --ci
```

`--ci` stops the CLI from prompting for the key password (the key has none). Run this from Git Bash; PowerShell drops empty environment variables, and without the password variable the CLI waits on a prompt forever.

The public key is in `src-tauri/tauri.conf.json`; the updater polls `https://github.com/NobbyBo11ocks/dayz-launcher/releases/latest/download/latest.json`.

### Releasing through GitHub Actions (preferred)

Bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, commit, then push a matching tag:

```bash
git tag v0.1.4 && git push origin main v0.1.4
```

The [release workflow](.github/workflows/release.yml) builds the signed installer on a clean Windows runner, creates the release with generated notes, uploads the installer, its `.sig` and `latest.json`, smoke-installs the result, and verifies the published manifest. It needs two repository secrets, set once from this machine:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/dayz-launcher.key
```

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body ""
```

Run the workflow manually from the Actions tab for a build-only dry run.

### Releasing by hand

To publish a release from this machine:

1. Bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, then run the signed build above.
2. Create the GitHub release with the installer and its signature:

   ```bash
   gh release create v0.1.1 "src-tauri/target/release/bundle/nsis/DayZ Launcher_0.1.1_x64-setup.exe" "src-tauri/target/release/bundle/nsis/DayZ Launcher_0.1.1_x64-setup.exe.sig" --title v0.1.1 --notes-file notes.md
   ```

3. Generate the update manifest from the uploaded asset (GitHub renames spaces in asset names to dots, so the URL must come from the API) and upload it:

   ```bash
   node tools/make_latest.js v0.1.1 --notes notes.md
   ```

   ```bash
   gh release upload v0.1.1 src-tauri/target/release/bundle/nsis/latest.json --clobber
   ```

4. Check the release the way the app will see it (fetches the manifest through the endpoint, downloads the installer, verifies the minisign signature against the public key):

   ```bash
   node tools/verify_update_sig.js
   ```


The installer is not code-signed, so Windows SmartScreen shows an "unknown publisher" warning on first run.

## Data and credits

- Country flags: IP geolocation by [DB-IP](https://db-ip.com) (IP to Country Lite, CC BY 4.0), compacted into `src-tauri/resources/geoip-v4.bin` by `node tools/geoip_build.js` (re-run to pick up the current month, then commit). Flag images from [flag-icons](https://github.com/lipis/flag-icons) (MIT), rasterised into `src/assets/flags.png` by `node tools/flags_build.js`.
- Everything else comes from Steam and the game servers themselves; the launcher sends nothing anywhere else.


## Verify the DayZ query protocol

```bash
node tools/a2s_probe.js <server-ip> <query-port>
```

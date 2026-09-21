# DayZ Launcher

Windows-only launcher for DayZ Standalone: key-free server list from Steam, direct A2S queries for ping and mods, one-click Workshop mod sync, launch through `DayZ_BE.exe`.

Stack: Tauri 2 (Rust) + Svelte 5 + Vite. Design and research live in [docs/](docs/00-README.md).

## Develop

```bash
npm install
npm run tauri dev
```

## Build installer

```bash
npm run tauri build
```

Output: `src-tauri/target/release/bundle/nsis/` (`DayZ Launcher_<version>_x64-setup.exe` plus a `.sig` for the updater).

Updater artifacts are signed with a minisign key. The private key lives outside the repository (`%USERPROFILE%\.tauri\dayz-launcher.key`); set it before building:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dayz-launcher.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build -- --ci
```

`--ci` stops the CLI from prompting for the key password (the key has none). Run this from Git Bash; PowerShell drops empty environment variables, and without the password variable the CLI waits on a prompt forever.

The public key is in `src-tauri/tauri.conf.json`; the updater polls `https://github.com/NobbyBo11ocks/dayz-launcher/releases/latest/download/latest.json`. To publish a release, bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, build, then create a GitHub release tagged `v<version>` with three assets: the installer, its `.sig`, and a `latest.json` like this (GitHub rewrites spaces in asset names to dots, hence `DayZ.Launcher` in the URL):

```json
{
  "version": "0.1.1",
  "notes": "What changed",
  "pub_date": "2026-09-21T16:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of DayZ Launcher_0.1.1_x64-setup.exe.sig>",
      "url": "https://github.com/NobbyBo11ocks/dayz-launcher/releases/download/v0.1.1/DayZ.Launcher_0.1.1_x64-setup.exe"
    }
  }
}
```

```bash
gh release create v0.1.1 "src-tauri/target/release/bundle/nsis/DayZ Launcher_0.1.1_x64-setup.exe" "src-tauri/target/release/bundle/nsis/DayZ Launcher_0.1.1_x64-setup.exe.sig" latest.json --title "v0.1.1" --notes "What changed"
```

The installer is not code-signed, so Windows SmartScreen shows an "unknown publisher" warning on first run.


## Verify the DayZ query protocol

```bash
node tools/a2s_probe.js <server-ip> <query-port>
```

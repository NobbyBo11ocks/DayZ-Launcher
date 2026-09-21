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

The public key is in `src-tauri/tauri.conf.json`; change the updater endpoint there to your release host. The installer is not code-signed, so Windows SmartScreen shows an "unknown publisher" warning on first run.

## Verify the DayZ query protocol

```bash
node tools/a2s_probe.js <server-ip> <query-port>
```

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

The per-user installer defaults to `%LOCALAPPDATA%\Programs\DayZ Launcher`. Tauri's stock default, `%LOCALAPPDATA%\DayZ Launcher`, is the official DayZ Launcher's data folder, so `src-tauri/nsis/installer.nsi` is a copy of the stock template with that one line changed. After upgrading `@tauri-apps/cli`, run `node tools/nsis_template_check.js` (add `--write` to refresh the copy from the new tag).

Updater artifacts are signed with a minisign key. The private key lives outside the repository (`%USERPROFILE%\.tauri\dayz-launcher.key`); set it before building:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dayz-launcher.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build -- --ci
```

`--ci` stops the CLI from prompting for the key password (the key has none). Run this from Git Bash; PowerShell drops empty environment variables, and without the password variable the CLI waits on a prompt forever.

The public key is in `src-tauri/tauri.conf.json`; the updater polls `https://github.com/NobbyBo11ocks/dayz-launcher/releases/latest/download/latest.json`. To publish a release:

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


## Verify the DayZ query protocol

```bash
node tools/a2s_probe.js <server-ip> <query-port>
```

# 04 · Tech stack decision (ADR-001)

Status: **accepted 2026-09-21**. Windows-only (decided 2026-09-21). Every version below was read from the package registry on 2026-09-21; re-verify with the commands in §5 before pinning.

## 1. Decision

| Layer | Choice | Version (verified) | Source |
|---|---|---|---|
| Shell / backend | **Tauri 2** (Rust) | `tauri` 2.11.6 (crates.io, updated 2026-09-19); `@tauri-apps/cli` 2.11.5; `@tauri-apps/api` 2.11.1 | S-20, S-21, S-22 |
| Language (backend) | Rust stable | 1.98.1 (2026-09-03), installed via rustup 2026-09-21 (SHA-256 verified, D-017) | S-23, S-49 |
| Async runtime | tokio | 1.53.1 | S-24 |
| Steam integration | `steamworks` crate (Steamworks SDK bindings) | 0.13.1 | S-25 |
| NTFS junctions | `junction` | 2.0.0 | S-26 |
| Registry | `winreg` | 0.56.0 | S-27 |
| VDF/ACF parsing | `keyvalues-parser` | 0.2.4 | S-28 |
| UI framework | **Svelte 5** (runes), plain Svelte + Vite, **no SvelteKit** (create-tauri-app's `svelte-ts` template now generates SvelteKit; replaced, D-015) | 5.57.1 | S-29, S-46 |
| Bundler | Vite | 8.3.0 (needs Node ^20.19 or ≥ 22.12) | S-30 |
| Svelte↔Vite | `@sveltejs/vite-plugin-svelte` | 7.3.0 (peer: vite ^8, svelte ^5.46.4) | S-31 |
| Types | TypeScript | **6.0.3** (7.0.2 is newest, but `svelte-check` 4.7.6 peers on `^5 \|\| ^6`; revisit when svelte-check accepts 7, D-016) | S-32, S-47 |
| Node (dev only) | Node.js | 24.18.1 installed locally, npm 11.16.0 | S-41 |
| Web runtime | WebView2 Evergreen | 153.0.4234.48 installed locally | S-41 |
| Installer | Tauri NSIS, `webviewInstallMode: downloadBootstrapper`, per-user | Tauri docs | S-33 |
| Updater | `tauri-plugin-updater` (signed artifacts, GitHub Releases JSON) | Tauri plugins | S-34 |

Tauri 3.0.0-alpha.1 exists on crates.io but is pre-release; stay on 2.11.x.

## 2. Why Tauri over the alternatives

| Option | Verdict | Reason |
|---|---|---|
| Electron | rejected | 85–200 MB bundles, ~120–170 MB idle RAM, ~1.4 s cold start in 2026 benchmarks (S-35). Beans' ~100 MB download is the cautionary example. |
| .NET 10 WPF / WinUI 3 | rejected | Good native perf, but the official DayZ launcher is WPF (.NET Framework 4.5.1, verified from `DayZLauncher.exe.config`) and looks dated; theming and modern layout cost far more effort than CSS. WinUI 3 packaging (Windows App SDK runtime) adds friction. |
| Avalonia / Flutter | rejected | Larger runtime, weaker text/layout tooling than the web stack for a data-dense table UI, smaller pool of components. |
| Tauri 2 + WebView2 | **chosen** | 3–10 MB bundle, ~40–50 MB idle for the Rust core plus WebView2's shared processes, ~0.4 s cold start (S-35). WebView2 is already present on Windows 10/11 so nothing heavy ships. Rust gives us zero-cost UDP fan-out for A2S and safe FFI to Steamworks. |

## 3. Why Svelte 5 over React/Solid

- Svelte compiles away the framework: ~47 KB baseline app vs ~156 KB React 19 in 2026 comparisons (S-36). Solid is comparable (~7 KB runtime) but Svelte 5 runes give the same fine-grained reactivity with a larger component ecosystem and simpler mental model.
- No virtual DOM diffing on a 20 000-row server list; we virtualise anyway, but the per-row update cost matters when pings stream in.
- React is rejected purely on footprint and re-render cost; ecosystem size is irrelevant for a launcher.

## 4. Steam integration model (ADR-002)

Two ways exist to get a **complete** server list without a Steam Web API key:

1. **Steamworks `ISteamMatchmakingServers::RequestInternetServerList(221100)`** through the `steamworks` crate (`MatchmakingServers::internet_server_list`, verified in docs.rs for 0.13.1, S-25). Requires Steam running and `Client::init_app(221100)`; Steam will show the user as "in DayZ" while the launcher runs (same as the official launcher, which Steam itself launches as app 221100).
2. ~~Raw **Master Server Query Protocol** (UDP to `hl2master.steampowered.com:27011`)~~: **retired by Valve** (NXDOMAIN on every resolver tested 2026-09-21, D-031). Not an option.

Because one-click mod sync **requires** Steamworks anyway (`UGC::subscribe_item`, `download_item`, `item_state`, `item_install_info`, all present in 0.13.1), option 1 is chosen. `steam_api64.dll` (vendored by `steamworks-sys`) is bundled as a Tauri resource next to the exe. Fallbacks for the list only: a user-supplied Steam Web API key (`IGameServersService/GetServerList`, optional setting), then the DZSA list. BattleMetrics remains link-out only.

## 5. Re-verification commands

```bash
npm view svelte version && npm view vite version && npm view @tauri-apps/cli version && npm view @tauri-apps/api version && npm view typescript version && npm view @sveltejs/vite-plugin-svelte version
```

```bash
cargo search tauri --limit 1 && cargo search steamworks --limit 1 && cargo search junction --limit 1 && cargo search winreg --limit 1 && cargo search keyvalues-parser --limit 1 && cargo search tokio --limit 1
```

## 6. Prerequisites (status 2026-09-21)

- Rust toolchain: **installed**, stable 1.98.1 `x86_64-pc-windows-msvc`, `%USERPROFILE%\.cargo\bin` added to the user PATH. VS 2022 Build Tools with C++ x64 were already present.
- Steamworks redistributable: `steamworks-sys` vendors `lib/steam/redistributable_bin/win64/steam_api64.dll` (317 080 bytes) and `.lib` (S-48); the crate targets SDK 1.64 and MSRV 1.80. Ship that DLL as a Tauri resource in M3 (D-018).

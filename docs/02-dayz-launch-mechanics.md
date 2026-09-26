# 02 · DayZ launch mechanics on Windows

All paths and values below were read from **this machine** on 2026-09-21 (S-41) unless another source is cited. Windows-only by decision.

## 1. Identifiers

| Item | Value | Evidence |
|---|---|---|
| DayZ client App ID | **221100** | `steam_appid.txt` in the game folder, `appmanifest_221100.acf`, A2S GameID |
| DayZ dedicated server App ID | 223350 | present in `libraryfolders.vdf` apps map |
| Workshop content App ID | 221100 (same as client) | `steamapps\workshop\content\221100\` |
| Current client version | 1.29.163709 | `DayZ_x64.exe` ProductVersion `1.29.0.163709` **from the `StringFileInfo` string table** (the fixed `VS_FIXEDFILEINFO` block has 16-bit fields and cannot hold 163709, D-025); A2S reports `1.29.163709`; RULES `requiredVersion=129` |
| Current build id | 24689949 | `appmanifest_221100.acf` `buildid`, `appworkshop_221100.acf` `LastBuildID` |

## 2. Locating Steam and the game

1. Registry (`winreg` crate):
   - `HKCU\Software\Valve\Steam` → `SteamPath` (`c:/program files (x86)/steam`, forward slashes, lower case) and `SteamExe`.
   - `HKLM\SOFTWARE\WOW6432Node\Valve\Steam` → `InstallPath` (`C:\Program Files (x86)\Steam`).
   - `HKCU\Software\Valve\Steam\ActiveProcess` → `pid` and `ActiveUser` (non-zero when a user is logged in; live: ActiveUser 1824600665). **`pid` goes stale**: it read 16884 while the live `steam.exe` was 15176 (D-026). Detect liveness by enumerating processes (`CreateToolhelp32Snapshot`) for `steam.exe` whose image path is under `SteamPath`; use the registry pid only as a hint.
2. Libraries: parse `<SteamPath>\steamapps\libraryfolders.vdf` (`keyvalues-parser`). Shape:

```text
"libraryfolders" { "0" { "path" "C:\\Program Files (x86)\\Steam" "label" "" "contentid" "…" "totalsize" "0" "apps" { "228980" "417894432" } }
                   "1" { "path" "G:\\SteamLibrary" … "apps" { "221100" "25563415305" "223350" "3979187009" … } } }
```

   The `apps` map tells you which library holds 221100 without touching the disk. Live: DayZ is in `G:\SteamLibrary`.
3. Install dir: `<lib>\steamapps\appmanifest_221100.acf` → `installdir` (`DayZ`), so the game folder is `<lib>\steamapps\common\DayZ`. `StateFlags` 4 = fully installed. Also `LastUpdated`, `SizeOnDisk`, `buildid`.
4. Game folder contents (live): `Addons\`, `BattlEye\`, `dta\`, `Launcher\`, `MainMenu.ChernarusPlus`, `MainMenu.Sakhal`, `sakhal\`, `!Workshop\`, `DayZ_x64.exe`, `DayZ_BE.exe`, `DayZDiag_x64.exe`, `DayZLauncher.exe` (+ `.config`: WPF, .NET Framework 4.5.1, log4net), `CrashReporter.exe`, `DayZUninstaller.exe`, `steam_api64.dll`, `steam_appid.txt`, `installscript.vdf`, `dayz.gproj`, `amd_ags_x64.dll`.

Fallback when the registry is missing: let the user pick `DayZ_x64.exe` in a file dialog and derive everything from that path.

## 3. Workshop layout

- Item folder: `<lib>\steamapps\workshop\content\221100\<publishedId>\` containing `addons\`, `keys\`, `meta.cpp`, `mod.cpp`.
- `meta.cpp` (live, CF):

```text
protocol = 1;
publishedid = 1559212036;
name = "CF";
timestamp = 5250757174595880000;
```

- `mod.cpp` (live, CF): `name = "Community Framework"; picture/logo/logoSmall/logoOver = "…edds"; tooltip; overview; action = "https://…"; author; authorID; version = "1.5.8";`
- `<lib>\steamapps\workshop\appworkshop_221100.acf` (live excerpt):

```text
"AppWorkshop" { "appid" "221100" "SizeOnDisk" "193223329" "NeedsUpdate" "0" "NeedsDownload" "0"
  "TimeLastUpdated" "1789986498" "TimeLastFullCheck" "1789986509" "TimeLastAppRan" "1784847537" "LastBuildID" "24689949"
  "WorkshopItemsInstalled" { "1559212036" { "size" "527656" "timeupdated" "1771519119" "manifest" "4114705373119672275" } … }
  "WorkshopItemDetails" { … } }
```

  This file is the cheapest "what is installed and is it current" signal: read it on startup, watch it for changes (`notify` crate) while Steam downloads.

- **Three different names exist for one mod.** Match by ID only.

| Where | Example for 1559212036 |
|---|---|
| `meta.cpp` `name` (= Workshop title at publish time) | `CF` |
| `mod.cpp` `name` (what A2S_RULES reports) | `Community Framework` |
| Junction folder | `@CF` |

## 4. The `!Workshop` junction folder (official launcher behaviour, verified)

`<game>\!Workshop\` holds one **NTFS junction** per subscribed mod, named `@<meta.cpp name>` (spaces allowed, e.g. `@Dabs Framework`), pointing at `<lib>\steamapps\workshop\content\221100\<id>`. Live: 20 junctions plus a marker file `!DO_NOT_CHANGE_FILES_IN_THESE_FOLDERS`. **16 of the 20 are dangling** (the official launcher does not remove junctions when Workshop content is deleted; only 4 items are actually installed), so an inventory must read junctions with `symlink_metadata` + reparse data, never by following them (D-027).

Junctions need no admin rights (unlike symlinks), which is why the official launcher uses them. Use the `junction` crate. We reuse the **same folder and naming** so the official launcher, DZSA and ours interoperate; create a junction only when missing, never delete ones we did not create. The one removal is the Mods page's confirmed clean-up, which takes only a junction whose target is a `…\steamapps\workshop\content\221100\<id>` folder that is not found (not one that cannot be read), and deletes it by the path it was listed at (D-093, D-276).

## 5. Launch command line

### 5.1 What the official launcher passes to the game (primary evidence: game RPT logs)

`%LOCALAPPDATA%\DayZ\DayZ_x64_2026-07-23_23-56-35.RPT`:

```text
== "G:\SteamLibrary\steamapps\common\DayZ\DayZ_x64.exe" "-mod=G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@CF;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@Dabs Framework;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@DayZ-Editor"
```

- `-mod=` takes **absolute paths** to the junctions, `;`-separated, the whole `-mod=…` as **one quoted argument** (spaces inside are fine because of the quotes).
- **Order: the reverse of RULES.** The official launcher put `@CF` first, and the engine then processed the list back to front (Dabs Framework before CF in the same RPT). A server's RULES lists its mods frameworks last — the reverse of the `-mod=` its admin wrote — so a client that passes RULES as it comes runs the server's load order backwards. The launcher reverses RULES (D-265, S-89); the engine still sorts declared dependencies, so the difference shows only among mods that do not declare each other: `modded class` chains, file overrides.
- Load order matters: dependencies (CF, Dabs Framework) first. Use the order the server reports in A2S_RULES.

### 5.2 BattlEye wrapper

**Verified (confidence A)** from the official launcher's own log, `%LOCALAPPDATA%\DayZ Launcher\Logs\Launcher.log`, 2026-07-23 23:56:31:

```text
GameExecutor: Starting the game, process path: G:\SteamLibrary\steamapps\common\DayZ\DayZ_BE.exe
GameExecutor:                    parameters:   0 1 1 -exe DayZ_x64.exe "-mod=G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@CF;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@Dabs Framework;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@DayZ-Editor"
```

So the official launcher spawns `DayZ_BE.exe` with the fixed prefix `0 1 1 -exe DayZ_x64.exe` followed by the game arguments; the RPT in 5.1 is the same launch seen from the game's side. The same log shows `GameExecutor: Game exited (with exit code: N)`, so the launcher waits on the BE process to detect exit. DZSA's log (S-13) shows the identical convention. The meaning of the three numbers is undocumented; keep them verbatim.

If the convention ever changes, re-capture it with:

```powershell
Get-CimInstance Win32_Process -Filter "Name='DayZ_BE.exe' OR Name='DayZ_x64.exe'" | Select-Object Name, CommandLine
```

### 5.3 Our launch line (target)

Working directory = game folder. Spawn `DayZ_BE.exe` with:

```text
0 1 1 -exe DayZ_x64.exe "-mod=<abs>;<abs>;…" -connect=<ip> -port=<gamePort> -name=<profileName> [-password=<pw>] [-nosplash -skipintro -noPause] [user extras]
```

- `<gamePort>` is the A2S_INFO EDF port (2402 in the live example), **not** the query port (27017).
- Steam must be running and logged in (`ActiveProcess\ActiveUser != 0`); the game loads `steam_api64.dll` and refuses to start otherwise.
- Do not launch through `steam://run/221100//…`: Steam's launch config for DayZ starts `DayZLauncher.exe` (Q2), and the Linux scripts only get away with `-applaunch … -nolauncher` under Proton.
- Windows `CreateProcess` command lines are limited to 32 767 characters; 40 mods × ~90 chars ≈ 3.6 KB, so absolute paths are safe. Relative `-mod=!Workshop\@CF` is unverified on Windows (Q7); stay with absolute.

### 5.4 Client parameters (S-44, S-07, S-05; B unless marked)

| Parameter | Meaning |
|---|---|
| `-connect=<ip>` / `-port=<gamePort>` | join a server directly |
| `-password=<pw>` | server password |
| `-name=<name>` | profile/character name shown in game; some servers require it. Official launcher exposes `name` (its `Parameters.json` favourites: `["window","name"]`) |
| `-mod=<a>;<b>` | client mods (absolute paths, see 5.1) **[A]** |
| `-nolauncher` | skip the official launcher when Steam starts the game |
| `-nosplash`, `-skipintro` | skip splash/intro |
| `-noPause` | keep running when unfocused |
| `-window` | windowed mode (official launcher exposes `window`) |
| `-profiles=<dir>` | profile folder (default `%USERPROFILE%\Documents\DayZ`) |
| `-cpuCount=<n>`, `-maxMem=<MB>`, `-maxVRAM=<MB>` | performance limits the 1.29 client still parses (S-90): threads to use, and the physical-memory and video-memory ceilings. The launcher adds all three sized to the PC it runs on — every active thread, physical RAM less 2 GB (above 4 GB), the largest GPU's dedicated memory from 2 GB up — and leaves out any key the extra arguments set themselves (D-267, S-91) |
| `-exThreads=<n>`, `-enableHT`, `-malloc=`, `-high` | Arma-era options **the DayZ 1.29 executable does not contain** (S-90): passing them does nothing. Process priority is set by the launcher itself (D-118) |
| `-world=empty` | Linux launcher passes it to skip loading the menu world (C); `world=` is not in the 1.29 executable's option table (S-90), so its effect is unconfirmed |
| `-filePatching`, `-doLogs`, `-BEpath=` | server/diag oriented, not exposed |

## 6. Steam Workshop subscribe and download (Steamworks, S-25)

The official launcher does exactly this: `Launcher.log` shows `Launcher.Steam.LauncherSteamFacade: Steam init result: Online`, `SteamUgcDownloadManager: Steam UGC Download manager initialization 0` and `SteamUgcDownloadManager: Item was installed: 1559212036`, plus `ModSignatureCalculator` PBO/bisign caching (S-41).

**Implemented and verified in M5 (D-051):** `steam/sdk.rs` runs the flow below on the Steam thread; `launch/mods.rs` creates the junction; `launch/args.rs` + `launch/process.rs` start the game. A live run downloaded Ear-Plugs (107 kB) in 1.3 s, created `!Workshop\@Ear-Plugs`, and DayZ started with `-connect`/`-port`/`-name` through `DayZ_BE.exe`.

Flow for "Join" on a server with missing mods:

1. `Client::init_app(221100)` once at startup on a dedicated thread; `run_callbacks()` every ~50 ms on that thread.
2. For each required Workshop ID: `ugc.item_state(id)` → bitflags `Subscribed`, `Installed`, `NeedsUpdate`, `Downloading`, `DownloadPending`.
3. If not subscribed: `ugc.subscribe_item(id)` (async result callback). Then `ugc.download_item(id, high_priority = true)` to start immediately instead of waiting for Steam's scheduler.
4. Progress: `ugc.item_download_info(id)` → `(bytes_downloaded, bytes_total)`; completion via the `DownloadItemResult` callback.
5. Path: `ugc.item_install_info(id)` → `{ folder, size_on_disk, timestamp }`; read `meta.cpp` `name`, create `!Workshop\@<name>` junction if missing.
6. Only then build the `-mod=` line.

Without Steamworks (fallback/diagnostics only): `appworkshop_221100.acf` + folder existence, and open `https://steamcommunity.com/sharedfiles/filedetails/?id=<id>` for manual subscribe. Mod titles/sizes for not-yet-installed items: `POST https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/` with `itemcount` and `publishedfileids[i]` (no key, S-39).

## 7. Official launcher local data (for an import feature)

`%LOCALAPPDATA%\DayZ Launcher\`:

- `FavouriteServers.xml` — `<Server IpAddress="1669946347" Port="5002" QueryEndPoint="99.137.91.235:5003" ConnectionEndPoint="99.137.91.235:5002" Name="…" Map="chernarusplus" MaxPlayers="10" ServerVersion="129163451" RequiredVersion="129" Tags="battleye,no3rd,external,privHive,shardABC123,lqs0,entm1.000000,11:54" …/>` (IpAddress is the IPv4 as a big-endian uint32). **Imported by `steam/official.rs` (M6, D-053)**: `QueryEndPoint` gives the row id, `ConnectionEndPoint` the game port; the server is probed live and otherwise stored from these fields.
- `Presets\*.dayz.defaultpreset2` etc.: XML `<addons-presets><published-ids><id>steam:1559212036</id>…`.
- `ServerBrowser.Settings.json`, `Servers.Banned.json`, `Parameters.json`, `Local.json`, `bisignCache.nson`, `pboCache.nson`, `Logs\`.
- `Launcher\Config.json` in the game folder lists Bohemia endpoints, including news: `https://dayz.com/api/v1.0/launcher?rowsPerPage=10&page={0}&absolute=1` and posts `https://dayz.com/api/v1.0/post/{0}`.

Game data: `%LOCALAPPDATA%\DayZ\` (RPT and crash logs), `%USERPROFILE%\Documents\DayZ\` (`<user>.core.xml`, `chars.DayZProfile`, `DayZ.cfg`, `profile.vars.DayZProfile`).

Default `-name`: Steam persona from `<SteamPath>\config\loginusers.vdf` (`PersonaName` of the entry with `MostRecent 1`) — confidence B, verify when implementing.

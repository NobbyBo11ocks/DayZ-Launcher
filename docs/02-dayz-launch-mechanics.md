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

Without either registry key there is no fallback (a file picker was planned and never built): Steam counts as not installed, and the Mods page and the join say so (`steam/diagnostics.rs`, `launch_game`).

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

  This file is the cheapest "what is installed and is it current" signal. The inventory (`steam/diagnostics.rs`) reads it; the Mods page's update badge asks Steamworks (`mods_stale`) five seconds after start and every 15 minutes, and uses this file's `NeedsUpdate` when Steam cannot be asked (D-191, D-275). Nothing watches the file for changes.

- **Three different names exist for one mod.** Match by ID only.

| Where | Example for 1559212036 |
|---|---|
| `meta.cpp` `name` (= Workshop title at publish time) | `CF` |
| `mod.cpp` `name` (what A2S_RULES reports) | `Community Framework` |
| Junction folder | `@CF` |

## 4. The `!Workshop` junction folder (official launcher behaviour, verified)

`<game>\!Workshop\` holds one **NTFS junction** per subscribed mod, named `@<meta.cpp name>` (spaces allowed, e.g. `@Dabs Framework`), pointing at `<lib>\steamapps\workshop\content\221100\<id>`. Live: 20 junctions plus a marker file `!DO_NOT_CHANGE_FILES_IN_THESE_FOLDERS`. **16 of the 20 are dangling** (the official launcher does not remove junctions when Workshop content is deleted; only 4 items are actually installed), so an inventory must read junctions with `symlink_metadata` + reparse data, never by following them (D-027).

Junctions need no admin rights (unlike symlinks), which is why the official launcher uses them. Use the `junction` crate. We reuse the **same folder and naming** so the official launcher, DZSA and ours interoperate; create a junction only when missing, never delete ones we did not create. Names we create are printable ASCII only: DayZ reads its command line in the ANSI code page, where Windows' best-fit conversion would turn look-alike characters in a mod's name into quotes or separators (D-283). The one removal is the Mods page's confirmed clean-up, which takes only a junction whose target is a `…\steamapps\workshop\content\221100\<id>` folder that is not found (not one that cannot be read), and deletes it by the path it was listed at (D-093, D-276).

## 5. Launch command line

### 5.1 What the official launcher passes to the game (primary evidence: game RPT logs)

`%LOCALAPPDATA%\DayZ\DayZ_x64_2026-07-23_23-56-35.RPT`:

```text
== "G:\SteamLibrary\steamapps\common\DayZ\DayZ_x64.exe" "-mod=G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@CF;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@Dabs Framework;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@DayZ-Editor"
```

- `-mod=` takes **absolute paths** to the junctions, `;`-separated, the whole `-mod=…` as **one quoted argument** (spaces inside are fine because of the quotes).
- **Order: the reverse of RULES.** The official launcher put `@CF` first, and the engine then processed the list back to front (Dabs Framework before CF in the same RPT). A server's RULES lists its mods frameworks last — the reverse of the `-mod=` its admin wrote — so a client that passes RULES as it comes runs the server's load order backwards. The launcher reverses RULES (D-265, S-89); the engine still sorts declared dependencies, so the difference shows only among mods that do not declare each other: `modded class` chains, file overrides.
- Load order matters, so the launcher passes the server's RULES list reversed, as above. Passing it in RULES order was D-008's rule, which D-265 corrected.

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
0 1 1 -exe DayZ_x64.exe "-mod=<abs>;<abs>;…" -connect=<ip> -port=<gamePort> [-password=<pw>] [-name=<name>] [-skipintro] [-nosplash] [-noPause] [-cpuCount=<n>] [-maxMem=<MB>] [-maxVRAM=<MB>] [extras]
```

- `<gamePort>` is the A2S_INFO EDF port (2402 in the live example), **not** the query port (27017).
- `-mod=` is the server's RULES list reversed (5.1). `-password` only when there is one; `-name` is the name from Settings or the chosen launch profile, else the Steam persona, and is left out when both are empty (D-052). The performance keys are sized to the PC unless the extras set them (D-267, 5.4). Extras that set `-connect`, `-port`, `-mod`, `-password` or `-name` are dropped, because the join sets those and DayZ could take the later one (D-265). The password is masked wherever the line is shown (D-163).
- Steam must be running and logged in (`ActiveProcess\ActiveUser != 0`); the game loads `steam_api64.dll` and refuses to start otherwise.
- Do not launch through `steam://run/221100//…`: Steam's launch config for DayZ starts `DayZLauncher.exe` (Q2), and the Linux scripts only get away with `-applaunch … -nolauncher` under Proton.
- Windows `CreateProcess` command lines are limited to 32 767 characters. The heaviest cached mod list, 121 mods, spells out to ≈ 8 100 characters of `-mod=`, a quarter of that, so absolute paths are safe and relative `!Workshop\@…` paths are unnecessary (Q7 closed, D-140, S-76).

### 5.4 Client parameters (S-44, S-07, S-05; B unless marked)

| Parameter | Meaning |
|---|---|
| `-connect=<ip>` / `-port=<gamePort>` | join a server directly |
| `-password=<pw>` | server password |
| `-name=<name>` | profile/character name shown in game; some servers require it. Official launcher exposes `name` (its `Parameters.json` favourites: `["window","name"]`) |
| `-mod=<a>;<b>` | client mods (absolute paths, see 5.1) **[A]** |
| `-nolauncher` | skip the official launcher when Steam starts the game |
| `-nosplash`, `-skipintro` | skip splash/intro; `skipintro` is not in the 1.29 executable at all (S-90), though the game's scripts may still read it, so the launcher passes it when the setting is on |
| `-noPause` | keep running when unfocused |
| `-window` | windowed mode (official launcher exposes `window`) |
| `-profiles=<dir>` | profile folder (default `%USERPROFILE%\Documents\DayZ`) |
| `-cpuCount=<n>`, `-maxMem=<MB>`, `-maxVRAM=<MB>` | performance limits the 1.29 client still parses (S-90): threads to use, and the physical-memory and video-memory ceilings. The launcher adds all three sized to the PC it runs on — every active thread, physical RAM less 2 GB (above 4 GB), the largest GPU's dedicated memory from 2 GB up — and leaves out any key the extra arguments set themselves (D-267, S-91) |
| `-exThreads=<n>`, `-enableHT`, `-malloc=`, `-high` | Arma-era options **the DayZ 1.29 executable does not contain** (S-90): passing them does nothing. The launcher never changes DayZ's priority: it runs itself at High and drops to Below normal from the spawn until the game exits (D-119, D-151) |
| `-world=empty` | Linux launcher passes it to skip loading the menu world (C); `world=` is not in the 1.29 executable's option table (S-90), so its effect is unconfirmed |
| `-filePatching`, `-doLogs`, `-BEpath=` | server/diag oriented, not exposed |

## 6. Steam Workshop subscribe and download (Steamworks, S-25)

The official launcher does exactly this: `Launcher.log` shows `Launcher.Steam.LauncherSteamFacade: Steam init result: Online`, `SteamUgcDownloadManager: Steam UGC Download manager initialization 0` and `SteamUgcDownloadManager: Item was installed: 1559212036`, plus `ModSignatureCalculator` PBO/bisign caching (S-41).

**Implemented and verified in M5 (D-051):** `steam/sdk.rs` runs the flow below on the Steam thread; `launch/mods.rs` creates the junction; `launch/args.rs` + `launch/process.rs` start the game. A live run downloaded Ear-Plugs (107 kB) in 1.3 s, created `!Workshop\@Ear-Plugs`, and DayZ started with `-connect`/`-port`/`-name` through `DayZ_BE.exe`.

Flow for "Join" on a server with missing mods:

1. `Client::init_app(221100)` on a dedicated thread, retried every 10 s while Steam is not running (D-125), released after the idle timeout and opened again on use (D-077, D-275). `run_callbacks()` runs every 10 ms while a download, refresh or unsubscribe is in progress and every 100 ms otherwise (D-164); nothing is pumped while the session is released.
2. For each required Workshop ID: `ugc.item_state(id)` → bitflags `Subscribed`, `Installed`, `NeedsUpdate`, `Downloading`, `DownloadPending`.
3. If not subscribed: `ugc.subscribe_item(id)` (async result callback). A join subscribes; the Mods page's Update only updates what is still subscribed (D-276). Then `ugc.download_item(id, high_priority = true)` to start immediately instead of waiting for Steam's scheduler, issued again every 5 s until Steam starts it.
4. Progress: `ugc.item_download_info(id)` → `(bytes_downloaded, bytes_total)`, sent to the UI every 250 ms. Completion is polled: an item is done when `item_state` says Installed and `item_install_info(id)` names a folder that is on disk. The `DownloadItemResult` callback only reports errors, and an item fails on Steam's second error report for it (D-242).
5. Limits: an item Steam calls installed whose folder is missing fails a minute after its kick, with the remedy (Q30, D-265); a download fails when Steam has moved nothing for 15 minutes, and in any case after 8 hours (D-240).
6. The junctions are made at launch, not by the sync: `launch_game` reads `appworkshop_221100.acf` (or the `content\221100\<id>` folders when it is missing), takes each mod's `meta.cpp` name and creates `!Workshop\@<name>` if missing (`launch/mods.rs`), then builds the `-mod=` line.

Without Steamworks: `appworkshop_221100.acf` and folder existence tell what is installed, and the Workshop page (`https://steamcommunity.com/sharedfiles/filedetails/?id=<id>`) is the manual way to subscribe. Titles and sizes come from Steamworks (`query_items`, 50 ids a request, D-052); the Web API's `GetPublishedFileDetails` (no key, S-39) was the planned fallback and is not implemented.

## 7. Official launcher local data (for an import feature)

`%LOCALAPPDATA%\DayZ Launcher\`:

- `FavouriteServers.xml` — `<Server IpAddress="1669946347" Port="5002" QueryEndPoint="99.137.91.235:5003" ConnectionEndPoint="99.137.91.235:5002" Name="…" Map="chernarusplus" MaxPlayers="10" ServerVersion="129163451" RequiredVersion="129" Tags="battleye,no3rd,external,privHive,shardABC123,lqs0,entm1.000000,11:54" …/>` (IpAddress is the IPv4 as a big-endian uint32). **Imported by `steam/official.rs` (M6, D-053)**: `QueryEndPoint` gives the row id, `ConnectionEndPoint` the game port; the server is probed live and otherwise stored from these fields.
- `Presets\*.dayz.defaultpreset2` etc.: XML `<addons-presets><published-ids><id>steam:1559212036</id>…`.
- `ServerBrowser.Settings.json`, `Servers.Banned.json`, `Parameters.json`, `Local.json`, `bisignCache.nson`, `pboCache.nson`, `Logs\`.
- `Launcher\Config.json` in the game folder lists Bohemia endpoints, including news: `https://dayz.com/api/v1.0/launcher?rowsPerPage=10&page={0}&absolute=1` and posts `https://dayz.com/api/v1.0/post/{0}`.

Game data: `%LOCALAPPDATA%\DayZ\` (RPT and crash logs), `%USERPROFILE%\Documents\DayZ\` (`<user>.core.xml`, `chars.DayZProfile`, `DayZ.cfg`, `profile.vars.DayZProfile`).

Default `-name`: the Steam persona from the running Steamworks session (`friends().name()`), used when Settings and the chosen launch profile leave the name empty; with no name at all, `-name` is left out (D-052, Q13 closed; `loginusers.vdf` is not read).

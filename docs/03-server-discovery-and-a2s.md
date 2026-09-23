# 03 · Server discovery and the A2S protocol (DayZ)

Everything in this document was either read from the Valve wiki / two independent open-source parsers, or **verified live on 2026-09-21** with [tools/a2s_probe.js](../tools/a2s_probe.js) against `51.81.8.81:27017` and cross-checked against the DZSA API's answer for the same server. Live-verified facts are marked **[LIVE]**.

## 1. Where the server list comes from

| Source | Key needed | Completeness | Gives mods? | Latency | Role in our launcher |
|---|---|---|---|---|---|
| Steamworks `ISteamMatchmakingServers::RequestInternetServerList(221100)` (S-25) | No (Steam client must be running) | All servers that heartbeat to Steam | No (name, map, players, ping, tags) | Streams in over a few seconds | **Primary list** |
| ~~Master Server Query Protocol, UDP `hl2master.steampowered.com:27011`~~ (S-17) | – | **Retired.** `hl2master`/`hl1master.steampowered.com` return NXDOMAIN on the system resolver, 1.1.1.1 and 8.8.8.8 (2026-09-21); community reports date the shutdown to October 2025 (S-50). Not usable (D-031) | – | – | none |
| Steam Web API `IGameServersService/GetServerList/v1?filter=\appid\221100&limit=20000` (S-16) | **Yes** | Same | No (`gametype` = tags) | One HTTP call | Not used (would leak a key) |
| Direct A2S_INFO / A2S_RULES to each server | No | n/a | **Yes** (RULES) | ~RTT per server, fan out | **Details + ping + mods** |
| DZSA `GET https://dayzsalauncher.com/api/v1/query/{ip}/{queryPort}` (S-18) **[LIVE]** | No (Cloudflare blocks non-browser user agents; got 403 from a plain fetcher) | Only registered servers | Yes | One HTTP call | Optional fallback when RULES fails |
| DZSA `GET /api/v1/launcher/servers/dayz` (S-19) | No | Registered only | Yes | > 10 MB JSON | Not used at startup (too heavy) |
| BattleMetrics `GET https://api.battlemetrics.com/servers?filter[game]=dayz&filter[search]=…` (S-38) | Optional | Broad | Partial | 60 req/min unauthenticated | Optional link-out only |

Decision: primary list from Steamworks, then A2S fan-out for ping/details, RULES on demand (selected row, favourites, join). Fallbacks when Steamworks is unavailable: a user-supplied Steam Web API key (optional setting, never shipped in the binary), then the DZSA list. See ADR-002 in [04](04-tech-stack-decision.md).

Steamworks filter keys (Master Server Query Protocol filters, S-15/S-17), passed as `&[HashMap<&str,&str>]` in the crate: `appid=221100`, `gamedir=dayz`, `empty=1`, `full=1`, `dedicated=1`, `secure=1`, `password=0`, `name_match=*text*`, `map=chernarusplus`, `gametype=battleye,no3rd`.

## 2. A2S transport (Valve "Server queries", S-15)

- UDP to the **query port** (DayZ default 27016; official examples use 27017 when the game port is 2402, but it is arbitrary; the Steam list gives the query address and `gameport` separately).
- All integers little-endian. Strings are UTF-8, NUL-terminated.
- Max packet 1400 bytes. Single packet header: `FF FF FF FF` (int32 −1). Split packet header: `FE FF FF FF` (−2) then:

| Field | Type | Note |
|---|---|---|
| ID | int32 | Same for every fragment of one answer. MSB set = bzip2-compressed (legacy Source engines; not expected from DayZ but check the bit). |
| Total | uint8 | Number of fragments |
| Number | uint8 | 0-based fragment index |
| Size | uint16 | Max fragment size (Orange Box+, present for DayZ) |
| Payload | bytes | Concatenate by Number; the result starts with `FF FF FF FF` again |

- **Challenge**: a server may answer any request with `FF FF FF FF 41 <int32 challenge>`; resend the same request with the 4 challenge bytes appended (INFO) or in place of the `FF FF FF FF` placeholder (PLAYER/RULES).

## 3. A2S_INFO

Request: `FF FF FF FF 54 "Source Engine Query\0" [challenge]`
Response: `FF FF FF FF 49` then:

| Field | Type | DayZ value **[LIVE]** |
|---|---|---|
| Protocol | uint8 | 17 |
| Name | string | `WILDLANDZ Green County Greatness` |
| Map | string | `GreenCounty` |
| Folder | string | `dayz` |
| Game | string | server **description** from `serverDZ.cfg` (`PvP \| Survival \| No Bases`), same text as the RULES description |
| AppID | uint16 | **0** (221100 overflows 16 bits; use the EDF GameID) |
| Players / MaxPlayers / Bots | uint8 ×3 | 0 / 50 / 0 |
| ServerType | char | `d` |
| Environment | char | `w` (Windows) |
| Visibility | uint8 | 0 = public, 1 = password |
| VAC | uint8 | 1 |
| Version | string | `1.29.163709` (matches `DayZ_x64.exe` ProductVersion 1.29.0.163709) |
| EDF | uint8 | `0xB1` = 0x80 port + 0x20 keywords + 0x10 SteamID + 0x01 GameID |
| Port (0x80) | uint16 | 2402 (**game port**, what `-port=` needs) |
| SteamID (0x10) | uint64 | 90293138942181394 |
| Spectator (0x40) | uint16 + string | absent |
| Keywords (0x20) | string | `battleye,no3rd,external,privHive,shardABC123,lqs0,etm4.000000,entm6.000000,mod,15:12` |
| GameID (0x01) | uint64 | 221100 (lower 24 bits = AppID) |

Ping = round-trip time of the INFO exchange (measure from the **final** request, i.e. after the challenge). Live RTT was 108 ms from this machine.

### Keyword tags (S-37, **[LIVE]** examples)

| Tag | Meaning |
|---|---|
| `battleye` | BattlEye enabled |
| `no3rd` | first-person only |
| `external` | public/external hive (vs `privHive` private hive) |
| `privHive` | private hive (characters stored on that server) |
| `shard<xxx>` | hive shard id (`shard000`/`shard001` official, `shardABC123` private) |
| `lqs<N>` | players in **login queue** (queue size) |
| `etm<f>` | environment time multiplier (day time acceleration, e.g. `etm4.000000`) |
| `entm<f>` | night time multiplier |
| `mod` | server runs mods |
| `isDLC` | requires DLC terrain (e.g. Livonia/Sakhal) |
| `allowedFilePatching` | server accepts clients started with `-filePatching` (seen live on a modded server) |
| `HH:MM` | current in-game time (last tag) |

## 4. A2S_RULES and the DayZ Server Browser Protocol v2

Request: `FF FF FF FF 56 <challenge or FF FF FF FF>`
Response: `FF FF FF FF 45 <uint16 count>` then `count` × (`key\0 value\0`).

DayZ smuggles a binary structure through the key/value pairs (S-09, S-11, S-12, S-14; **[LIVE]**):

1. Pairs whose **key is exactly 2 bytes** and `key[0] <= key[1]` are fragments: `key = [index (1-based), total]`. Live: keys `1/4 … 4/4`, 4 fragments for 12 mods, 447 escaped bytes in one UDP packet. Sort by index, concatenate the values.
2. **Unescape** the concatenation (values cannot contain `0x00`, so these bytes are escaped):

| Escaped | Byte |
|---|---|
| `01 01` | `0x01` |
| `01 02` | `0x00` |
| `01 03` | `0xFF` |

   Confirmed by `internal/bread/sequences.go` (WoozyMasta), `dayzquery.py` (Yepoleb) and the live probe (447 → 443 bytes, zero trailing bytes after parse). A blog summary that says `01 02 → 0x02` is wrong.

3. Parse the payload (all little-endian):

| Field | Type | Live value |
|---|---|---|
| protocolVersion | uint8 | 2 (Arma 3 uses 3 and has two extra difficulty bytes; DayZ does not) |
| overflowFlags | uint8 | 0 |
| dlcFlags | uint16 | 0. Bit 0x1 Livonia, 0x2 Frost Line (Sakhal), 0x4 Badlands, 0x8 Survivor GameZ (S-12 `dlc.go`) |
| dlcHash[] | uint32 × popcount(dlcFlags) | none |
| modCount | uint8 | 12 |
| mod[i].hash | uint32 | e.g. `0x68afc6c1` for CF (purpose unknown) |
| mod[i].idLen | uint8 | 4 on every live entry; mask with `0x0F`, then read that many bytes LE as the Workshop ID (parsers accept 1/2/4/8) |
| mod[i].id | uintN | 1559212036 |
| mod[i].nameLen + name | uint8 + UTF-8 | `Community Framework` (= `mod.cpp` `name`, **not** the Workshop title `CF`) |
| signatureCount | uint8 | 12 |
| signature[i] | uint8 len + string | `CooltrainV3`, `dab`, `dayz`, … (accepted `.bikey` names) |
| descriptionLen + description | uint8 + string | `PvP \| Survival \| No Bases` |

4. Remaining plain pairs **[LIVE]**: `allowedBuild=0`, `clientPort=49591`, `dedicated=1`, `island=GreenCounty`, `language=65545`, `platform=win`, `requiredBuild=0`, `requiredVersion=129`, `timeLeft=15`.

**Match mods by Workshop ID only.** Names differ between A2S (`mod.cpp`), DZSA (Workshop title) and the local junction name (`meta.cpp` name). Live example: `Community Framework` / `CF` / `@CF`.

**DayZ does not split large RULES answers** (D-033). Live captures of a 121-mod server (4 587-byte datagram, 43 rules, 34 fragments) and a 105-mod server (5 309 bytes, 49 rules, 40 fragments, 197 signatures) each arrived as **one** UDP datagram, far above the 1 400-byte Source convention. Receive buffers must therefore be large (the client uses 64 KiB for RULES and 16 KiB for INFO and PLAYER); the split-packet path stays implemented per spec but has not been observed from DayZ. Fragment index is 1-based and fits in a byte (max seen 40).

The per-mod `hash` is a **content hash**: Community Framework 1559212036 reported `0x68AFC6C1` on all three captured servers (D-035), so it can later flag "server runs a different build of this mod".

## 5. A2S_PLAYER

Request `FF FF FF FF 55 <challenge>`; response `FF FF FF FF 44 <uint8 count>` then per player `index u8, name string, score int32, duration float32`. **[LIVE, D-034]** DayZ returns **one entry per connected player with an empty name, score 0 and the real connection duration in seconds** (2/2 and 4/4 entries on populated servers; 0 on the empty one). Names are never exposed. The entry count is an independent head-count: comparing it with INFO `players` is the basis for launchZ-style "fake population" detection, and the durations give session-length statistics.

## 6. DZSA API (S-18, **[LIVE]** 2026-09-21)

`GET https://dayzsalauncher.com/api/v1/query/51.81.8.81/27017` →

```json
{"status":0,"result":{"gamePort":2402,"sponsor":false,"profile":false,
 "endpoint":{"ip":"51.81.8.81","port":27017},"game":"dayz",
 "name":"WILDLANDZ Green County Greatness","nameOverride":false,"map":"GreenCounty",
 "folder":"dayz","players":0,"maxPlayers":50,"environment":"w","password":false,
 "version":"1.29.163709","mission":"dayz","vac":true,"battlEye":true,
 "firstPersonOnly":true,"shard":"private","timeAcceleration":4,"time":"15:00",
 "mods":[{"name":"Summer In America","steamWorkshopId":3747295862}, … 12 entries …]}}
```

The 12 `steamWorkshopId`s matched the live A2S_RULES decode exactly. Per-server list fields used by dayz-ctl (S-05): `name, endpoint.ip, endpoint.port, gamePort, password, firstPersonOnly, shard, battlEye, vac, players, maxPlayers, map, time, mods[].name, mods[].steamWorkshopId, version, environment, timeAcceleration`.

## 7. Implementation notes for the Rust core (as built in M2)

- `a2s::Client`: one short-lived socket per query, challenge loop (max 3), split reassembly, 64 KiB receive buffer, ICMP-unreachable detection, **send pacing** (default 400 datagrams/s), concurrency 128, timeout 1 s, 1 retry. Measured (D-037): live servers answer with p99 ≈ 250 ms; loss is driven by burst size through consumer NAT, so pacing and modest concurrency beat raw fan-out width.
- **Player counts**: INFO `players` is spoofed by more than half of community servers (D-038). Query PLAYER for every server that is on screen, in favourites, or being joined, and display that head-count; show the INFO number only as "reported" with a warning when it disagrees.
- Keep a per-server state machine: `Listed → InfoOk(ping) → PlayersOk(head-count) → RulesOk(mods)`; RULES only on demand (selection, favourites, filters that need mods, join).
- Store the raw INFO/RULES bytes for a server when a parse fails and surface it on the Logs page (D-168); that is how protocol drift gets caught.
- Cache: list snapshot + last INFO per server in SQLite with timestamps; on startup render the cache immediately, then refresh.
- Version check: compare INFO `version` with the local `DayZ_x64.exe` ProductVersion; `requiredVersion=129` maps to 1.29.

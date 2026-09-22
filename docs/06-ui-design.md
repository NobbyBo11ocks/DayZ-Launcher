# 06 · UI design

Goal: the fastest way from "open launcher" to "in game", in a dark, quiet, information-dense interface that still looks current. Layout borrows the master–detail pattern from launchZ, the one-click join and themes from Beans, and avoids DZSA's dated table chrome ([01](01-competitor-analysis.md)).

## 1. Window and layout

- Default 1280×800, minimum 960×600; size, position and maximised state are remembered between runs (`tauri-plugin-window-state`, D-098). Custom title bar (`decorations: false`, drag region) so the theme covers the whole window: 30 px high, no app name, an update notice when one is pending, and our own minimise/maximise/close buttons (D-091).
- Three regions:

```text
┌──────────────────────────────────────────────────────────────────┐
│ (drag region)                              update notice │ ▭ ✕   │
├──────────┬───────────────────────────────┬────────────────────────┤
│ News ③   │ filter bar                    │                        │
│ Servers  │ virtualised table             │ details pane           │
│ LAN      │ name · map · players/queue    │ header + JOIN          │
│ Favs     │ time · ping · flags           │ info grid              │
│ Friends  │ (sortable columns)            │ mods (state per mod)   │
│ Recent   │                               │ population sparkline   │
│ Mods     │                               │                        │
│ Settings │                               │                        │
│ Diagn.   │                               │                        │
└──────────┴───────────────────────────────┴────────────────────────┘
```

- News (D-099) sits first with an unread badge (③ above); it is a padded view of cards that scrolls inside, not a table.

- Responsive: rail collapses to icons < 1100 px; details pane hidden < 1000 px. The third breakpoint specified here — dropping "time" and "version" below 900 px — was never built and is unreachable anyway: the window minimum is 960 px. The open recommendation from D-153 stands: make the pane an overlay at small widths rather than hiding it, so Join is reachable at the minimum size.
- Only the server list views scroll their table. Recent, Mods, Settings, Diagnostics and Friends fit the viewport (`.content.padded` is `overflow: hidden`); when the window is smaller than their content, a table or a column scrolls on its own, never the page (D-094).

## 2. Server table

| Column | Content |
|---|---|
| Flag + name | country flag (from offline GeoIP, Q5), server name, lock icon if password, shield if BattlEye, "1PP" pill for `no3rd`, DLC pill |
| Map | short map name with colour dot per terrain |
| Mods | number of mods from the scanned list, blank when the server has not been scanned (D-146) |
| Players | **Verified** head-count from A2S_PLAYER for every row on screen (`players/max`, `+queue` from `lqs`). The fill bars specified here were removed in v0.1.3 (D-069): at 36 px they read as noise behind the numbers. An unverified count is dimmed and marked "?" (D-160); an untrusted one gets a warning glyph. More than half of community servers spoof INFO (D-038) |
| Ping | numeric with 3-band colour (green < 60, amber < 120, red) |
| Time | in-game clock with sun/moon glyph; tooltip shows `etm`/`entm` acceleration |
| Version | the server's build, amber when it differs from the local client |

The favourite star and the alert bell live in the name cell, not a separate Actions column; there is no per-row join button — Enter or a double-click opens the join dialog.

Row height 36 px, hover highlight, keyboard navigation (↑/↓, Enter = join, F = favourite, / = focus search).

## 3. Details pane

Header: name, map, version (red badge if it differs from the local client), JOIN button (primary), copy IP, open in BattleMetrics/DZSA (optional). Info grid: players, queue, perspective, time + acceleration, hive/shard, addresses. Mods list: each row shows name (from A2S), Workshop ID, state chip (installed / update available / missing / downloading with progress) and a link icon. "Sync mods" button when anything is missing. Population sparkline: last 72 h from local samples.

## 4. Filters

Chips above the table: Perspective (1PP/3PP), Map, Country (D-073), Not full, Not empty, Has queue, No password, BattlEye only, Modded/Vanilla, Day only, Max ping, Mod (one, chosen from the union of seen mods, D-080), Version = mine, Friends (D-128) and Hide inflated. All filters persist. Two specified here were never built: a time-acceleration filter, and multi-select on mods — one mod answers "which servers run this", which is the question people actually ask.

## 5. Visual system

- Tokens (`:root`): `--bg`, `--bg-elev`, `--bg-row`, `--fg`, `--fg-muted`, `--accent`, `--accent-fg` (text on an accent surface: `#111` on dark, `#fff` on the deeper light-theme accents), `--ok`, `--warn`, `--danger`, `--radius: 8px`, `--row-h: 36px`.
- Themes: **Slate** (default dark, neutral greys with a single accent) and **Light**. Twelve accents in colour-wheel order (amber, orange, red, rose, pink, violet, indigo, blue, sky, teal, green, lime), each with a vivid dark-theme tone and a deeper light-theme tone; picked as a row of dots in Settings (D-131). **Lime** is the default (D-132). Theme and accent switch instantly (attributes on `<html>`).
- Typography: Segoe UI Variable / system-ui; 13 px table, 15 px headers, tabular numerals for ping/players.
- Motion: 120 ms fades, no slides on lists; skeleton rows while the first batch streams; reduced-motion respected.
- Iconography: single inline SVG sprite, stroke icons 16 px.

## 6. States and copy

| State | UI |
|---|---|
| Steam not running | full-width banner with "Open Steam" button; list still shows cached data |
| DayZ not found | onboarding card: auto-detect result, "Locate DayZ_x64.exe" |
| No mods yet / join with missing mods | modal: list of mods with sizes (from GetPublishedFileDetails), total, "Subscribe and join" |
| Download in progress | inline progress per mod, overall bar in the JOIN button |
| Version mismatch | red badge + tooltip "Server 1.29.163709, you have 1.28.x. Update DayZ in Steam." |
| Query failed | grey row with retry icon; details pane shows raw diagnostics |

## 7. Accessibility

Contrast ≥ 4.5:1 for text on all themes, visible focus rings, full keyboard operation of table and filters, `aria-sort` on headers, live region for "x servers, y online" updates, no information conveyed by colour alone (ping band also has a glyph).

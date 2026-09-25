# 06 · UI design

Goal: the fastest way from "open launcher" to "in game", in a dark, quiet, information-dense interface that still looks current. Layout borrows the master–detail pattern from launchZ, the one-click join and themes from Beans, and avoids DZSA's dated table chrome ([01](01-competitor-analysis.md)).

## 1. Window and layout

- Default 1280×800, minimum 960×600; size, position and maximised state are remembered between runs (`tauri-plugin-window-state`, D-098). Custom title bar (`decorations: false`, drag region) so the theme covers the whole window: 30 px high, no app name, an update notice when one is pending, and our own minimise/maximise/close buttons (D-091). **Moved:** the update notice now sits at the foot of the section rail, not in the title bar (D-216).
- Three regions:

```text
┌─────────────────────────────────────────────────────────────────────────┐
│ counts · greeting (drag region)                                  ─ ▭ ✕  │
├──────────────┬────────────────────────────────┬─────────────────────────┤
│ News ③       │ search ⌖   (notice)    Refresh │                         │
│ Servers      │ virtualised table              │ details pane            │
│ LAN          │ name · map · players/queue     │ header + JOIN           │
│ Favs         │ time · ping · flags            │ info grid               │
│ Friends      │ (sortable columns)             │ mods (state per mod)    │
│ Recent       │                                │ population sparkline    │
│ Mods         │                                │                         │
│ Settings     │                                │                         │
│ Logs         │                                │                         │
│ ──────────── │                                │                         │
│ FILTERS Reset│                                │                         │
│ (Servers page│                                │                         │
│  only, §4)   │                                │                         │
│ ↑ Update     │                                │                         │
└──────────────┴────────────────────────────────┴─────────────────────────┘
```

- News (D-099) sits first with an unread badge (③ above); it is a padded view of cards that scrolls inside, not a table.

- The rail is 240 px wide (180 px until D-249 widened it for the filters). It holds the sections, the Servers filters under them while that page is open (§4), and the update item at its foot (D-216). Section icons are in the accent and their labels in the default text colour (D-253; D-250 had the labels in the accent as well); the open section has the raised surface, a short accent pill centred on its left edge (D-255; it was a 3 px bar that bent round the rounded corners) and a heavier weight, and the update item an accent tint.
- The Servers header is one row: the search, Direct connect beside it, and Refresh at the right. The status line that sat under the search is gone (D-250); a notice that needs the user — Steam not running, with the DZSA fallback, or an error — shows in the row between them only while it is true.
- Responsive: rail collapses to icons < 1100 px, except on the Servers page, whose filters keep it at full width at every size (720 px of table at the 960 px minimum, against a 700 px floor); details pane hidden < 1000 px. The third breakpoint specified here — dropping "time" and "version" below 900 px — was never built and is unreachable anyway: the window minimum is 960 px. D-189 did it: below 1240 px the pane is an overlay at small widths rather than hiding it, so Join is reachable at the minimum size. With the 240 px rail that breakpoint is 1300 px (D-249).
- Only the server list views scroll their table. Recent, Mods, Settings, Logs (the Diagnostics page was replaced by it in D-168) and Friends fit the viewport (`.content.padded` is `overflow: hidden`); when the window is smaller than their content, a table or a column scrolls on its own, never the page (D-094).

## 2. Server table

| Column | Content |
|---|---|
| Flag + name | country flag (from offline GeoIP, Q5), server name, lock icon if password, shield if BattlEye, "1PP" pill for `no3rd`, DLC pill |
| Map | short map name with text label from `mapLabel` (D-195); no colour dot was ever built |
| Mods | number of mods from the scanned list, blank when the server has not been scanned (D-146) |
| Players | **Verified** head-count from A2S_PLAYER for every row on screen (`players/max`, `+queue` from `lqs`). The fill bars specified here were removed in v0.1.3 (D-069): at 36 px they read as noise behind the numbers. An unverified count is dimmed and marked "?" (D-160); an untrusted one gets a warning glyph. More than half of community servers spoof INFO (D-038) |
| Ping | numeric with 3-band colour (green < 60, unstyled 60-119, amber >= 120) |
| Time | in-game clock with sun/moon glyph; tooltip shows `etm`/`entm` acceleration |
| Version | the server's version as major.minor ("1.29", D-251), amber when its build differs from the local client; the full build (1.29.163709) is in the tooltip and the details pane |

The favourite star lives in the name cell, not a separate Actions column, drawn at 18 px since D-254: outlined, and filled in the accent for a favourite; there is no per-row join button — Enter or a double-click opens the join dialog. The alert bell that sat beside it was removed with the feature (D-182).

Row height 36 px, hover highlight, keyboard navigation (↑/↓, Enter = join, F = favourite, / = focus search).

## 3. Details pane

Header: name, map, version (red badge if it differs from the local client), JOIN button (primary), copy IP, open in BattleMetrics/DZSA (optional). Info grid: players, queue, perspective, time + acceleration, hive/shard, addresses. Mods list: each row shows name (from A2S), Workshop ID, state chip (installed / update available / missing / downloading with progress) and a link icon. "Sync mods" button when anything is missing. Population sparkline: last 72 h from local samples.

## 4. Filters

A column in the left rail, under the sections, while the Servers page is open (D-249, from a design sketch; until then they were two rows of chips above the table, D-108). Top to bottom: Perspective (Any / 1PP / 3PP); Playstyle (PVE / PVP / RP, D-208); Status, as checkboxes — Not empty, Not full, Has queue, No password, Day only ("Daytime"), Version = mine, then Friends playing (D-128) and Hide inflated with their counts; Official / Community hive (D-195); Map and Country (D-073); Max ping (with presets, D-208); Mods — Modded / Vanilla and one specific mod, chosen from the union of seen mods (D-080). The heading carries a Reset button with the number of filters in effect, and only the groups scroll when the window is too short for them. The search stays in the page header with Direct connect and Refresh; it is shared with Favourites and LAN, which apply nothing else. All filters persist. Two specified here were never built: a time-acceleration filter, and multi-select on mods — one mod answers "which servers run this", which is the question people actually ask.

## 5. Visual system

- Tokens (`:root`): `--bg`, `--bg-elev`, `--bg-row`, `--fg`, `--fg-muted`, `--accent`, `--accent-fg` (text on an accent surface: `#111` on dark, `#fff` on the deeper light-theme accents), `--ok`, `--warn`, `--danger`, `--radius: 8px`, `--row-h: 36px`.
- Themes: **Slate** (default dark, neutral greys with a single accent) and **Light**. Twelve accents in colour-wheel order (amber, orange, red, rose, pink, violet, indigo, blue, sky, teal, green, lime), each with a vivid dark-theme tone and a deeper light-theme tone; picked as a row of dots in Settings (D-131). **Lime** is the default (D-132). Theme and accent switch instantly (attributes on `<html>`).
- Typography: Segoe UI Variable / system-ui; 13 px table, 15 px headers, tabular numerals for ping/players.
- Motion: 120 ms fades, no slides on lists; skeleton rows while the first batch streams; reduced-motion respected.
- Iconography: per-component inline SVG; the shared sprite was specified here and never built (docs/05 §7), stroke icons 16 px. The section rail draws its own at 18 px (`RailIcon.svelte`, 24-unit grid, 1.8 stroke, D-250), in the accent, with the labels beside them in the default text colour (D-253); the Unicode glyphs it used before changed size and weight with the font that had them.

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

Contrast ≥ 4.5:1 for text on all themes, visible focus rings, full keyboard operation of table and filters, `aria-sort` on headers, live region for "x servers, y online" updates, no information conveyed by colour alone (ping band carries its quality in a tooltip (D-198), not a glyph).

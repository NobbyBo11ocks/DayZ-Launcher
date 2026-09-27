# 06 · UI design

Goal: the fastest way from "open launcher" to "in game", in a dark, quiet, information-dense interface that still looks current. Layout borrows the master–detail pattern from launchZ, the one-click join and themes from Beans, and avoids DZSA's dated table chrome ([01](01-competitor-analysis.md)).

## 1. Window and layout

- Default 1280×800, minimum 960×600; size, position and maximised state are remembered between runs (`tauri-plugin-window-state`, D-098). Custom title bar (`decorations: false`, drag region) so the theme covers the whole window: 30 px high, no app name but the logo's gas mask at the left where a framed window shows its icon (D-258), an update notice when one is pending, and our own minimise/maximise/close buttons (D-091), drawn in the accent since D-300. **Moved:** the update notice now sits at the foot of the section rail, not in the title bar (D-216).
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
- Responsive: rail collapses to icons < 1100 px, except on the Servers page, whose filters keep it at full width at every size (720 px of table at the 960 px minimum, against a 700 px floor); on that page a window under 770 px tall shows the sections as one row of icons across the top of the rail instead, so the filters under them never scroll (D-259). Below 1300 px the details pane is an overlay rather than a third column, so Join is reachable at the minimum size (D-189 at 1240 px; 1300 px with the 240 px rail, D-249); the "hidden below 1000 px" specified here was never built. The third breakpoint specified here — dropping "time" and "version" below 900 px — was never built either, and is unreachable: the window minimum is 960 px.
- Only the list views scroll their table: Servers, LAN, Favourites and Friends. Recent, Mods, Settings and Logs (the Diagnostics page was replaced by it in D-168) fit the viewport (`.content.padded` is `overflow: hidden`); when the window is smaller than their content, a table or a column scrolls on its own, never the page (D-094).

## 2. Server table

| Column | Content |
|---|---|
| Flag + name | country flag (from offline GeoIP, Q5), then 🔒 for a password and pills for `no3rd` ("1PP"), mods ("MOD") and DLC, and a friends pill with the number of Steam friends playing there (D-128); then the server name. The BattlEye shield specified here was never built |
| Map | short map name with text label from `mapLabel` (D-195); no colour dot was ever built |
| Mods | number of mods from the scanned list; a dimmed "–" with the tooltip "Mod list not scanned yet" when the server has not been scanned, which is not the same as no mods (D-146) |
| Players | **Verified** head-count from A2S_PLAYER for every row on screen (`players/max`, `+queue` from `lqs`). The fill bars specified here were removed in v0.1.3 (D-069): at 36 px they read as noise behind the numbers. An unverified count is dimmed and marked "?" (D-288); an untrusted one gets a warning glyph. More than half of community servers spoof INFO (D-038) |
| Ping | numeric with 3-band colour (green < 60, unstyled 60-119, amber >= 120) |
| Time | in-game clock with sun/moon glyph; the `etm`/`entm` multipliers are in the details pane (the tooltip specified here was never built) |
| Version | the server's version as major.minor ("1.29", D-251), amber when its build differs from the local client; the full build (1.29.163709) is in the tooltip and the details pane |

The favourite star lives in the name cell, not a separate Actions column, drawn at 18 px since D-254: outlined, and filled in the accent for a favourite; there is no per-row join button — Enter or a double-click opens the join dialog. The alert bell that sat beside it was removed with the feature (D-182).

Row height 36 px, hover highlight, keyboard navigation (↑/↓, Enter = join, F = favourite, ←/→ = sort column and Space = reverse it (D-198), / = the search, from the list (D-291)). A screen reader hears each row as one sentence, with the words the cells only show as colour, glyph or tooltip (D-291).

## 3. Details pane

Header: name; chips for the country, the map, the version (amber when it differs from the local `DayZ_x64.exe`) and a password; the address with a copy link; Join (primary), the favourite star and close. Under it the trust box: what the list's verdict means for this server, in the player's words, with the rule's own on hover (D-248, D-268). Info grid: players, ping, view (1PP/3PP), time with the day and night multipliers, hive, anti-cheat. Then the description, the other servers at the same address (D-212), the population sparkline for the last 72 h from local samples, the mods — each a tick when installed or a circle when missing, the name from A2S and the Workshop id as a link to its Workshop page — and a summary of the connected sessions rather than a list (D-081). Specified and never built: a BattleMetrics/DZSA link, per-mod state chips with download progress, and a "Sync mods" button — downloads happen in the join dialog.

## 4. Filters

A column in the left rail, under the sections, while the Servers page is open (D-249, from a design sketch; until then they were two rows of chips above the table, D-108). Every filter is in view at once and nothing scrolls (D-259): each choice is one 26 px box with its label inside at the left and the control filling the rest. Top to bottom: Perspective (Any / 1PP / 3PP); Playstyle (PVE / PVP / RP, D-211); Status, as checkboxes with no heading — Not empty, Not full, Has queue, No password, Daytime, My version, then Friends playing (D-128) with its count. The rest sits at the foot of the rail (D-270): Hide inflated with its count; Official / Community hive (D-195); Map, then Country (D-073); Max ping (with presets, D-211); Mods — Modded / Vanilla, and under it the search box and the pick of one specific mod side by side, chosen from the union of seen mods (D-080). The heading carries a Reset button with the number of filters in effect. The column takes 356 px, 408 with a mod chosen and its one-line scan hint, against the 407 px the rail leaves under 32 px section rows at 1280×800; a shorter window turns the sections into a row of icons (§1), and the body's own scrolling is only the fallback for a pending update and a chosen mod together. The search stays in the page header with Direct connect and Refresh; it is shared with Favourites and LAN, which apply nothing else. All filters persist. Two specified here were never built: a time-acceleration filter, and multi-select on mods — one mod answers "which servers run this", which is the question people actually ask.

## 5. Visual system

- Tokens (`:root`): `--bg`, `--bg-elev`, `--bg-row`, `--fg`, `--fg-muted`, `--accent`, `--accent-fg` (text on an accent surface: `#111` on dark, `#fff` on the deeper light-theme accents), `--ok`, `--warn`, `--danger`, `--radius: 8px`, `--row-h: 36px`.
- Themes: **Slate** (default dark, neutral greys with a single accent) and **Light**. Twelve accents in colour-wheel order (amber, orange, red, rose, pink, violet, indigo, blue, sky, teal, green, lime), each with a vivid dark-theme tone and a deeper light-theme tone; picked as a row of dots in Settings (D-131). **Lime** is the default (D-132). Theme and accent switch instantly (attributes on `<html>`).
- Typography: Segoe UI Variable / system-ui; 12.5 px table, 15 px headers, tabular numerals for ping/players.
- Motion: 120 ms fades, no slides on lists; reduced-motion respected. The skeleton rows specified here were never built: the cached list paints at once, and an empty grid says why (D-165).
- Iconography: per-component inline SVG; the shared sprite was specified here and never built (docs/05 §7), stroke icons 16 px. The section rail draws its own at 18 px (`RailIcon.svelte`, 24-unit grid, 1.8 stroke, D-250), in the accent, with the labels beside them in the default text colour (D-253); the Unicode glyphs it used before changed size and weight with the font that had them.

## 6. States and copy

| State | UI |
|---|---|
| Steam not running | a notice in the Servers header with "Load list from DZSA" (D-250); the cached list stays |
| DayZ not found | a warning in the join dialog and on the Mods page, naming the folder when its drive is not mounted (D-165); there is no locate step |
| Join with missing mods | the join dialog: the mods with sizes (from Steamworks), the total, "Download N mods and join" and "Download only" |
| Download in progress | progress per mod and a status line with the rate and the time left, in the join dialog |
| Version mismatch | the version cell in amber with the tooltip "Server runs X; your DayZ is Y", the details pane's version chip in amber, and a warning in the join dialog |
| Query failed | a ⚠ with "Server is not answering…" in the row; the details pane says "Not answering" after its own retry (D-272) |
| After an update | the "What's new" window (D-301): every release since the one last seen, one short line per change under Added, Fixed, Changed or Removed in the News pill, and "Got it"; once per version, never on a first run (the welcome shows then) |

The banner with "Open Steam", the "Locate DayZ_x64.exe" card, the bar inside the Join button and the grey retry row specified here were never built.

## 7. Accessibility

Contrast ≥ 4.5:1 for text on all themes, visible focus rings, full keyboard operation of table and filters, `aria-sort` on headers, a live region that announces outcomes only ("N verified, M fake, K offline.", "N servers listed.", D-224), no information conveyed by colour alone (ping band carries its quality in a tooltip (D-198), not a glyph). Since D-291: keyboard sorts, favourites and filter results are announced; focus is handed on whenever the element holding it goes (the pane, confirmations, Reset, dialogs); the filters' single-choice rows are radio groups; the join dialog says each step from one region; Windows contrast themes keep a selection visible.

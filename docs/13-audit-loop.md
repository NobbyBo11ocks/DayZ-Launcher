# 13 · Audit loop ledger

Standing rule (2026-09-25): **one important audit at a time — audit, then fix, then the next audit, then fix.** One area per round. This file is the loop's record: read it first, update it as you go, and keep it short.

**Statuses:** `todo` · `IN PROGRESS since <UTC time>` · `done <date>` · `awaiting merge` · `blocked: <why>`.
Mark a row `IN PROGRESS` before starting it, and leave notes there if work stops mid-way.

## Queue (in order of importance)

| # | Area | Status | Result |
|---|---|---|---|
| 1 | **Join path**: Join button → mod plan (RULES, cached fallback) → Steam downloads (sync, stall, failures) → `!Workshop` junctions → launch arguments → `DayZ_BE.exe` start and exit watch | IN PROGRESS since 2026-09-25T16:10Z | |
| 2 | **Trust and verification**: R0–R12 (`browser/verify.rs`, `src/lib/types.ts` `isUntrusted`/`trustedPlayers`/`queueOf`, `cache.rs` `apply_verifications`), R11 continuity, the stale re-read after a pass, the details pane's verdict text | todo | |
| 3 | **Refresh and cache lifecycle**: Steam partitions and the `collapse_addr_hash` follow-up, upsert variants, the three prune lanes, vouch withdrawal, `servers:pruned`, WAL checkpoints, start-up load | todo | |
| 4 | **Steam session lifecycle**: `steam/sdk.rs` worker — init retry, idle release, Steam closing/restarting, friends and rich presence, the Workshop update poll | todo | |
| 5 | **Mods page and Workshop management**: scan, subscribe/unsubscribe, the update badge, the dangling-junction clean-up (never delete a junction the app did not create) | todo | |
| 6 | **Installer and updater**: NSIS template and `hooks.nsh` (D-262 icons included), update and uninstall modes, the updater manifest, the release workflow | todo | |
| 7 | **Front-end state and flows**: the servers store, filters, sort and counts, dialogs, event listeners — non-visual bugs only | todo | |
| 8 | **Security and privacy** | todo | |
| 9 | **Performance** at the current row counts (~40 000 cached servers) | todo | |
| 10 | **Docs against code** | todo | |
| 11 | **Accessibility** — report only: every item needs approval first | todo | |


## Waiting for approval (visual changes are never applied without it)

- From D-248: one word for the hidden set ("Hide inflated" vs "fake" vs "untrusted"); the "Has queue" chip (it filtered 4 488 rows to 2 on 2026-09-25); inline confirmations that replace the focused button (Unsubscribe, Clean, Clear list); a keys hint for ←/→, Space and Esc on the grid.

## Waiting for a check on the reference PC

- v0.1.54 (D-264): after the in-app update, the desktop shortcut, the Start menu entry and the taskbar button show the slightly bigger mask, and the taskbar icon is sharp. The fresh install of 0.1.52 was seen correct on screen (D-263).

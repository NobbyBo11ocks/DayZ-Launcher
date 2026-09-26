# 13 · Audit loop ledger

Standing rule (2026-09-25): **one important audit at a time — audit, then fix, then the next audit, then fix.** One area per round. This file is the loop's record: read it first, update it as you go, and keep it short.

**Statuses:** `todo` · `IN PROGRESS since <UTC time>` · `done <date>` · `awaiting merge` · `blocked: <why>`.
Mark a row `IN PROGRESS` before starting it, and leave notes there if work stops mid-way.

## Queue (in order of importance)

| # | Area | Status | Result |
|---|---|---|---|
| 1 | **Join path**: Join button → mod plan (RULES, cached fallback) → Steam downloads (sync, stall, failures) → `!Workshop` junctions → launch arguments → `DayZ_BE.exe` start and exit watch | done 2026-09-26 | D-265, released as v0.1.55: `-mod=` was in the reverse of the server's load order; a missing-folder mod waited 15 min; a closed dialog could still launch; 9 more |
| 2 | **Trust and verification**: R0–R12 (`browser/verify.rs`, `src/lib/types.ts` `isUntrusted`/`trustedPlayers`/`queueOf`, `cache.rs` `apply_verifications`), R11 continuity, the stale re-read after a pass, the details pane's verdict text | done 2026-09-26 | D-268, released as v0.1.57 (fixes) and v0.1.58 (the approved pane changes): R11 struck honest servers whose three oldest sessions left; R0 stuck when the first check could not count; the pane judged against a count of any age; the trust box lost R0's explanation and never gave R8's (approved fixes); 4 more |
| 3 | **Refresh and cache lifecycle**: Steam partitions and the `collapse_addr_hash` follow-up, upsert variants, the three prune lanes, vouch withdrawal, `servers:pruned`, WAL checkpoints, start-up load | done 2026-09-26 | D-271, released as v0.1.60: a full Refresh never withdrew a vouch; the cache and the store withdrew by different rules; the empty-list prune lanes deleted rows on maps never listed; 3 more. The carried-in vouch findings were (1) and (2) |
| 4 | **Steam session lifecycle**: `steam/sdk.rs` worker — init retry, idle release, Steam closing/restarting, friends and rich presence, the Workshop update poll | IN PROGRESS since 2026-09-26T04:57Z | |
| 5 | **Mods page and Workshop management**: scan, subscribe/unsubscribe, the update badge, the dangling-junction clean-up (never delete a junction the app did not create) | todo | |
| 6 | **Installer and updater**: NSIS template and `hooks.nsh` (D-262 icons included), update and uninstall modes, the updater manifest, the release workflow | todo | |
| 7 | **Front-end state and flows**: the servers store, filters, sort and counts, dialogs, event listeners — non-visual bugs only | todo | |
| 8 | **Security and privacy** | todo | |
| 9 | **Performance** at the current row counts (~40 000 cached servers) | todo | |
| 10 | **Docs against code** | todo | |
| 11 | **Accessibility** — report only: every item needs approval first | todo | |


## Waiting for approval (visual changes are never applied without it)

- From D-248: one word for the hidden set ("Hide inflated" vs "fake" vs "untrusted"); the "Has queue" chip (it filtered 4 488 rows to 2 on 2026-09-25); inline confirmations that replace the focused button (Unsubscribe, Clean, Clear list); a keys hint for ←/→, Space and Esc on the grid.
- ~~From D-268: keep the trust box's R0 text whenever the row is R0, in the warning colour~~ approved 2026-09-26 and applied in v0.1.58, wording unchanged. ~~Add "its player list shows N" to that text; relabel "Verified head-count" on the path where PLAYER did not answer and INFO said 0~~ approved the same day and applied in v0.1.58 ("Empty server"). ~~R8's explanation~~ approved the same day and applied in v0.1.58. Nothing from D-268 is waiting.

## Waiting for a check on the reference PC

- ~~v0.1.54 (D-264)~~ Seen 2026-09-25: the in-app updater took 0.1.52 to 0.1.54 by itself, and afterwards the desktop shortcut showed the bigger mask and the taskbar button was bigger and crisp (host-side captures before and after, compared side by side). Nothing waiting here now.

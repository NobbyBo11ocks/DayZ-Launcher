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
| 4 | **Steam session lifecycle**: `steam/sdk.rs` worker — init retry, idle release, Steam closing/restarting, friends and rich presence, the Workshop update poll | done 2026-09-26 | D-275, released as v0.1.61: a Steam restart re-opened a released session by itself; the update badge fell back to the file while released; install-and-restart skipped the Steam shutdown; 8 more |
| 5 | **Mods page and Workshop management**: scan, subscribe/unsubscribe, the update badge, the dangling-junction clean-up (never delete a junction the app did not create) | done 2026-09-26 | D-276, released as v0.1.62: the junction clean-up could remove junctions to folders that were there or were never Workshop items; Update re-subscribed unsubscribed mods; "Scan now" could never clear its own count; 5 more. The two approved visual changes followed in v0.1.63 (D-277) |
| 6 | **Installer and updater**: NSIS template and `hooks.nsh` (D-262 icons included), update and uninstall modes, the updater manifest, the release workflow | done 2026-09-26 | D-279, released as v0.1.64: installs from before the rename updated into a second copy; a wrong signing key would have been caught only after publishing; the updater kept offering a deleted release; 10 more. The three approved visible changes shipped in the same release (D-280) |
| 7 | **Front-end state and flows**: the servers store, filters, sort and counts, dialogs, event listeners — non-visual bugs only | done 2026-09-26 | D-281, released as v0.1.65: the details pane kept a server's first verdict; its missing marks disagreed with the join plan; uncountable rows were checked again on every scroll; 8 more |
| 8 | **Security and privacy** (the `junction` crate's `unstable_admin` carry-over from row 5 is fixed already, D-282) | IN PROGRESS since 2026-09-26T12:46Z | |
| 9 | **Performance** at the current row counts (~40 000 cached servers) | todo | |
| 10 | **Docs against code** | todo | |
| 11 | **Accessibility** — report only: every item needs approval first | todo | |


## Waiting for approval (visual changes are never applied without it)

- From D-248: one word for the hidden set ("Hide inflated" vs "fake" vs "untrusted"); the "Has queue" chip (it filtered 4 488 rows to 2 on 2026-09-25); inline confirmations that replace the focused button (Unsubscribe, Clean, Clear list); a keys hint for ←/→, Space and Esc on the grid.
- ~~From D-268: keep the trust box's R0 text whenever the row is R0, in the warning colour~~ approved 2026-09-26 and applied in v0.1.58, wording unchanged. ~~Add "its player list shows N" to that text; relabel "Verified head-count" on the path where PLAYER did not answer and INFO said 0~~ approved the same day and applied in v0.1.58 ("Empty server"). ~~R8's explanation~~ approved the same day and applied in v0.1.58. Nothing from D-268 is waiting.
- ~~From D-276: (a) show "—" in the Mods page's Servers column and disable "On no server" until the server list's mod lists have loaded, and drop "Populated" from the column's tooltip; (b) name the mod a download failed on, and show a download replaced by a newer one as a notice, not an error~~ approved 2026-09-26 and applied in v0.1.63 (D-277). The request called (b)'s second half an unsubscribe replaced by a newer one; the finding was the download ("superseded by a newer sync"), which is what changed.
- ~~From D-279: (a) check for updates again while the launcher stays open; (b) show the window again when Windows refuses the downloaded setup; (c) ask before "Install and restart" while DayZ is running or a mod download is in progress~~ approved 2026-09-26 and applied in v0.1.64 (D-280).

## Waiting for a check on the reference PC

- ~~v0.1.54 (D-264)~~ Seen 2026-09-25: the in-app updater took 0.1.52 to 0.1.54 by itself, and afterwards the desktop shortcut showed the bigger mask and the taskbar button was bigger and crisp (host-side captures before and after, compared side by side). Nothing waiting here now.

# 11 · Fake population ("spoofed") servers: evidence and detection

Researched 2026-09-21, after the M2 sweep showed impossible player totals. Sources S-51…S-54 in [08](08-sources.md); measurements D-038, D-043 in [09](09-decisions-log.md). Everything below was verified live with [tools/spoof_probe.js](../tools/spoof_probe.js).

## 1. What is being faked, and how

- **Mechanism.** A public memory patch for the DayZ server binary (S-51) overwrites the player count the server reports in **A2S_INFO**. The fake count is dynamic: *"20 fake players + 1 real player = 21 reported"*. Community reports (S-52, Bohemia feedback T186766) put the scale at ~9 000 fake servers as of January 2025, many redirecting joiners to paid-item servers; Bohemia resolved one farm in October 2024 but the list-wide problem remains.
- **What stays truthful.** A2S_PLAYER is answered by the unpatched code path and lists real players only (one anonymous entry per player with connection duration, D-034). A second spoof variant (a proxy in front of the query port) answers INFO but **never answers PLAYER** at all.
- **Every list source inherits the lie.** Steam's `ISteamMatchmakingServers` pings servers with A2S_INFO, so its `players` is inflated too; DZSA, BattleMetrics and the official launcher are all downstream of A2S. The claim in community threads that "the Steam API can't be spoofed" refers to Steam's game-wide concurrent-player statistic, not per-server counts (S-53). There is no authenticated per-server head-count available to a launcher.

## 2. Live measurement (random sample of 128 DZSA addresses + 8 sweep suspects)

| Group | Count | Pattern |
|---|---|---|
| Answered INFO | 126 | – |
| **Inflated**: INFO 52–127 players, PLAYER list empty | **55** | e.g. `[Solo+] Divide Horizon 120/120`, `ZEUS 127/127`; hosted on a handful of IPs (`31.76.60.155` ×11, `104.248.31.234` ×8, `23.88.48.223` ×5, `213.176.72.x`, `194.41.113.x`, `93.88.200.x`, `31.77.139.x`) |
| **Unverifiable**: INFO 81–127/127, PLAYER never answers | **25** | all on `31.77.142.x` / `31.76.54.x` / `2.26.11.210` ("XOMA PvP", "GROZA", "MIDNIGHT PLUS", "HATE STALKER RP") |
| Consistent, populated | 13 | INFO = PLAYER count exactly; durations distinct and plausible (e.g. 16/40 with 16 distinct durations 24–1578 s) |
| Consistent, empty | 33 | 0/0 |

**63 % of a random sample is fake.** Of 13 464 servers in the cache after the first Steam refresh, 2 988 report players > 0; the probe suggests roughly one in six of those is genuine, consistent with DZSA's own count of ~2 030 populated servers.

## 2b. Steam's master server does not repeat the lie (D-044/D-045)

The first partitioned refresh returned 2 988 servers for Steam's `hasplayers` filter and 10 000 (capped) for `noplayers`, yet 11 563 of the 13 491 cached rows carry INFO `players > 0` and 8 747 claim 61+. Those thousands came back from the **`noplayers`** partition: Steam's master evaluates the filter on the game server's authenticated Steam session (the count it learns from player auth tickets), which the A2S memory patch does not touch. The eight busiest IPs in the cache (`31.76.60.155`, `144.31.214.170`, `89.169.13.187`, `172.236.0.90`, `136.244.118.75`, `65.20.105.33`, `194.58.57.61`, `87.76.179.22`) each host 176–395 "populated" servers, all of them in that group.

A full per-map refresh (D-046) put the scale beyond doubt: **27 037 of 33 605 responding servers are inflated**, i.e. Steam's list holds roughly 6 500 genuine DayZ servers (≈ 3 000 populated, ≈ 3 900 empty) and about 27 000 fakes, concentrated on chernarusplus, enoch and namalsk (each capped at 10 000 with 77–90 % fakes). Because of that, the automatic refresh on start only asks Steam for `hasplayers` servers. The manual Refresh runs the whole partition list, which begins with the populated servers and then walks the empty ones map by map (D-141).

So partition membership is a free, list-level inflated flag: a row from a `noplayers` partition whose INFO count is positive is fake (rule R0). This is why community posts say the "Steam API can't be spoofed" (S-53): the Web API's `players` and the master's filters use the same authenticated count. A future spoofer would have to fake Steam auth sessions, which is where rules R2–R5 take over.

## 3. Detection rules (R0 shipped in M3; R2–R5 in M4; **R6–R7 specified, never built**)

Trust is computed per server from cheap signals; the INFO number is never shown as fact.

| # | Rule | Verdict |
|---|---|---|
| R0 | Row came from a Steam `noplayers` partition (`steamEmpty = true`) and INFO `players > 0` | **Inflated** — badge, hidden by the default "Hide inflated" filter, sorted as 0 |
| R1 | INFO `players` = 0 | nothing to verify; show 0 |
| R2 | PLAYER answers; `verified = entries`; `INFO − verified ≤ 2` | **Verified** — show `verified` |
| R3 | PLAYER answers; `INFO − verified ≥ 5` or ≥ 20 % of `max_players` | **Inflated** — show `verified`, badge, hidden by the default "Hide inflated" filter |
| R4 | INFO `players` > 0 but PLAYER times out (after a patient 2.5 s retry) while INFO keeps answering | **Unverifiable** — treated like Inflated **only when Steam has not vouched for the server** (`steamEmpty ≠ false`); Steam-vouched rows stay visible with the server's own number and a "Steam-confirmed" note. Rare in practice: 6 of 2 858 populated servers (D-050); a first, burstier implementation mis-labelled 676 (D-047) |
| R5 | PLAYER answers with ≥ 5 entries and **either** at most two distinct durations, **or** any non-empty name, **or** all durations under 60 s *and* repetitive. D-160 stopped "all young" firing on its own — a freshly restarted server looks exactly like that | **Synthetic list** — treated like Inflated (defence against future PLAYER spoofing) |
| ~~R6~~ | *Not implemented.* Across samples: INFO stays constant while `verified` stays 0 for ≥ 3 samples | would need a persisted trust score; `verify.rs` judges one sample at a time and its own header says "rules R2–R5" |
| ~~R7~~ | *Not implemented.* ≥ 10 servers on one IP with ≥ 80 % Inflated/Unverifiable | would need per-IP aggregation, which nothing in the host or the front end does |

Hosting providers legitimately run many servers per IP, so R7 could only ever be a prior, never a verdict on its own — part of why it has not been built. Names ("official", "Vanilla++") are never used as signals.

## 4. Cost

PLAYER is one datagram each way. Measured (D-050): the automatic pass after a refresh verifies every populated server (2 858) in 33 s at the client's 400 datagrams/s, sending PLAYER only and retrying non-answers once with INFO at a 2.5 s timeout; rows on screen are re-checked with INFO + PLAYER once their last verification is older than 120 s (0.1–2 s for a screenful). Empty servers are not queried. Sending INFO + PLAYER for everything in one burst cost 60 s and produced hundreds of false "unverifiable"/"offline" results (D-047), so burst size matters more than raw rate.

## 5. UX

- Players column shows the verified count; until verified, the INFO number is dimmed with a "reported" tooltip.
- Badges: ⚠ *inflated*, ? *unverifiable*; both excluded by the default filter "Hide inflated servers" (user can disable).
- Sorting by players uses the verified number, so farms drop to the bottom instead of the top.
- Details pane shows both numbers and the rule that fired, so the user can see why.

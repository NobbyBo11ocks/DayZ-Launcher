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

## 3. Detection rules (R0 shipped in M3; R2–R5 in M4; the R6–R7 of this table were never built — the R6 in use is the vouch rule of §3b, and R8–R12 follow it)

Trust is computed per server from cheap signals; the INFO number is never shown as fact.

| # | Rule | Verdict |
|---|---|---|
| R0 | Row came from a Steam `noplayers` partition (`steamEmpty = true`) and INFO `players > 0` | **Inflated** — badge, hidden by the default "Hide inflated" filter, sorted as 0 |
| R1 | INFO `players` = 0 | nothing to verify; show 0 |
| R2 | PLAYER answers; `verified = entries`; `INFO − verified ≤ 4` on every path — a slack of two was shipped for an afternoon on the reasoning that a fresh INFO sits 50 ms from PLAYER, and a 245-address live probe found ~70 honest servers on one hosting provider whose INFO is an edge cache and whose PLAYER is a 100-second snapshot, disagreeing by a few for ~15 minutes around every restart; the 20 % clause needs at least three ghosts, since one or two connecting players on a ten-slot server tripped it four times in 102 (D-233) | **Verified** — show `verified` |
| R3 | PLAYER answers; `INFO − verified ≥ 5` or ≥ 20 % of `max_players` | **Inflated** — show `verified`, badge, hidden by the default "Hide inflated" filter |
| R4 | INFO `players` > 0 but PLAYER times out (after a patient 2.5 s retry) while INFO keeps answering | **Unverifiable** — untrusted **unless Steam vouches for the server *and* a real head-count was taken before**, in which case the last head-count stays on screen. A Steam-vouched claim nobody has ever counted is untrusted, its cell shows the dimmed "?" claim, and it sorts as zero: the earlier exemption trusted the server's own INFO for any operator holding one Steam session and dropping PLAYER, which put three fingerprinted farm boxes in the default top 25 at 96, 67 and 44 (D-233). Honest firewalled servers: 6 of 2 858 (D-050); a burstier implementation once mis-labelled 676 (D-047) |
| R5 | PLAYER answers with ≥ 5 entries and **either** at most two distinct durations, **or** any non-empty name, **or** all durations under 60 s *and* repetitive. D-160 stopped "all young" firing on its own — a freshly restarted server looks exactly like that | **Synthetic list** — treated like Inflated (defence against future PLAYER spoofing) |
| ~~R6 (superseded)~~ | *Not implemented; the number now names the vouch rule of §3b (D-233).* Across samples: INFO stays constant while `verified` stays 0 for ≥ 3 samples | would need a persisted trust score; `verify.rs` judges one sample at a time and its own header says "rules R2–R5" |
| ~~R7~~ | *Not implemented.* ≥ 10 servers on one IP with ≥ 80 % Inflated/Unverifiable | would need per-IP aggregation, which nothing in the host or the front end does |

Hosting providers legitimately run many servers per IP, so R7 could only ever be a prior, never a verdict on its own — part of why it has not been built. Names ("official", "Vanilla++") are never used as signals.

## 3b. Rules added 2026-09-24 (D-233)

Four audits ran against the live cache (24 116 servers, 5 516 verified, a full day of population samples) and the published tools that fake population. Each rule below carries the measurement that justified it; the false-positive check is the honest population it was tested against.

| Rule | Where | Condition | Yield on the live cache | False-positive check |
|---|---|---|---|---|
| R6 | `types.ts` `isUntrusted`, `trustedPlayers` | `unverifiable` and never counted → untrusted; sorts as 0 even with the filter off | 3 rows in the default top 25 (96/127, 67/127, 44/127; two on adjacent 185.248.x /24s, one in the 31.77.142.0/24 farm that holds 3 727 rows) | The honest-and-firewalled busy server: 0 observed in two caches; the class exists by construction and stays reachable with the filter off |
| R8 | `types.ts` `isUntrusted` | INFO `players > 127` → untrusted | 2 323 rows, every one already R0 | Largest head-count ever verified here is 116; `max_players > 127` is **not** used (one honest 225-slot server exists) |
| R9 | `types.ts` `isUntrusted` | never counted ∧ `bots > 0` ∧ `bots == players` → untrusted (a prior, only on rows without a head-count) | 0 new today; covers 5 093 of 5 093 R0 rows and 45 of 102 R3 rows — insurance for an INFO patcher that also holds a Steam session | 3 honest verified rows have `bots == players` (two Wasteland Warriors, one RETRO DAYZ); all three carry a head-count that exempts them. DZSA rows always carry `bots = 0` (`dzsa.rs`), so the prior is blind there |
| R10 | `servers.svelte.ts` `#recomputeClones` | exact name (case and whitespace folded) of a server verified at ≥ 5 players on another address, and this copy not verified → untrusted; clears when the copy verifies | 2 771 rows: 177 R0, 15 unverifiable, 2 579 DZSA phantoms at "? 0/127" that were 67 % of the visible list | 0 of 1 077 honest populated names run on two addresses; the 22 collisions at 1–4 players are generic names ("DayZ Server", "test server") and never reach the ≥ 5 reference set |
| R11 | `verify.rs` `continuity` | between two checks ≥ 60 s apart, real session durations reappear advanced by *some* shift within ±120 s of the gap — the wall-clock gap on a direct-answer host, the snapshot's age on a snapshot-serving one (400.0 s over a 341.9 s gap was measured live) — matched to 3 s once the shift is found; a list where no shift carries over more than one session — or a fifth, on a busy server — twice in a row, is **Synthetic**. Restarts (every session younger than the gap) and lists that halved are exempt; a later check that carries over clears the strikes. **Since D-237:** checks more than 30 minutes apart are not compared (churn alone could strike an honest server across a 90-minute gap), exempt and impossible comparisons keep the strikes they found instead of clearing them, the verdict stands until a comparison overturns it — across a restart (seeded from the cached verdict) and across a details-pane check (which now runs R11 too) | 0 today: no host runs ≥ 8 verified ≥ 10-player servers, and the cache holds no durations to size it offline. It was written against T2 as docs/11 then described it (`random() × 10 000` per query), which R2 and R5 both pass the moment the server has one real player; that is only the tool's first commit — the version that ships writes zero-length sessions, which R11 barely sees and R12 catches (D-238). R11 still covers any faker that re-draws its list | Deathmatch churn: half the old sessions must vanish *and* it must happen twice in a row; the pure matcher is unit-tested against a twenty-session list with one join and one leave (19 of 20 carry over) and against a re-drawn list with one real player (1 of 20) |

Also from the same audits, applied without a rule number: the **queue tag** (`lqs<N>`, a free-text keyword) is believed only when the server is actually full — a 7-player server was showing "+172"; a **fresh Steam batch saying empty with INFO at 0 beats a stored head-count** — 87 rows painted a 20-hour-old 3–5 over an empty server; and **Steam's vouch expires**: a complete refresh withdraws it from every server it did not return, where before it survived up to the 30-day prune (2 669 vouched rows were absent from the latest refresh).

### Corrections from the 2026-09-25 audit (D-237)

A code audit, not a new measurement: each change removes a path by which the rules above misjudged a server, and none of them adds a signal.

- **R3 is judged against a fresh INFO.** The automatic pass skipped INFO because Steam's is "a minute old at most" (D-047), which stopped being true when every Refresh became the full, multi-minute pass (D-141, 473 s measured in D-046). A restart or ordinary churn between the listing and the PLAYER query read as Inflated and hid the server until the next Refresh. A count older than 60 s is now re-read, and a check without a fresh INFO no longer writes its fallback count over the row.
- **R0 yields to our own count** when the listing that said "empty" also said 0 players: a server listed by `noplayers` and filled since was hidden as inflated the moment it was verified. A listing that claimed players (every farm does) keeps R0.
- **A list judged synthetic is not a head-count.** It used to be stored as one, so R6's "counted before" exemption trusted a farm again at its fabricated number the first time its PLAYER timed out; such a row now sorts as 0 and shows its claim under the warning.
- **R11** as above: a maximum gap, strikes that survive exempt comparisons, and a verdict that survives restarts and the details pane.

### R12: sessions no clock produced (D-238)

| Rule | Where | Condition | Yield | False-positive check |
|---|---|---|---|---|
| R12 | `verify.rs` `judge`, before R5 | two or more PLAYER entries with 0 < duration < 1 ms: those entries are not counted, and the verdict is **Inflated** with the remaining sessions as the head-count | Every server running T2 as it ships (above) with a real player on it; not measured live — there is no live cache in the container this was written in | A real duration under a millisecond needs the query to land within a millisecond of the connection, for two players at once; one such entry alone is exempt, and an entry at exactly 0.0 is not counted as zero-length. **Measured 2026-09-25 (D-243):** 0 of 259 honest lists from 150 verified servers carried even one zero-length entry |

### What the live probe found (245 addresses, 345 records, 14:52–15:08 UTC, D-037 respected)

**No server the launcher marks verified was shown to carry a fabricated or inflated population.** Every verified row claiming ≥ 30 was probed, 100 of them two or three times; 16 of 18 R0-hidden farm rows answered PLAYER with zero entries (`207.154.232.44:6448` claims 250/250, lists nobody) and none returned a populated list, so R0 stands. The lists are real everywhere: 221 lists, zero names, zero non-zero scores, byte size always `6 + 10·n`, join-ordered, modest churn (91 % stayed over 5½ minutes on the controls), longest session 10.0 h.

What the probe did find is **a query cache on one hosting provider** (`185.207.214.x`, `195.18.27.x`, `80.242.59.x`, `185.189.255.x`, `109.248.4.x`): INFO answers in ~11 ms from something nearer than the game server, PLAYER in ~55 ms from a snapshot refreshed every 100 s — 70 of 195 suspects, 0 of 32 controls. Around the hosts' synchronised restarts INFO and PLAYER disagree for ~15 minutes in both directions (`195.18.27.183:2305` answered INFO 0/110 while PLAYER listed 97, which no spoofer would produce), and session durations advance by the snapshot's age rather than the gap. Two of this afternoon's own changes would have misread that: a tolerance of three (reverted) and a continuity matcher with a fixed window (replaced by the shift search). A third finding: R5's `≤ 2 distinct durations` clause fired on SALVATION's five-player restart reconnect (10.9–11.9 s; 34 more joined within five minutes) and now requires the list not to be young.
### What was measured and rejected

The temporal signals — flat counts, always at max, round numbers only, a queue in every sample, lockstep siblings, no night dip — all yield **0** on 1 027 verified series with ≥ 5 samples, and the one server that trips four of them is KarmaKrew Namalsk #1, honest and full at 55/55 with a login queue. The `population` table holds verified head-counts only (`publish()` writes samples for `Verified` alone), so it cannot contain a fake by construction. R7 as a verdict adds 0: the 74 hosts with ≥ 10 servers and ≥ 80 % flagged are 100 % R0 already, and one hosting provider runs six honest servers on one address (169 verified players). Numbered-name twins trip Bohemia's own officials; identical mod lists are how communities work; the keyword-string fingerprint of the three farm boxes is shared by ten honest MAGNUS STALKER-PVE servers.

### The techniques, mapped (sources in D-233)

| Technique | Tool or evidence | Caught by |
|---|---|---|
| T1 INFO count patched in the server process | DataGoblin memory opcode (2023), LinuxPhantom `hid.dll` (2024) | R0 when no real player; R3 when some; R9 as a prior |
| T2 query-port proxy rewriting INFO *and* PLAYER | anatolykopyl, Go, last commit 2026-01 (S-80). **Corrected 2026-09-25 against the source:** only its first commit (3e6c0fe) used `random() × 10 000` durations. Every commit since — c539ddc the same day, the only release v1.0.0, and master — still draws them but appends each of its `amount` entries (default 10) with index 0, no name, score 0 and the constant duration bytes `00 00 00 01`, 2.35 × 10⁻³⁸ s. It raises the PLAYER count uncapped and the INFO count capped at max | R0 with zero real players; **R12** otherwise (D-238). Before R12, R5 caught it only with at most one real player and R11 only when four fifths of the list were fake: the four-player live capture plus the tool's default output verified at 14 |
| T3 proxy or firewall answering INFO, never PLAYER | 25 of 128 in the S-54 sample; the 31.77.142.x farm | R4, now without the vouch loophole (R6) |
| T4 mirror entries, one host registering dozens–hundreds of Steam entries | D-044 (8 hosts with 176–395 entries); Steam threads 2024–25 | R0 per entry; R10 for the name clones |
| T5 impersonating a real community's name | Bohemia tracker T196720 / T195788 | R10 |
| T6 queue inflation | "255/255 + 2 million in queue" reports | queue shown only when full |
| T7 real bot clients (licensed accounts idling) | **no evidence found** in EN/RU/ZH; ≥ $49.99 a seat and one account cannot sit on two servers | none — out of A2S's reach; R11 sees them as real sessions, because they are |
| T8 `CreateUnauthenticatedUserConnection` sessions | Steamworks header; whether `hasplayers` counts them is undocumented | open question Q26 |
| T9 caching DDoS proxies (up to 10 s stale) | gnif SteamQueryProxy, 0x280 | not a fake; R11's shift search (±120 s window, 3 s once the shift is found) absorbs it |

### The farms outgrew the list (D-245)

On 2026-09-25 Steam's master list carried 36 100 DayZ entries, 30 011 of them inflated at listing (R0 or R8), against 24 237 the day before. The `noplayers` partitions for enoch, namalsk and chernarusplus held 14 751, 14 316 and 11 223 entries that hour — 96–98 % fake — so each exceeded Steam's 10 000-row cap, and the farms rotate ports: of the ids one and two hours older than the latest refresh, 10 048 and 11 695 were not in it. The cache had grown from 24 116 to 71 397 rows in a day; 52 567 of them were fakes and 64 398 had never been counted by a verification pass.

What changed, none of it a new signal: a row only the DZSA fallback produced leaves when a listing of the empty partitions did not return it (D-247), a row never counted leaves the cache after three days unseen, and a fake-at-listing row leaves as soon as a listing of the empty partitions did not return it — a capped or timed-out listing counts, which only ever costs a fake its row until it is listed again (favourites stay; the store drops the same ids). A map partition that hits the cap is followed by the same request with Steam's `collapse_addr_hash` filter (S-83), one server per address, which no farm can fill; the catch-all partition asks that way from the start.

Measured and rejected as a master-server exclusion: the farms' `shard` tags. `shardABC123` and `shard123ABC` appear on 41 511 of the 41 514 fake rows of that hour — and on 1 805 and 1 941 verified servers, so a `nor` on them would have hidden thousands of honest servers. Rows per address is not usable at the master either: 22 verified servers sit on addresses carrying 20 or more entries, 0 on addresses carrying 50 or more, but the filter grammar has no such operator.

## 4. Cost

PLAYER is one datagram each way. Measured (D-050): the automatic pass after a refresh verifies every populated server (2 858) in 33 s at the client's 400 datagrams/s, sending PLAYER only and retrying non-answers once with INFO at a 2.5 s timeout; rows on screen are re-checked with INFO + PLAYER once their last verification is older than 120 s (0.1–2 s for a screenful). Empty servers are not queried. Sending INFO + PLAYER for everything in one burst cost 60 s and produced hundreds of false "unverifiable"/"offline" results (D-047), so burst size matters more than raw rate.

## 5. UX

- Players column shows the verified count; until verified, the INFO number is dimmed with a "reported" tooltip.
- Badges: ⚠ *inflated*, ? *unverifiable*; both excluded by the default filter "Hide inflated servers" (user can disable).
- Sorting by players uses the verified number, so farms drop to the bottom instead of the top.
- Details pane shows both numbers and the rule that fired, so the user can see why.

// The details pane's verdict in words (src/lib/verdict.ts) against the host's reasons. Each
// reason below is written the way browser/verify.rs formats it — its own tests pin the
// formats of R13, R14 and R11 — so a rule reworded on one side only fails here instead of
// showing the general "looks fake" line.
import test from "node:test";
import assert from "node:assert/strict";
import { freshApp } from "./harness.mjs";

const check = (verdict, reason, extra = {}) => ({
  id: "203.0.113.7:2303",
  verdict,
  reported: 40,
  verified: 12,
  maxPlayers: 60,
  pingMs: 30,
  tags: null,
  verifiedAt: 0,
  reason,
  ...extra,
});

test("every rule's reason has its sentence, the three of D-314 their own (D-248, D-315)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { explainVerdict, verdictHeading } = await app.load("verdict");
  const cases = [
    // R13, R14 and R11's invariant (D-314), in the approved words (D-315).
    [check("synthetic", "11 entries on a server of 10 slots"), "The player list looks fake: it lists more players than the server has slots."],
    [check("synthetic", "3 sessions under a second beside one of 2400 s"), "The player list looks fake: several players joined within the same second as long-running ones."],
    [check("synthetic", "5 of 25 sessions older than the gap were missing from the list 300 s earlier"), "The player list looks fake: players it lists as older were not there at the previous check."],
    // R11's count, the standing verdict, and R5.
    [check("synthetic", "4 of 20 sessions carried over between checks 120 s apart"), "The player list looks fake: the sessions seen at the previous check did not carry over."],
    [check("synthetic", "sessions did not carry over at earlier checks; none since was close enough to compare"), "Sessions did not carry over at earlier checks; waiting for a check close enough to compare."],
    [check("synthetic", "6 entries, 1 distinct durations, all_young=true, named=false"), "The player list looks fake: 6 entries with only 1 different session length."],
    [check("synthetic", "6 entries, 6 distinct durations, all_young=false, named=true"), "The player list looks fake: its entries carry names, which real DayZ lists never do."],
    // R12, R3, R2 and the rest.
    [check("inflated", "INFO 14, 4 real sessions: 10 listed entries are zero-length, which no connected player is", { verified: 4 }), "4 real players; the rest of its list are fake entries with no play time."],
    [check("inflated", "INFO 40 vs PLAYER 12"), "The server claims 40 players; 12 are actually connected."],
    [check("verified", "INFO 12 vs PLAYER 12", { reported: 12 }), "12 counted; the server claims 12."],
    [check("verified", "PLAYER 12; INFO did not answer", { reported: -1 }), "12 counted; the server's own number did not arrive."],
    [check("verified", "INFO reports 0 and PLAYER did not answer", { reported: 0, verified: 0 }), "The server reports nobody on it and did not share its player list."],
    [check("unverifiable", "INFO reports 40 but PLAYER did not answer (timed out)", { verified: null }), "The server claims 40 players but does not share its player list."],
    [check("offline", "no PLAYER reply", { verified: null }), "The server did not answer at all."],
  ];
  for (const [v, want] of cases) assert.equal(explainVerdict(v), want, v.reason);
  assert.equal(explainVerdict(check("synthetic", "a reason no rule writes")), "The player list looks fake.");

  assert.equal(verdictHeading(check("verified", "INFO reports 0 and PLAYER did not answer")), "Empty server");
  assert.equal(verdictHeading(check("verified", "INFO 12 vs PLAYER 12")), "Verified player count");
  assert.equal(verdictHeading(check("synthetic", "11 entries on a server of 10 slots")), "Fake player list");
});

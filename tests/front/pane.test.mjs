// The details pane's arithmetic (src/lib/pane.ts, and the two helpers the list shares with
// it in src/lib/types.ts): row 26's approved changes. The pane's markup reads these; the
// host's half (the scanned list, the population samples) has its tests in commands.rs.
import test from "node:test";
import assert from "node:assert/strict";
import { freshApp, row } from "./harness.mjs";

test("the Players line shows the list's number, and the claim only beside a head-count (row 26)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { playersShown } = await app.load("types");
  const { claimBeside } = await app.load("pane");
  // Nobody could count it: the pane showed a bold 0 where the list shows ⚠ 40/60.
  const refuses = row({ players: 40, verdict: "unverifiable", verifiedPlayers: null });
  assert.equal(playersShown(refuses), 40);
  assert.equal(claimBeside(refuses), null, "no 'server claims' beside the claim itself");
  // A head-count beside a larger claim.
  const counted = row({ players: 40, verifiedPlayers: 31, verdict: "inflated" });
  assert.equal(playersShown(counted), 31);
  assert.equal(claimBeside(counted), 40);
  // Steam's fresh "empty" puts 0 where an old head-count was: that 0 is no head-count.
  const emptied = row({ players: 0, verifiedPlayers: 5, steamEmpty: true, verdict: "verified" });
  assert.equal(playersShown(emptied), 0);
  assert.equal(claimBeside(emptied), null);
  assert.equal(claimBeside(row({ players: 12, verifiedPlayers: 12, verdict: "verified" })), null, "nothing to say when they agree");
  assert.equal(playersShown(row({ players: 9, verdict: null })), 9, "not checked yet: the claim, as the list shows it");
});

test("the other servers at an address leave out the pane's own and put the untrusted last (row 26)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { siblingsOf } = await app.load("pane");
  const ip = "203.0.113.9";
  const self = row({ ip, gamePort: 2302, queryPort: 27016 });
  // The same server, listed again under the query port it had before.
  const oldPort = row({ ip, gamePort: 2302, queryPort: 2303 });
  const busy = row({ ip, gamePort: 2402, players: 50, verifiedPlayers: 48, verdict: "verified" });
  const quiet = row({ ip, gamePort: 2502, players: 5, verifiedPlayers: 5, verdict: "verified" });
  // The largest count at the address, and a fake one.
  const fake = row({ ip, gamePort: 2602, players: 60, verifiedPlayers: 60, verdict: "synthetic" });
  const elsewhere = row({ ip: "203.0.113.10", gamePort: 2702 });
  assert.deepEqual(
    siblingsOf(self, [self, oldPort, fake, quiet, busy, elsewhere]).map((r) => r.id),
    [busy.id, quiet.id, fake.id],
  );
});

test("the Connected section counts real sessions only (row 26)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { sessionSummary } = await app.load("pane");
  const p = (durationSecs) => ({ index: 0, name: "", score: 0, durationSecs });
  // R12: two entries no clock produced (the tool's 2.35e-38 and an exact 0) are no sessions.
  const s = sessionSummary([p(40), p(700), p(4000), p(2.35e-38), p(0)], "inflated");
  assert.equal(s.count, 3);
  assert.equal(s.recent, 1, "nor arrivals");
  assert.equal(s.median, 700);
  assert.equal(s.longest, 4000);
  assert.deepEqual(s.buckets.map((b) => b.n), [2, 0, 1, 0, 0]);
  assert.equal(s.peak, 2);
  // One alone is not R12's, and stays.
  assert.equal(sessionSummary([p(40), p(0)], "verified").count, 2);
  // A list judged fake has no sessions to show.
  assert.equal(sessionSummary([p(40), p(700)], "synthetic"), null);
  assert.equal(sessionSummary([], "verified"), null);
  assert.equal(sessionSummary(null, "verified"), null);
});

test("each mod once, marked as the join sees it (row 26)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { distinctMods, modMark, modCounts } = await app.load("pane");
  const m = (workshopId, name) => ({ workshopId, name });
  const mods = distinctMods([m(1559212036, "CF"), m(0, "ServerPack"), m(1564026768, "COT"), m(1559212036, "CF again"), m(0, "ServerKeys"), m(0, "ServerPack")]);
  assert.deepEqual(mods.map((x) => x.name), ["CF", "ServerPack", "COT", "ServerKeys"]);
  const installed = new Set([1559212036, 1564026768]);
  const stale = new Set([1564026768, 999]);
  assert.equal(modMark(m(0, "ServerPack"), installed, stale), "server-side");
  assert.equal(modMark(m(0, "ServerPack"), null, stale), "server-side");
  assert.equal(modMark(m(1559212036, "CF"), installed, stale), "installed");
  assert.equal(modMark(m(1564026768, "COT"), installed, stale), "update");
  assert.equal(modMark(m(999, "Gone"), installed, stale), "missing", "no folder: a download, whatever Steam says of its version");
  assert.equal(modMark(m(1559212036, "CF"), null, stale), "unknown");
  assert.deepEqual(modCounts([...mods, m(999, "Gone"), m(999, "Gone")], installed, stale), { missing: 1, toUpdate: 1 });
  assert.deepEqual(modCounts(mods, null, stale), { missing: 0, toUpdate: 0 }, "nothing is missing while the installed set is unknown");
});

test("an unknown server version is no difference (row 26)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { versionDiffers } = await app.load("types");
  assert.equal(versionDiffers("1.28.161464", "1.29.163709"), true);
  assert.equal(versionDiffers("1.29.163709", "1.29.163709"), false);
  assert.equal(versionDiffers("", "1.29.163709"), false, "unknown");
  assert.equal(versionDiffers("1.28.161464", null), false, "no DayZ found: nothing to compare with");
});

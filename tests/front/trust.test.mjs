// The trust model the list, its sort and the title-bar count rest on (src/lib/types.ts:
// `isInflated`, `isUntrusted`, `trustedPlayers`, `queueOf`). docs/11 has the rules; the
// host's `judge` (browser/verify.rs) is the other half and has its own tests.
import test from "node:test";
import assert from "node:assert/strict";
import { freshApp, row } from "./harness.mjs";

test("the trust rules: which rows are hidden as untrusted, what count they sort by, and when a queue is believed (D-233, D-237, D-268, D-296 (3))", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { isInflated, isUntrusted, trustedPlayers, queueOf } = await app.load("types");
  // An honest server Steam lists as populated and a check counted.
  const honest = row({ players: 20, maxPlayers: 60, steamEmpty: false, verifiedPlayers: 18, verdict: "verified", bots: 0 });
  assert.equal(isUntrusted(honest), false);
  assert.equal(trustedPlayers(honest), 18, "the head-count, not the claim");

  // R0: Steam says empty, INFO claims players.
  const r0 = { ...honest, steamEmpty: true, players: 40, verifiedPlayers: null, verdict: null };
  assert.equal(isInflated(r0), true);
  assert.equal(isUntrusted(r0), true);
  assert.equal(trustedPlayers(r0), 0);
  // A fresh "empty" with INFO agreeing at 0 beats a head-count from hours ago.
  assert.equal(trustedPlayers({ ...honest, steamEmpty: true, players: 0, verifiedPlayers: 5 }), 0);

  for (const verdict of ["inflated", "synthetic", "offline"]) assert.equal(isUntrusted({ ...honest, verdict }), true, verdict);
  // R8: more than any DayZ server this launcher has counted.
  assert.equal(isUntrusted({ ...honest, players: 128 }), true);
  assert.equal(isUntrusted({ ...honest, players: 127 }), false);
  // R9: the bots byte equal to the claim, on a row nobody has counted.
  assert.equal(isUntrusted({ ...honest, verifiedPlayers: null, verdict: null, bots: 20 }), true);
  assert.equal(isUntrusted({ ...honest, bots: 20 }), false, "a head-count exempts it");
  // R10: a name cloned from a verified server elsewhere.
  assert.equal(isUntrusted({ ...honest, clone: true }), true);
  // R4 and R6: a server that refuses PLAYER keeps its place only with Steam's vouch and a count taken before.
  const refuses = { ...honest, verdict: "unverifiable" };
  assert.equal(isUntrusted(refuses), false);
  assert.equal(isUntrusted({ ...refuses, verifiedPlayers: null }), true, "a claim nobody has counted");
  assert.equal(isUntrusted({ ...refuses, steamEmpty: null }), true, "no vouch");

  // A claim is not a count: the sort does not reward one.
  assert.equal(trustedPlayers({ ...refuses, verifiedPlayers: null }), 0);
  assert.equal(trustedPlayers({ ...honest, verdict: "synthetic", verifiedPlayers: null }), 0);
  assert.equal(trustedPlayers({ ...honest, verdict: null, verifiedPlayers: null }), 20, "a row no check has reached counts its claim until one does");

  // The queue keyword is believed only from a server that is (nearly) full by its trusted count.
  const tagged = (over) => ({ ...honest, ...over, tags: { ...honest.tags, queue: over.queue } });
  assert.equal(queueOf(tagged({ queue: 172, verifiedPlayers: 7 })), 0, "a 7-player server's +172");
  assert.equal(queueOf(tagged({ queue: 12, verifiedPlayers: 58 })), 12);
  assert.equal(queueOf(tagged({ queue: 12, players: 60, verifiedPlayers: null, verdict: "unverifiable" })), 0, "a faker claiming 60/60");
});

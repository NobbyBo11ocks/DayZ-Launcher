// Row 28, the final check of the whole app: the store fixes it made (src/lib/state/
// servers.svelte.ts). The host's half has its tests in cache.rs, tags.rs and verify.rs.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, freshApp, row, settle } from "./harness.mjs";

async function started(t) {
  const app = await freshApp(t);
  const { servers } = await app.load("state/servers.svelte");
  await servers.start();
  await settle();
  return { app, servers };
}

function deliver(app, servers, rows, kind = "batch") {
  app.mock.send({ kind, data: rows });
  servers.flushRows();
}

/** A verification as the host sends one on the row stream. */
const verified = (id, over = {}) => ({ id, verdict: "verified", reported: 10, verified: 10, maxPlayers: 60, pingMs: 30, playerRttMs: 31, tags: null, verifiedAt: 1_790_000_000, reason: "INFO 10 vs PLAYER 10", ...over });

test("a check that moves a server to another map recounts the Map dropdown (row 28)", async (t) => {
  const { app, servers } = await started(t);
  const a = row({ map: "chernarusplus" });
  const b = row({ map: "chernarusplus" });
  deliver(app, servers, [a, b]);
  await advance(t, 400);
  assert.deepEqual(servers.maps.map(([id, , n]) => [id, n]), [["chernarusplus", 2]]);
  const facts = { name: a.name, map: "enoch", version: "", serverVersion: 0, gamePort: null, password: false, bots: 0 };
  app.mock.send({ kind: "verified", data: [verified(a.id, { facts })] });
  await advance(t, 400);
  assert.deepEqual(
    servers.maps.map(([id, , n]) => [id, n]).sort(),
    [["chernarusplus", 1], ["enoch", 1]],
  );
});

test("the 'dropped out of the server list' line goes when the next join opens (row 28)", async (t) => {
  const { app, servers } = await started(t);
  const gone = row({ name: "Gone Server" });
  const other = row();
  deliver(app, servers, [gone, other]);
  servers.requestJoin(gone.id);
  app.mock.send({ kind: "pruned", data: [gone.id] });
  servers.flushRows();
  await settle();
  assert.equal(servers.joiningId, null);
  assert.match(servers.error ?? "", /Gone Server dropped out of the server list/);
  servers.requestJoin(other.id);
  assert.equal(servers.error, null, "the player joined again, as it said to");
});

test("unknown versions and times sort last whichever way the column sorts (row 28)", async (t) => {
  const { app, servers } = await started(t);
  const known = row({ serverVersion: 129_163_709, version: "1.29.163709", tags: { timeMinutes: 600 } });
  const older = row({ serverVersion: 128_161_464, version: "1.28.161464", tags: { timeMinutes: 60 } });
  const unknown = row({ serverVersion: 0, version: "", tags: { timeMinutes: null } });
  deliver(app, servers, [unknown, known, older]);
  await advance(t, 400);
  const ids = () => servers.list.map((r) => r.id);
  servers.setSort("version");
  assert.deepEqual(ids(), [older.id, known.id, unknown.id], "ascending");
  servers.setSort("version");
  assert.deepEqual(ids(), [known.id, older.id, unknown.id], "descending");
  servers.setSort("time");
  assert.deepEqual(ids(), [older.id, known.id, unknown.id], "time, ascending");
  servers.setSort("time");
  assert.deepEqual(ids(), [known.id, older.id, unknown.id], "time, descending");
});

test("a DZSA row keeps the tags it has no field for, as the cache does (row 28)", async (t) => {
  const { app, servers } = await started(t);
  const steam = row({ tags: { dlc: true, allowedFilePatching: true, queue: 3, nightMultiplier: 6.5, timeMinutes: 600, firstPersonOnly: true } });
  deliver(app, servers, [steam]);
  deliver(app, servers, [{ ...steam, steamEmpty: null, pingMs: 0, tags: { battleye: true, firstPersonOnly: false, privateHive: false, modded: false, dlc: false, allowedFilePatching: false, timeMinutes: 900 } }], "dzsa-batch");
  const r = servers.rows.get(steam.id);
  assert.equal(r.tags.dlc, true);
  assert.equal(r.tags.allowedFilePatching, true);
  assert.equal(r.tags.queue, 3);
  assert.equal(r.tags.nightMultiplier, 6.5);
  // DZSA's word on what it does know.
  assert.equal(r.tags.timeMinutes, 900);
  assert.equal(r.tags.firstPersonOnly, false);
});

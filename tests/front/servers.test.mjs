// The servers store (src/lib/state/servers.svelte.ts): what reaches the list, the shared
// error line, the counts, and the join dialog's guards. The automatic refresh has its own
// file (refresh.test.mjs), the saved filters theirs (filters.test.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { advance, deferred, done, freshApp, row, settle, steam } from "./harness.mjs";

async function started(t, setup) {
  const app = await freshApp(t);
  setup?.(app.mock);
  const { servers } = await app.load("state/servers.svelte");
  await servers.start();
  await settle();
  return { app, servers };
}

/** Rows as a refresh delivers them, merged at once instead of after the 350 ms pool. */
function deliver(app, servers, rows, kind = "batch") {
  app.mock.send({ kind, data: rows });
  servers.flushRows();
}

test("one bad row-stream message does not stop the ones after it (D-302 (4), row 14 F2)", async (t) => {
  const { app, servers } = await started(t);
  // A message the store cannot take: a verification list that is not a list.
  app.mock.send({ kind: "verified", data: null });
  const after = row({ name: "Sent after the bad one" });
  deliver(app, servers, [after]);
  app.mock.send({ kind: "net", data: false });
  assert.deepEqual(app.mock.uncaught, [], "the handler kept its throw to itself");
  assert.ok(servers.rows.has(after.id), "the next batch reached the list");
  assert.equal(servers.netDown, true, "and so did the message after that");
  assert.ok(
    app.mock.logs.some((l) => l.level === "error" && l.target === "rows" && l.message.startsWith("a verified message failed")),
    "the bad message is in the log",
  );
});

test("the shared error line comes down with the next success of the action that put it up, and only that one (D-302 (9), row 14 F10)", async (t) => {
  const app = await freshApp(t);
  const { servers } = await app.load("state/servers.svelte");
  const favErr = "Windows could not write the file.";
  const histErr = "The recent joins could not be read.";
  app.mock.fail("favourite_set", favErr);
  await servers.toggleFavourite("198.51.100.7:27016");
  assert.equal(servers.error, favErr);
  assert.equal(servers.favourites.has("198.51.100.7:27016"), false, "the star went back");

  app.mock.handle("history_list", []);
  await servers.loadHistory();
  assert.equal(servers.error, favErr, "another action's success leaves it");

  app.mock.handle("favourite_set", null);
  await servers.toggleFavourite("198.51.100.8:27016");
  assert.equal(servers.error, null, "the next star that saves takes it down");

  // A newer line from another action outlives the older action's success.
  app.mock.fail("favourite_set", favErr);
  await servers.toggleFavourite("198.51.100.9:27016");
  app.mock.fail("history_list", histErr);
  await servers.loadHistory();
  await servers.toggleFavourite("198.51.100.9:27016");
  assert.equal(servers.error, histErr);
  await servers.loadHistory();
  assert.equal(servers.error, null);
});

test('"Refreshing… N listed" counts each server Steam listed in this refresh once, and nothing else (D-306 (7), row 16)', async (t) => {
  const { app, servers } = await started(t, (m) => m.handle("servers_refresh", true));
  await servers.refresh(true, true);
  assert.equal(servers.refreshListed, 0);

  const a = row({ steamEmpty: false });
  const b = row({ steamEmpty: true, players: 0 });
  deliver(app, servers, [a, b]);
  // Rows arriving meanwhile that Steam did not list — the DZSA list's, a probe's, the
  // favourites import's — carry no Steam flag and are not its answer.
  deliver(app, servers, [row({ steamEmpty: null }), row({ steamEmpty: null })], "dzsa-batch");
  deliver(app, servers, [row({ steamEmpty: undefined })]);
  assert.equal(servers.refreshListed, 2, "empty servers count too");

  deliver(app, servers, [a, row({ steamEmpty: false })]);
  assert.equal(servers.refreshListed, 3, "a server listed twice counts once");

  // A second press declined as busy, and a DZSA import finishing, do not end the count.
  app.mock.send(done({ rejected: true, reason: "busy", complete: false }));
  app.mock.send(done({ source: "dzsa", complete: false }));
  deliver(app, servers, [row({ steamEmpty: false })]);
  assert.equal(servers.refreshListed, 4);

  // Steam's own end of the refresh does.
  app.mock.send(done());
  deliver(app, servers, [row({ steamEmpty: false })]);
  assert.equal(servers.refreshListed, 4);
});

test("the connection notice stays up until both the host and Windows say the connection is back (D-307 (5), D-303 (9))", async (t) => {
  const app = await freshApp(t, { online: false });
  const { servers } = await app.load("state/servers.svelte");
  await servers.start();
  assert.equal(servers.netDown, true, "started with Windows offline");
  app.fire("online");
  assert.equal(servers.netDown, false);

  app.mock.send({ kind: "net", data: false });
  assert.equal(servers.netDown, true, "the checks find no connection");
  // Wi-Fi reconnects: Windows says online while the host, which reports only changes, has not.
  app.fire("online");
  assert.equal(servers.netDown, true, "Windows' online does not clear what the host still finds");
  app.mock.send({ kind: "net", data: true });
  assert.equal(servers.netDown, false);

  app.fire("offline");
  assert.equal(servers.netDown, true, "Windows' offline alone raises it");
  app.fire("online");
  assert.equal(servers.netDown, false);
});

test("an import with no official file leaves 'could not be read' up; one that read the favourites takes it down (D-307 (4), D-303 (3))", async (t) => {
  const { app, servers } = await started(t, (m) => m.fail("favourites_list", "database is locked"));
  assert.equal(servers.favouritesUnread, true);

  app.mock.reply("import_official_favourites", { total: 0, imported: 0, already: 0, unreachable: 0, path: "x", missing: true });
  const none = await servers.importOfficial();
  assert.equal(none.missing, true);
  assert.equal(servers.favouritesUnread, true, "nothing was read, so they still could not be read");
  assert.equal(app.mock.count("favourites_list"), 1, "and nothing was read again");

  app.mock.handle("favourites_list", [
    { id: "198.51.100.1:27016", addedAt: 1 },
    { id: "198.51.100.2:27016", addedAt: 2 },
  ]);
  app.mock.reply("import_official_favourites", { total: 2, imported: 2, already: 0, unreachable: 0, path: "x" });
  await servers.importOfficial();
  assert.equal(servers.favouritesUnread, false);
  assert.deepEqual([...servers.favourites], ["198.51.100.1:27016", "198.51.100.2:27016"]);

  // A failed import is the asking page's to show, not the line every list page shares.
  app.mock.fail("import_official_favourites", "Windows could not open FavouriteServers.xml");
  assert.equal(await servers.importOfficial(), "Windows could not open FavouriteServers.xml");
  assert.equal(servers.error, null);
});

test("'not scanned yet' counts the servers a scan will read, and asks the mod maps only about those (D-276, D-310)", async (t) => {
  const { app, servers } = await started(t);
  const modded = { modded: true };
  const counts = [
    row({ tags: modded, steamEmpty: false, players: 12 }), // Steam lists it populated, not counted yet
    row({ tags: modded, verifiedPlayers: 5, verdict: "verified" }),
  ];
  const scanned = row({ tags: modded, verifiedPlayers: 7, verdict: "verified" });
  const waiting = row({ tags: modded, verifiedPlayers: 7, verdict: "verified" }); // its last read failed
  const skipped = [
    row({ tags: modded, steamEmpty: true, players: 40 }), // R0
    row({ tags: modded, steamEmpty: null, players: 30 }), // unverified, Steam never called it populated
    row({ tags: modded, verifiedPlayers: 0, verdict: "verified" }),
    row({ verifiedPlayers: 20, verdict: "verified" }), // vanilla
  ];
  deliver(app, servers, [...counts, scanned, waiting, ...skipped]);
  app.mock.send({ kind: "mods", data: [[{ id: scanned.id, mods: [1559212036] }], [[1559212036, "CF"]], [waiting.id]] });
  assert.equal(servers.unscannedModded, 2);

  // A modded farm: thousands of rows no scan will read.
  const farm = Array.from({ length: 2000 }, (_, i) => row({ ip: `203.0.113.${(i % 200) + 1}`, queryPort: 30000 + i, tags: modded, steamEmpty: i % 2 ? true : null, players: 60 }));
  deliver(app, servers, farm);
  let asks = 0;
  for (const m of [servers.modsByServer, servers.modsUnreadable]) {
    const has = m.has.bind(m);
    m.has = (k) => (asks++, has(k));
  }
  servers.rowsChanged();
  assert.equal(servers.unscannedModded, 2);
  assert.ok(asks <= 8, `the reactive maps were asked ${asks} times for 4 rows that can count`);
});

test("the start-up status read never overwrites a newer status event (D-281 (10))", async (t) => {
  const app = await freshApp(t);
  const snapshot = deferred();
  app.mock.handle("steam_status", () => snapshot.promise);
  app.mock.handle("servers_refresh", true);
  const { servers } = await app.load("state/servers.svelte");
  const starting = servers.start();
  await settle();
  // The worker's one-time "session open" lands while the snapshot read is out.
  app.mock.emit("steam:status", steam());
  snapshot.resolve(steam({ initialized: false }));
  await starting;
  assert.equal(servers.steam.initialized, true, "Refresh stays enabled");
});

test("a Join never replaces an open join dialog, and a prune that takes its server closes it and says why (D-295 F8, D-296 (5))", async (t) => {
  const { app, servers } = await started(t);
  const alpha = row({ name: "Alpha" });
  const bravo = row({ name: "Bravo" });
  const other = row({ name: "Other" });
  deliver(app, servers, [alpha, bravo, other]);

  servers.requestJoin(alpha.id);
  servers.requestJoin(bravo.id);
  assert.equal(servers.joiningId, alpha.id, "Enter behind the dialog did not replace it");

  app.mock.send({ kind: "pruned", data: [alpha.id] });
  assert.equal(servers.joiningId, null);
  assert.equal(servers.error, "Alpha dropped out of the server list, so its join was closed. Join it again from Recent or with Direct connect.");

  servers.requestJoin(bravo.id);
  app.mock.send({ kind: "pruned", data: [other.id] });
  assert.equal(servers.joiningId, bravo.id, "a prune of other rows leaves it open");
});

test("a row whose check cannot count players is asked again after ten minutes, not two, and nothing is asked while Windows is offline (D-281 (3), D-302 (3))", async (t) => {
  const { app, servers } = await started(t, (m) => m.handle("servers_verify", { total: 1, verified: 0, inflated: 0, unverifiable: 0, synthetic: 0, offline: 1, elapsedMs: 1 }));
  const secs = () => Math.floor(Date.now() / 1000);
  const silent = row({ steamEmpty: null, verdict: "offline" });
  const counted = row({ verdict: "verified", verifiedPlayers: 10, verifiedAt: secs() - 60 });
  deliver(app, servers, [silent, counted]);
  const asked = () => app.mock.argsOf("servers_verify").at(-1)?.ids ?? [];

  await servers.verifyVisible([silent.id, counted.id]);
  assert.deepEqual(asked(), [silent.id], "never checked; the other was counted a minute ago");
  // Its check answers: still silent, so no count and no `verifiedAt`.
  app.mock.send({ kind: "verified", data: [{ id: silent.id, verdict: "offline", reported: 0, verified: null, maxPlayers: 60, pingMs: null, tags: null, verifiedAt: secs(), reason: "no answer" }] });

  await advance(t, 3 * 60_000, 60_000);
  await servers.verifyVisible([silent.id, counted.id]);
  assert.deepEqual(asked(), [counted.id], "three minutes on: the counted row is stale, the silent one is not");

  await advance(t, 8 * 60_000, 60_000);
  await servers.verifyVisible([silent.id]);
  assert.deepEqual(asked(), [silent.id], "eleven minutes after its check");

  app.env.online = false;
  await advance(t, 11 * 60_000, 60_000);
  const before = app.mock.count("servers_verify");
  await servers.verifyVisible([silent.id, counted.id]);
  assert.equal(app.mock.count("servers_verify"), before, "no network, nothing asked");
});

test("mod lists that arrive while the stored ones are read are applied after that older snapshot (D-281 (8))", async (t) => {
  const { app, servers } = await started(t);
  const x = row({ tags: { modded: true } });
  deliver(app, servers, [x]);
  const snapshot = deferred();
  app.mock.handle("mods_index", () => snapshot.promise);
  const reading = servers.loadModsIndex();
  await settle();
  // A scan batch lands while the read is out: x runs CF and Expansion now.
  app.mock.send({ kind: "mods", data: [[{ id: x.id, mods: [1559212036, 2116157322] }], [[1559212036, "CF"], [2116157322, "Expansion"]], []] });
  // The snapshot was taken before it: CF only.
  snapshot.resolve({ catalog: [{ id: 1559212036, name: "CF", servers: 1 }], index: [{ id: x.id, mods: [1559212036] }], unreadable: [] });
  await reading;
  assert.deepEqual(servers.modsByServer.get(x.id), [1559212036, 2116157322]);
  assert.equal(servers.modCatalog.get(2116157322)?.servers, 1);
  assert.equal(servers.modCatalog.get(1559212036)?.servers, 1);
  assert.equal(servers.modsIndexLoaded, true);
});

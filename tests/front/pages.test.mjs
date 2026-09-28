// The other pages' store logic (row 23 of docs/13): Favourites, Recent, LAN and Friends —
// what has been read, favourite writes, a check's facts on the row, the LAN scan, the join
// paths' probes — and the fixes before it that had no test (D-209, D-222, D-235, D-240,
// D-256, D-295). The pages themselves read these fields; src/lib/state/servers.svelte.ts
// holds the logic.
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

function deliver(app, servers, rows, kind = "batch") {
  app.mock.send({ kind, data: rows });
  servers.flushRows();
}

/** A verification as the host sends one on the row stream. */
function verified(id, over = {}) {
  return { id, verdict: "verified", reported: 10, verified: 10, maxPlayers: 60, pingMs: 30, playerRttMs: 31, tags: null, verifiedAt: 1_790_000_000, reason: "INFO 10 vs PLAYER 10", ...over };
}

test("favourites are read beside the list, and the pages know what has been read (row 23)", async (t) => {
  const app = await freshApp(t);
  const list = deferred();
  app.mock.handle("servers_cached", () => list.promise);
  app.mock.handle("favourites_list", [{ id: "198.51.100.3:27016" }]);
  const { servers } = await app.load("state/servers.svelte");
  const start = servers.start();
  await settle();
  assert.equal(servers.favouritesLoaded, true, "not behind the whole list");
  assert.equal(servers.favourites.has("198.51.100.3:27016"), true);
  assert.equal(servers.listLoaded, false, "the list is still coming");
  assert.equal(servers.historyLoaded, false);
  list.resolve({ keys: [], tagKeys: [], rows: [], lastRefresh: null });
  await start;
  assert.equal(servers.listLoaded, true);
  // A list that could not be read is done too: the pages stop waiting for it.
  const app2 = await freshApp(t, { timers: false });
  app2.mock.fail("servers_cached", "database is locked");
  const s2 = (await app2.load("state/servers.svelte")).servers;
  await s2.start();
  assert.equal(s2.listLoaded, true);
});

test("favourites that could not be read at start are read again, and a star made meanwhile stays (row 23)", async (t) => {
  const { app, servers } = await started(t, (m) => m.fail("favourites_list", "database is locked"));
  assert.equal(servers.favouritesUnread, true);
  assert.equal(servers.favouritesLoaded, false);
  // A star while the rest are unread, and one still on its way when the read lands.
  app.mock.handle("favourite_set", null);
  await servers.toggleFavourite("198.51.100.9:27016");
  const held = deferred();
  app.mock.reply("favourite_set", () => held.promise);
  const inFlight = servers.toggleFavourite("198.51.100.10:27016");
  app.mock.handle("favourites_list", [{ id: "198.51.100.1:27016" }, { id: "198.51.100.9:27016" }]);
  await servers.retryFavourites();
  assert.equal(servers.favouritesUnread, false);
  assert.deepEqual([...servers.favourites].sort(), ["198.51.100.1:27016", "198.51.100.10:27016", "198.51.100.9:27016"].sort());
  held.resolve(null);
  await inFlight;
  assert.equal(servers.favourites.has("198.51.100.10:27016"), true);
  // Once read, nothing more is asked.
  const reads = app.mock.count("favourites_list");
  await servers.retryFavourites();
  assert.equal(app.mock.count("favourites_list"), reads);
});

test("favourite writes go one at a time and the last press wins (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const id = "198.51.100.4:27016";
  const first = deferred();
  app.mock.reply("favourite_set", () => first.promise);
  app.mock.handle("favourite_set", null);
  const a = servers.toggleFavourite(id);
  const b = servers.toggleFavourite(id);
  await settle();
  assert.equal(app.mock.count("favourite_set"), 1, "the second waits for the first");
  assert.equal(servers.favourites.has(id), false, "the screen shows the last press");
  first.resolve(null);
  await Promise.all([a, b]);
  assert.deepEqual(app.mock.argsOf("favourite_set"), [{ id, on: true }, { id, on: false }], "in the order pressed");
  // A failed write behind later presses does not undo the last one: star, unstar, star
  // with the first write failing ends starred, as the last two writes leave the cache.
  const failing = deferred();
  app.mock.reply("favourite_set", () => failing.promise);
  const c = servers.toggleFavourite(id);
  const d = servers.toggleFavourite(id);
  const e = servers.toggleFavourite(id);
  failing.reject("Windows could not write the file.");
  await Promise.all([c, d, e]);
  assert.equal(servers.favourites.has(id), true, "the last star stands");
});

test("a check's INFO reaches the row: a rename, a new version, a moved game port (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const old = row({ name: "Zulu", version: "1.29.163709", serverVersion: 129163709, gamePort: 2302 });
  const other = row({ name: "Mike" });
  deliver(app, servers, [old, other]);
  app.mock.handle("favourite_set", null);
  await servers.toggleFavourite(old.id);
  await servers.toggleFavourite(other.id);
  servers.setSort("name");
  await advance(t, 400);
  assert.deepEqual(servers.favouriteRows.map((r) => r.name), ["Mike", "Zulu"]);
  const facts = { name: "Alpha", map: "enoch", version: "1.30.100000", serverVersion: 130100000, gamePort: 2402, password: true, bots: 0 };
  app.mock.send({ kind: "verified", data: [verified(old.id, { facts })] });
  await advance(t, 400);
  const r = servers.rows.get(old.id);
  assert.deepEqual([r.name, r.map, r.version, r.serverVersion, r.gamePort, r.password], ["Alpha", "enoch", "1.30.100000", 130100000, 2402, true]);
  assert.deepEqual(servers.favouriteRows.map((x) => x.name), ["Alpha", "Mike"], "the name sort follows the rename");
  // With the whole list ranked by name — over 500 rows sorted, as on Servers — a short
  // list takes its order from that ranking, which a rename has to rebuild.
  const many = Array.from({ length: 520 }, (_, i) => row({ name: `Filler ${String(i).padStart(3, "0")}` }));
  deliver(app, servers, many);
  await advance(t, 400);
  assert.equal(servers.list.length > 500, true);
  const renamed = { ...facts, name: "Zebra" };
  app.mock.send({ kind: "verified", data: [verified(old.id, { facts: renamed })] });
  await advance(t, 400);
  assert.equal(servers.list.length > 500, true, "the whole list sorted again");
  assert.deepEqual(servers.favouriteRows.map((x) => x.name), ["Mike", "Zebra"], "the ranking follows the rename");
  // Without facts, or with empty ones, nothing moves.
  app.mock.send({ kind: "verified", data: [verified(old.id), verified(old.id, { facts: { ...facts, name: "", map: "", version: "", serverVersion: 0, gamePort: null } })] });
  await advance(t, 400);
  const k = servers.rows.get(old.id);
  assert.deepEqual([k.name, k.map, k.version, k.gamePort], ["Zebra", "enoch", "1.30.100000", 2402]);
});

test("a LAN scan is the LAN page's: the Servers count, what it found, a refused scan (row 23)", async (t) => {
  const { app, servers } = await started(t);
  app.mock.handle("servers_refresh", true);
  app.mock.emit("steam:status", steam());
  await settle();
  // The automatic refresh the session started, finished.
  app.mock.send(done());
  await settle();
  servers.verifying = false;
  servers.refreshListed = 36430;
  assert.equal(await servers.refreshLan(), true);
  assert.equal(servers.lanScanning, true);
  assert.equal(servers.refreshListed, 0, "not the last refresh's count");
  // A server behind a VPN's LAN adapter answers the scan from a public-looking address.
  const hamachi = row({ ip: "26.14.3.9", queryPort: 27016 });
  deliver(app, servers, [{ ...hamachi, steamEmpty: null }]);
  app.mock.send(done({ source: "lan", responded: 1 }));
  await advance(t, 400);
  assert.equal(servers.lanScanning, false);
  assert.deepEqual(servers.lanRows.map((r) => r.id), [hamachi.id]);
  assert.equal(servers.lanTotal, 1);
  // A refused scan leaves the Servers page's line, indicator and automatic refresh alone.
  servers.verifying = true;
  await servers.refreshLan();
  app.mock.send(done({ source: "lan", rejected: true, reason: "no-session", complete: false }));
  await settle();
  assert.equal(servers.lanScanning, false);
  assert.equal(servers.error, null);
  assert.equal(servers.verifying, true);
  // So does a refusal because a refresh is running.
  await servers.refreshLan();
  app.mock.send(done({ source: "lan", rejected: true, reason: "busy", complete: false }));
  await settle();
  assert.equal(servers.lanScanning, false);
  assert.equal(servers.error, null);
});

test("Join again takes only a server on the entry's game port, in one probe (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const h = { id: "198.51.100.5:27016", joinedAt: 1, name: "Old haunt", ip: "198.51.100.5", gamePort: 2302, mods: 0 };
  // The host now answers with the server the entry joined.
  app.mock.reply("direct_connect", ({ address, expectGamePort }) => row({ ip: "198.51.100.5", queryPort: 27016, gamePort: expectGamePort ?? 2402, id: address }));
  assert.equal(await servers.rejoin(h), null);
  assert.deepEqual(app.mock.argsOf("direct_connect"), [{ address: h.id, expectGamePort: 2302 }]);
  assert.equal(servers.joiningId, h.id);
  servers.joiningId = null;
  // A reply on another game port is another server: no join opens.
  const other = row({ ip: "198.51.100.6", queryPort: 27016, gamePort: 2402 });
  const h2 = { ...h, id: other.id, ip: other.ip };
  app.mock.reply("direct_connect", other);
  assert.match(await servers.rejoin(h2), /did not answer on 198\.51\.100\.6:2302/);
  assert.equal(servers.joiningId, null);
  assert.equal(app.mock.count("direct_connect"), 2, "one probe per join");
  // The listed row, while it is still this entry's server, opens without a probe.
  const listed = row({ ip: "198.51.100.8", queryPort: 27016, gamePort: 2302, verdict: "verified" });
  deliver(app, servers, [listed]);
  assert.equal(await servers.rejoin({ ...h, id: listed.id, ip: listed.ip }), null);
  assert.equal(servers.joiningId, listed.id);
  assert.equal(app.mock.count("direct_connect"), 2);
});

test("a friend's server: Steam's query port as it is, a game address with its port checked, the shared line untouched (row 23)", async (t) => {
  const { app, servers } = await started(t);
  app.mock.handle("direct_connect", ({ address }) => row({ id: address, ip: address.split(":")[0], queryPort: Number(address.split(":")[1]), gamePort: 2302 }));
  assert.equal(await servers.joinFriend({ ip: "198.51.100.20", gamePort: 2302, queryPort: 0 }), null);
  servers.joiningId = null;
  assert.equal(await servers.joinFriend({ ip: "198.51.100.21", gamePort: 2302, queryPort: 27016 }), null);
  servers.joiningId = null;
  assert.deepEqual(app.mock.argsOf("direct_connect"), [
    { address: "198.51.100.20:2302", expectGamePort: 2302 },
    { address: "198.51.100.21:27016", expectGamePort: null },
  ]);
  app.mock.fail("direct_connect", "No DayZ server answered at 198.51.100.22:2302. Check the address, or the server may be offline.");
  assert.match(await servers.joinFriend({ ip: "198.51.100.22", gamePort: 2302, queryPort: 0 }), /No DayZ server answered/);
  assert.equal(servers.error, null, "the page says it; the shared line does not");
});

test("a Direct connect failure stays until a probe succeeds, and never wipes another action's line (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const msg = "No DayZ server answered at 203.0.113.9:2302. Check the address, or the server may be offline.";
  app.mock.fail("direct_connect", msg);
  assert.equal(await servers.directConnect("203.0.113.9:2302", false), null);
  assert.equal(servers.error, msg);
  app.mock.handle("history_list", []);
  await servers.loadHistory();
  assert.equal(servers.error, msg, "another action's success leaves it");
  app.mock.handle("direct_connect", ({ address }) => row({ id: address }));
  await servers.directConnect("203.0.113.9:27016", false);
  assert.equal(servers.error, null);
  // A refresh's failure is not the probe's to clear.
  servers.fail("refresh", "Steam did not answer the refresh, so the list was not updated. Press Refresh to try again.");
  await servers.directConnect("203.0.113.10:27016", false);
  assert.match(servers.error, /Steam did not answer the refresh/);
});

test("Recent shows a new join without being reopened, once it has been read (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const a = { id: "198.51.100.30:27016", joinedAt: 1, name: "A", ip: "198.51.100.30", gamePort: 2302, mods: 0 };
  const b = { ...a, id: "198.51.100.31:27016", joinedAt: 2, name: "B", ip: "198.51.100.31" };
  servers.noteJoined();
  await settle();
  assert.equal(app.mock.count("history_list"), 0, "not read yet: the page reads it when it opens");
  app.mock.reply("history_list", [a], [b, a]);
  await servers.loadHistory();
  assert.equal(servers.historyLoaded, true);
  servers.noteJoined();
  await settle();
  assert.equal(servers.history[0].name, "B");
});

test("the avatar's initial is a whole character (row 23)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { initialOf } = await app.load("avatar");
  assert.equal(initialOf("😀Bob"), "😀");
  assert.equal(initialOf("🇬🇧 Dave"), "🇬🇧");
  assert.equal(initialOf("bob"), "B");
  assert.equal(initialOf(""), "");
});

test("a name sort of a short list does not rank the whole list, and gives the same order (row 23)", async (t) => {
  const { app, servers } = await started(t);
  const rows = ["delta", "Alpha", "charlie", "Bravo", "alpha"].map((name) => row({ name }));
  deliver(app, servers, rows);
  app.mock.handle("favourite_set", null);
  for (const r of rows) await servers.toggleFavourite(r.id);
  servers.setSort("name");
  await advance(t, 400);
  const byCollator = [...rows].sort((a, b) => new Intl.Collator(undefined, { numeric: true, sensitivity: "base" }).compare(a.name, b.name) || (a.id < b.id ? -1 : 1));
  assert.deepEqual(servers.favouriteRows.map((r) => r.id), byCollator.map((r) => r.id));
  servers.setSort("name");
  await advance(t, 400);
  assert.deepEqual(servers.favouriteRows.map((r) => r.id), byCollator.map((r) => r.id).reverse(), "descending flips the tie-break too");
});

test("the friends list is kept while the session is released, and goes with Steam at once (row 23, approved)", async (t) => {
  const { app, servers } = await started(t);
  app.mock.handle("servers_refresh", true);
  app.mock.emit("steam:status", steam());
  const ada = { steamId: "1", name: "Ada", state: "online", inDayz: true };
  app.mock.handle("friends_list", [ada]);
  await servers.pollFriends();
  assert.deepEqual(servers.friendsList, [ada]);
  const at = servers.friendsAt;
  assert.equal(typeof at, "number");
  // Released: the poll does not ask, and the list and its time stay for the page.
  app.mock.emit("steam:status", steam({ idle: true }));
  const asked = app.mock.count("friends_list");
  await servers.pollFriends();
  assert.equal(app.mock.count("friends_list"), asked);
  assert.deepEqual(servers.friendsList, [ada]);
  assert.equal(servers.friendsAt, at);
  // The player's Refresh reads it anyway.
  await servers.loadFriends();
  assert.equal(app.mock.count("friends_list"), asked + 1);
  // Steam gone: cleared by the status itself, not a minute later.
  app.mock.emit("steam:status", steam({ initialized: false }));
  assert.equal(servers.friendsList, null);
  assert.equal(servers.friendsAt, null);
  assert.equal(servers.friendsInDayz, null);
});

test("the listed row for a friend: by query port, or the one answering row at a game address (row 23, approved)", async (t) => {
  const { app, servers } = await started(t);
  const ip = "198.51.100.70";
  const one = row({ ip, queryPort: 27016, gamePort: 2302, verdict: "verified" });
  const twinA = row({ ip, queryPort: 27017, gamePort: 2402, verdict: "verified" });
  const twinB = row({ ip, queryPort: 27018, gamePort: 2402, verdict: "verified" });
  const down = row({ ip, queryPort: 27019, gamePort: 2502, verdict: "offline" });
  deliver(app, servers, [one, twinA, twinB, down]);
  assert.equal(servers.friendRow({ ip, gamePort: 2302, queryPort: 27016 })?.id, one.id);
  assert.equal(servers.friendRow({ ip, gamePort: 2302, queryPort: 0 })?.id, one.id, "by game address");
  assert.equal(servers.friendRow({ ip, gamePort: 2402, queryPort: 0 }), null, "two ids: cannot tell");
  assert.equal(servers.friendRow({ ip, gamePort: 2502, queryPort: 0 }), null, "offline: not live");
  assert.equal(servers.friendRow({ ip, gamePort: 2302, queryPort: 27099 }), null, "Steam's query port is taken as it is");
});

test("Favourites count before the search, and Recent reads its named limit (row 23, approved)", async (t) => {
  const { app, servers } = await started(t);
  const a = row({ name: "Namalsk Survival" });
  const b = row({ name: "Chernarus Hardcore" });
  deliver(app, servers, [a, b]);
  app.mock.handle("favourite_set", null);
  await servers.toggleFavourite(a.id);
  await servers.toggleFavourite(b.id);
  await servers.toggleFavourite("198.51.100.99:27016"); // a favourite the list holds no row for
  servers.filters.search = "namalsk";
  await advance(t, 400);
  assert.equal(servers.favouriteRows.length, 1);
  assert.equal(servers.favouriteTotal, 2);
  const { HISTORY_LIMIT } = await app.load("state/servers.svelte");
  assert.equal(HISTORY_LIMIT, 100);
  app.mock.handle("history_list", []);
  await servers.loadHistory();
  assert.deepEqual(app.mock.argsOf("history_list").at(-1), { limit: HISTORY_LIMIT });
});

// ---- Fixes before row 23 that had no test ----

test("the friends count and markers go when Steam goes, and stay while the session is released (D-222)", async (t) => {
  const { app, servers } = await started(t);
  const on = row({ ip: "198.51.100.40", queryPort: 27016, gamePort: 2302 });
  deliver(app, servers, [on]);
  app.mock.handle("servers_refresh", true);
  app.mock.emit("steam:status", steam());
  app.mock.handle("friends_list", [{ steamId: "1", name: "Ada", state: "online", inDayz: true, server: { ip: on.ip, gamePort: 2302, queryPort: 27016 } }]);
  await servers.pollFriends();
  assert.equal(servers.friendsInDayz, 1);
  assert.deepEqual(servers.friendsOn.get(on.id), ["Ada"]);
  app.mock.emit("steam:status", steam({ idle: true }));
  await servers.pollFriends();
  assert.equal(servers.friendsInDayz, 1, "released on purpose: kept");
  // Through the poll itself: a status read at start (`steam_status`) comes without an
  // event, and only the poll sees it.
  servers.steam = steam({ initialized: false });
  await servers.pollFriends();
  assert.equal(servers.friendsInDayz, null);
  assert.equal(servers.friendsOn.size, 0);
});

test("rows at one game address: answering first, then the newest count, and the friend marker takes the best (D-295 F2)", async (t) => {
  const { app, servers } = await started(t);
  const ip = "198.51.100.50";
  const offline = row({ ip, queryPort: 27016, gamePort: 2302, verdict: "offline", verifiedAt: 300 });
  const older = row({ ip, queryPort: 27017, gamePort: 2302, verdict: "verified", verifiedAt: 100 });
  const newer = row({ ip, queryPort: 27018, gamePort: 2302, verdict: "verified", verifiedAt: 200 });
  const elsewhere = row({ ip, queryPort: 27019, gamePort: 2402 });
  deliver(app, servers, [offline, older, newer, elsewhere]);
  assert.deepEqual(servers.rowsAtGameAddress(ip, 2302).map((r) => r.id), [newer.id, older.id, offline.id]);
  app.mock.handle("servers_refresh", true);
  app.mock.emit("steam:status", steam());
  app.mock.handle("friends_list", [{ steamId: "1", name: "Ada", state: "online", inDayz: true, server: { ip, gamePort: 2302, queryPort: 0 } }]);
  await servers.pollFriends();
  assert.deepEqual([...servers.friendsOn.keys()], [newer.id]);
});

test("the LAN address ranges, on their edges (D-256)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { isLanIp } = await app.load("state/servers.svelte");
  const yes = ["10.0.0.1", "127.0.0.1", "172.16.0.1", "172.31.255.1", "192.168.1.2", "169.254.3.4"];
  const no = ["172.15.0.1", "172.32.0.1", "172.160.0.1", "192.169.0.1", "169.253.0.1", "100.64.0.1", "8.8.8.8", "fe80::1"];
  for (const ip of yes) assert.equal(isLanIp(ip), true, ip);
  for (const ip of no) assert.equal(isLanIp(ip), false, ip);
});

test("a Direct connect keeps the verdict and count the list had (D-256)", async (t) => {
  const { app, servers } = await started(t);
  const known = row({ verdict: "synthetic", verifiedPlayers: null, players: 60 });
  deliver(app, servers, [known]);
  app.mock.handle("direct_connect", () => ({ ...known, verdict: null, verifiedPlayers: null, steamEmpty: null, players: 61 }));
  await servers.directConnect(known.id, false);
  assert.equal(servers.rows.get(known.id).verdict, "synthetic");
});

test("the LAN total counts before the search, and Favourites and LAN follow the list's sort (D-240, D-209)", async (t) => {
  const { app, servers } = await started(t);
  const a = row({ ip: "192.168.1.10", name: "Basement", players: 3 });
  const b = row({ ip: "192.168.1.11", name: "Attic", players: 7 });
  deliver(app, servers, [a, b]);
  servers.filters.search = "attic";
  await advance(t, 400);
  assert.equal(servers.lanTotal, 2);
  assert.deepEqual(servers.lanRows.map((r) => r.id), [b.id]);
  servers.filters.search = "";
  servers.setSort("players");
  await advance(t, 400);
  const order = servers.lanRows.map((r) => r.id);
  servers.setSort("players");
  await advance(t, 400);
  assert.deepEqual(servers.lanRows.map((r) => r.id), order.reverse(), "the arrow moves the rows");
});

test("a pruned row takes its friend marker with it (D-235)", async (t) => {
  const { app, servers } = await started(t);
  const on = row({ ip: "198.51.100.60", queryPort: 27016, gamePort: 2302 });
  deliver(app, servers, [on]);
  app.mock.handle("servers_refresh", true);
  app.mock.emit("steam:status", steam());
  app.mock.handle("friends_list", [{ steamId: "1", name: "Ada", state: "online", inDayz: true, server: { ip: on.ip, gamePort: 2302, queryPort: 27016 } }]);
  await servers.pollFriends();
  assert.equal(servers.friendsOn.has(on.id), true);
  app.mock.send({ kind: "pruned", data: [on.id] });
  assert.equal(servers.friendsOn.has(on.id), false);
});

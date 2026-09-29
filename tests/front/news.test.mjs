// The News page's feed (src/lib/state/news.svelte.ts): a fetch that failed offline.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, deferred, freshApp, NOW, settle } from "./harness.mjs";

const OFFLINE = "The news could not be loaded: no connection to Steam. It is tried again when the connection is back.";

test("a failed news fetch is tried again when the connection or the window comes back, at most once a minute (D-302 (11), row 14 F6)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("news_cached", { items: [] });
  app.mock.fail("news_fetch", OFFLINE, { always: true });
  const { news } = await app.load("state/news.svelte");
  await news.start();
  const fetches = () => app.mock.count("news_fetch");
  assert.equal(fetches(), 1);
  assert.equal(news.error, OFFLINE);

  app.fire("focus"); // the player comes back to the window, still offline
  await settle();
  assert.equal(fetches(), 2);
  await advance(t, 10_000);
  app.fire("online");
  await settle();
  assert.equal(fetches(), 2, "not twice inside a minute");

  await advance(t, 50_000);
  app.mock.handle("news_fetch", { items: [{ gid: "1", title: "Update 1.29.2", url: "https://example.invalid/", author: "Bohemia", feed: "Community Announcements", official: true, date: 1_790_000_000, summary: "", update: true }] });
  app.fire("online");
  await settle();
  assert.equal(fetches(), 3, "the connection is back");
  assert.equal(news.error, null);
  assert.equal(news.items.length, 1);

  await advance(t, 120_000);
  app.fire("focus");
  await settle();
  assert.equal(fetches(), 3, "nothing to retry once it worked");
  news.stop();
});

/** Settings as the host hands them out, with the News mark at `newsSeen`. */
function settings(newsSeen, over = {}) {
  return {
    profileName: "",
    extraArgs: "",
    skipIntro: true,
    noSplash: true,
    noPause: false,
    launchProfiles: [],
    steamIdleMinutes: 5,
    logging: true,
    logMuted: [],
    ui: { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen, news: true, lastSeenVersion: "", openOn: "news" },
    ...over,
  };
}
const post = (gid, date, over = {}) => ({ gid, title: `Post ${gid}`, url: `https://example.invalid/${gid}`, author: "Bohemia", feed: "Community Announcements", official: true, date, summary: "", update: true, ...over });
const SEEN = Math.floor(NOW / 1000) - 5 * 86_400;

/** Window attention and Windows notifications answered, the window unfocused. */
function notifications(mock) {
  mock.handle("plugin:window|request_user_attention", null);
  mock.handle("plugin:window|is_focused", false);
  mock.handle("plugin:notification|is_permission_granted", true);
  mock.handle("plugin:notification|notify", null);
}

test("several new update posts in one fetch: the newest three, newest last, one flash and one notification (row 24)", async (t) => {
  const app = await freshApp(t);
  notifications(app.mock);
  app.mock.handle("settings_get", settings(SEEN));
  app.mock.handle("news_cached", { items: [post("old", SEEN - 100)] });
  const fresh = [post("u4", SEEN + 400), post("u3", SEEN + 300), post("u2", SEEN + 200), post("u1", SEEN + 100)];
  app.mock.handle("news_fetch", { items: [...fresh, post("old", SEEN - 100)] });
  const { news } = await app.load("state/news.svelte");
  await news.start();
  await settle();
  assert.deepEqual(news.alerts.map((a) => a.gid), ["u2", "u3", "u4"], "the newest last");
  assert.equal(app.mock.count("plugin:window|request_user_attention"), 1);
  assert.deepEqual(app.mock.argsOf("plugin:notification|notify").map((a) => a.options.body), ["Post u4"]);
  news.stop();
});

test("the stored posts are asked for at once, applied after the mark, and the page knows when they are in (row 24)", async (t) => {
  const app = await freshApp(t);
  const read = deferred();
  app.mock.handle("settings_get", () => read.promise);
  app.mock.handle("news_cached", { items: [post("a", SEEN - 10)] });
  app.mock.handle("news_fetch", { items: [post("a", SEEN - 10)] });
  const { news } = await app.load("state/news.svelte");
  const started = news.start();
  await settle();
  assert.equal(app.mock.count("news_cached"), 1, "not behind the settings read");
  assert.equal(news.loaded, false);
  assert.equal(news.items.length, 0, "not before the mark is read (D-245)");
  read.resolve(settings(SEEN));
  await started;
  assert.equal(news.loaded, true);
  assert.equal(news.items.length, 1);
  assert.equal(news.seen, SEEN);
  news.stop();
});

test("switching News off lets go of the list and every picture, and on again reads the stored copy (row 24)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("settings_get", settings(SEEN));
  const withPicture = post("p", SEEN - 10, { image: "https://clan.akamai.steamstatic.com/images/1/x.jpg" });
  app.mock.handle("news_cached", { items: [withPicture] });
  app.mock.handle("news_fetch", { items: [withPicture] });
  app.mock.handle("news_thumb", () => new ArrayBuffer(8));
  const { news } = await app.load("state/news.svelte");
  await news.start();
  assert.equal(news.thumbUrl(withPicture), null, "asked for");
  await settle();
  assert.equal(news.thumbs.size, 1);
  const revoked = [];
  t.mock.method(URL, "revokeObjectURL", (u) => void revoked.push(u));
  news.stop();
  assert.equal(news.items.length, 0);
  assert.equal(news.thumbs.size, 0);
  assert.equal(revoked.length, 1);
  assert.equal(news.loaded, false);
  await news.start();
  assert.equal(news.items.length, 1, "read again");
  news.stop();
});

// ---- Earlier fixes that had no test ----

test("a first run counts the newest five as unread, and writes its mark only over a file that was read (D-194, D-256)", async (t) => {
  const posts = Array.from({ length: 8 }, (_, i) => post(`f${i}`, SEEN - i * 1000));
  for (const readable of [true, false]) {
    const app = await freshApp(t, { timers: false });
    app.mock.handle("settings_get", readable ? settings(0) : settings(0, { unreadable: true }));
    app.mock.handle("news_cached", { items: [] });
    app.mock.handle("news_fetch", { items: posts });
    const { news } = await app.load("state/news.svelte");
    await news.start();
    await settle();
    assert.equal(news.unread, 5, readable ? "read" : "unreadable");
    const { uiPrefs } = await app.load("state/uiprefs.svelte");
    await uiPrefs.flush();
    const writes = app.mock.argsOf("ui_prefs_set").filter((a) => a.patch?.newsSeen != null);
    assert.equal(writes.length, readable ? 1 : 0, readable ? "the mark written" : "no mark over a file that was not read");
    news.stop();
  }
});

test("switched off during a fetch: no toast, no flash, no notification (D-185, D-256)", async (t) => {
  const app = await freshApp(t);
  notifications(app.mock);
  app.mock.handle("settings_get", settings(SEEN));
  app.mock.handle("news_cached", { items: [] });
  const held = deferred();
  app.mock.handle("news_fetch", () => held.promise);
  const { news } = await app.load("state/news.svelte");
  const started = news.start();
  await settle();
  news.stop();
  held.resolve({ items: [post("late", SEEN + 50)] });
  await started;
  await settle();
  assert.deepEqual(news.alerts, []);
  assert.equal(app.mock.count("plugin:window|request_user_attention"), 0);
  assert.equal(app.mock.count("plugin:notification|notify"), 0);
});

test("off and on during the first fetch arms one half-hour refresh, not two (D-240)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("settings_get", settings(SEEN));
  app.mock.handle("news_cached", { items: [] });
  const first = deferred();
  app.mock.reply("news_fetch", () => first.promise);
  app.mock.handle("news_fetch", { items: [post("a", SEEN - 10)] });
  const { news } = await app.load("state/news.svelte");
  const a = news.start();
  await settle();
  news.stop();
  const b = news.start();
  await settle();
  // The first fetch answers ten minutes later: a second timer, armed then, would fire
  // ten minutes after the first one and fetch again.
  await advance(t, 10 * 60_000);
  first.resolve({ items: [post("a", SEEN - 10)] });
  await Promise.all([a, b]);
  const before = app.mock.count("news_fetch");
  await advance(t, 30 * 60_000, 60_000);
  assert.equal(app.mock.count("news_fetch"), before + 1, "one timer");
  news.stop();
});

test("the New boundary is frozen for a visit while the mark moves on (D-240, D-245)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("settings_get", settings(SEEN));
  const newer = post("n", SEEN + 100, { update: false });
  app.mock.handle("news_cached", { items: [newer] });
  app.mock.handle("news_fetch", { items: [newer] });
  const { news } = await app.load("state/news.svelte");
  await news.start();
  news.beginVisit();
  await settle();
  assert.equal(news.visitSeen, SEEN);
  news.markSeen();
  assert.equal(news.seen, SEEN + 100, "looked at");
  assert.equal(news.visitSeen, SEEN, "still New during this visit");
  news.endVisit();
  news.beginVisit();
  assert.equal(news.visitSeen, SEEN + 100, "the next visit starts past it");
  news.stop();
});

test("no alert for an update post older than two weeks, and the badge counts update posts only (row 24, approved)", async (t) => {
  const app = await freshApp(t);
  notifications(app.mock);
  const stale = Math.floor(NOW / 1000) - 60 * 86_400; // News back on after two months
  app.mock.handle("settings_get", settings(stale));
  app.mock.handle("news_cached", { items: [] });
  const oldUpdate = post("old", stale + 86_400); // 59 days old
  const newUpdate = post("new", Math.floor(NOW / 1000) - 3_600);
  const sale = post("sale", Math.floor(NOW / 1000) - 7_200, { update: false, title: "Sale" });
  app.mock.handle("news_fetch", { items: [newUpdate, sale, oldUpdate] });
  const { news } = await app.load("state/news.svelte");
  await news.start();
  await settle();
  assert.deepEqual(news.alerts.map((a) => a.gid), ["new"], "the weeks-old post is not toasted");
  assert.equal(app.mock.count("plugin:notification|notify"), 1);
  assert.equal(news.unread, 2, "the two update posts; the sale keeps its New pill only");
  news.stop();
});

test("a picture that could not be made says so to the page, which then shows the post as text (row 24, approved)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("settings_get", settings(SEEN));
  const withPicture = post("p", SEEN - 10, { image: "https://clan.akamai.steamstatic.com/images/1/x.jpg" });
  app.mock.handle("news_cached", { items: [withPicture] });
  app.mock.handle("news_fetch", { items: [withPicture] });
  app.mock.fail("news_thumb", "picture request failed");
  const { news } = await app.load("state/news.svelte");
  await news.start();
  assert.equal(news.thumbFailed(withPicture), false);
  news.thumbUrl(withPicture);
  await settle();
  assert.equal(news.thumbFailed(withPicture), true);
  assert.equal(news.thumbFailed(withPicture, true), false, "per size");
  news.stop();
});

test("the News grid's rows come out full, maximised or not (D-326)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { newsGrid, CARDS_MAX } = await app.load("newsgrid");
  // Maximised on 1920 px (1624 px of grid): eight update cards were five and three.
  assert.deepEqual(newsGrid(1624, 8), { cols: 4, count: 8 });
  assert.deepEqual(newsGrid(1624, 12), { cols: 4, count: 12 });
  // Nine cannot come out even within the card widths: the fewest gaps, the most columns.
  assert.deepEqual(newsGrid(1624, 9), { cols: 5, count: 9 });
  // The window's usual size holds three a row; eight leave one gap there, not a column of giants.
  assert.deepEqual(newsGrid(1010, 8), { cols: 3, count: 8 });
  // "All news": more posts than the grid shows, so whole rows of the most columns.
  assert.deepEqual(newsGrid(1624, 59), { cols: 5, count: 25 });
  assert.deepEqual(newsGrid(1010, 59), { cols: 3, count: 24 });
  assert.equal(CARDS_MAX, 24);
  // Before the grid has a width: the style sheet's own columns.
  assert.deepEqual(newsGrid(0, 8), { cols: 0, count: 8 });
  assert.deepEqual(newsGrid(1624, 0).count, 0);
});

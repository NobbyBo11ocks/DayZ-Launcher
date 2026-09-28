// Theme, News and the start page (src/lib/state/prefs.svelte.ts): the page's own copy for
// the first frame, the settings file once read, and what goes back to the file.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, deferred, freshApp, settle } from "./harness.mjs";

const NEWS_KEY = "dayz-launcher.news.v1";
const OPEN_KEY = "dayz-launcher.open-on.v1";
const UI = { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen: 0, news: true, lastSeenVersion: "", openOn: "news" };
const settings = (extra = {}, ui = {}) => ({ profileName: "", extraArgs: "", skipIntro: true, noSplash: true, noPause: false, launchProfiles: [], steamIdleMinutes: 5, logging: true, logMuted: [], ...extra, ui: { ...UI, ...ui } });

test("the start page and News follow the settings file once it is read, and the page's start-up copy follows the file (D-306 (5), D-304 (6) F3)", async (t) => {
  const app = await freshApp(t, { storage: { [NEWS_KEY]: "off", [OPEN_KEY]: "favourites" } });
  const file = deferred();
  app.mock.handle("settings_get", () => file.promise);
  const { prefs } = await app.load("state/prefs.svelte");
  assert.equal(prefs.news, false, "the first frame, from the page's copy");
  assert.equal(prefs.openOn, "favourites");

  // The installer switched News on and the file says Servers.
  file.resolve(settings({}, { news: true, openOn: "servers" }));
  await settle();
  assert.equal(prefs.news, true);
  assert.equal(prefs.openOn, "servers");
  assert.equal(app.storage.get(NEWS_KEY), "on", "the next start's first frame agrees");
  assert.equal(app.storage.get(OPEN_KEY), "servers");

  prefs.setOpenOn("favourites");
  await advance(t, 150);
  assert.equal(app.storage.get(OPEN_KEY), "favourites");
  assert.equal(app.mock.argsOf("ui_prefs_set").at(-1).patch.openOn, "favourites");
});

test("a damaged settings file set aside gets the page's own News and start-page choice, and its stand-in defaults switch nothing back on (D-304 (6) F1, D-306 (5), D-194)", async (t) => {
  const app = await freshApp(t, { storage: { [NEWS_KEY]: "off", [OPEN_KEY]: "servers" } });
  // Set aside and replaced by the defaults: News on, open on News.
  app.mock.handle("settings_get", settings({ reset: true, keptAs: "settings.json.broken-1790519999" }));
  const { prefs } = await app.load("state/prefs.svelte");
  const { uiPrefs } = await app.load("state/uiprefs.svelte");
  await uiPrefs.ready;
  await settle();
  assert.equal(uiPrefs.readOk, false);
  assert.equal(prefs.news, false, "News stays off");
  assert.equal(prefs.openOn, "servers");

  await advance(t, 150);
  const written = Object.assign({}, ...app.mock.argsOf("ui_prefs_set").map((a) => a.patch));
  assert.equal(written.news, false, "the new file gets News off");
  assert.equal(written.openOn, "servers", "and the start page");
});

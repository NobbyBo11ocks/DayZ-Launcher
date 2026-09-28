// The one writer of the settings file's `ui` section (src/lib/state/uiprefs.svelte.ts):
// theme, filters, News, the start page, the What's new mark. A lost or reordered write
// here is a setting that comes back wrong at the next start.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, deferred, freshApp, settle } from "./harness.mjs";

const UI = { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen: 0, news: true, lastSeenVersion: "", openOn: "news" };
const settings = (extra = {}, ui = {}) => ({ profileName: "", extraArgs: "", skipIntro: true, noSplash: true, noPause: false, launchProfiles: [], steamIdleMinutes: 5, logging: true, logMuted: [], ...extra, ui: { ...UI, ...ui } });

test("preference writes go one at a time, so a failed older write cannot land over a newer one (D-307 (3), row 17)", async (t) => {
  const app = await freshApp(t);
  // The host: one settings mutex, and a first write held by a scanner, then refused.
  let file = { ...UI, filters: { map: "F0" } };
  let lock = Promise.resolve();
  let n = 0;
  const held = deferred();
  app.mock.handle("settings_get", settings({}, file));
  app.mock.handle("ui_prefs_set", ({ patch }) => {
    const k = ++n;
    const run = lock.then(async () => {
      if (k === 1) await held.promise;
      file = { ...file, ...patch };
      return { ...file };
    });
    lock = run.catch(() => {});
    return run;
  });
  const { uiPrefs } = await app.load("state/uiprefs.svelte");
  await uiPrefs.ready;

  uiPrefs.patch({ filters: { map: "F1" } });
  await advance(t, 150); // the first write is out
  await advance(t, 100);
  uiPrefs.patch({ filters: { map: "F2" } }); // the player moves on while it is held
  await advance(t, 150);
  held.reject(new Error("Access is denied. (os error 5)"));
  await settle();
  await advance(t, 10_000, 1_000); // past the retry
  assert.deepEqual(file.filters, { map: "F2" }, "the file ends with what the screen shows");
  assert.deepEqual(uiPrefs.current.filters, { map: "F2" });
});

test("an answer standing in for an unreadable or late settings file is not a read: changes made before it and after it reach the file (D-302 (12) H4, D-304 (7) F4)", async (t) => {
  const app = await freshApp(t);
  const file = deferred();
  app.mock.handle("settings_get", () => file.promise);
  const { uiPrefs } = await app.load("state/uiprefs.svelte");

  uiPrefs.patch({ theme: "light" }); // before the file was read
  await advance(t, 150); // its write waits for the read
  assert.equal(app.mock.count("ui_prefs_set"), 0);
  // The host cannot read the file and stands in with the defaults.
  file.resolve(settings({ unreadable: true }));
  const copy = await uiPrefs.ready;
  assert.equal(copy.theme, "light", "the copy the stores reconcile against has the change");
  assert.equal(uiPrefs.readOk, false);
  assert.deepEqual(uiPrefs.health, { unreadable: true, reset: false, keptAs: null });
  await settle();
  assert.deepEqual(app.mock.argsOf("ui_prefs_set")[0], { patch: { theme: "light" } });

  // News switched back on: the same as the default standing in, not as the file.
  uiPrefs.patch({ news: true });
  await advance(t, 150);
  assert.deepEqual(app.mock.argsOf("ui_prefs_set")[1], { patch: { news: true } });
});

test("a write that failed is tried again every five seconds, six times, then with the next change (D-302 (12) H9, row 14)", async (t) => {
  const app = await freshApp(t);
  app.mock.fail("ui_prefs_set", new Error("The process cannot access the file because it is being used by another process."), { always: true });
  const { uiPrefs } = await app.load("state/uiprefs.svelte");
  await uiPrefs.ready;
  const writes = () => app.mock.count("ui_prefs_set");

  uiPrefs.patch({ news: false });
  await advance(t, 150);
  assert.equal(writes(), 1);
  await advance(t, 5_000);
  assert.equal(writes(), 2, "five seconds on, without another change");
  await advance(t, 60_000, 1_000);
  assert.equal(writes(), 7, "six timed retries");
  await advance(t, 60_000, 5_000);
  assert.equal(writes(), 7, "then it waits for the next change");

  app.mock.handle("ui_prefs_set", ({ patch }) => ({ ...UI, ...patch }));
  uiPrefs.patch({ theme: "light" });
  await advance(t, 150);
  assert.deepEqual(app.mock.argsOf("ui_prefs_set").at(-1), { patch: { news: false, theme: "light" } }, "the lone change went with it");
});

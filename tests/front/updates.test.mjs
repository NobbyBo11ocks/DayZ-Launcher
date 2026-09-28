// The in-app updater (src/lib/state/updates.svelte.ts): the check at start and on focus,
// "Check for updates", and "Install and restart". The plugin is a mock whose `check`
// answers with harness.mjs's `fakeUpdate`, a download the test drives by hand.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, fakeUpdate, freshApp, NOW, settle } from "./harness.mjs";

const UI = { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen: 0, news: true, lastSeenVersion: "", openOn: "news" };
const settings = (ui = {}) => ({ profileName: "", extraArgs: "", skipIntro: true, noSplash: true, noPause: false, launchProfiles: [], steamIdleMinutes: 5, logging: true, logMuted: [], ui: { ...UI, ...ui } });
const OFFLINE = "error sending request for url (https://github.com/NobbyBo11ocks/DayZ-Launcher/releases/latest/download/latest.json)";

test("a download that stops moving is given up after a minute, its late data is neither shown nor installed, and failures say what to do (D-302 (7) F5, D-306 (18), D-281 (5))", async (t) => {
  const app = await freshApp(t);
  let u = null;
  app.mock.handle("plugin:updater|check", () => (u = fakeUpdate("0.1.88")));
  const { updates } = await app.load("state/updates.svelte");
  await updates.checkNow();
  assert.equal(updates.state, "available");

  const installing = updates.install();
  await settle();
  assert.equal(updates.state, "downloading");
  const checks = app.mock.count("plugin:updater|check");
  void updates.install(); // a second click
  await settle();
  assert.equal(app.mock.count("plugin:updater|check"), checks, "a second Install while one runs is ignored");
  assert.equal(u.downloads, 1);

  const stuck = u;
  stuck.onEvent({ event: "Started", data: { contentLength: 1000 } });
  stuck.onEvent({ event: "Progress", data: { chunkLength: 300 } });
  assert.equal(updates.progress, 30);
  await advance(t, 55_000, 5_000);
  assert.equal(updates.state, "downloading", "under a minute without data");
  await advance(t, 10_000, 5_000);
  assert.equal(updates.state, "available", "Install is offered again");
  assert.equal(updates.error, "Could not download 0.1.88: nothing arrived for a minute. Press Install and restart to try again.");
  assert.equal(updates.progress, 0);
  await installing;

  // The abandoned request runs on until its own limit; what it brings is ignored.
  stuck.onEvent({ event: "Progress", data: { chunkLength: 700 } });
  assert.equal(updates.progress, 0, "its late progress is not shown");
  stuck.finish();
  await settle();
  assert.equal(stuck.installs, 0, "and it is never installed");

  // Failures in words; the request's own text goes to the log.
  const offline = updates.install();
  await settle();
  u.failDownload(OFFLINE);
  await offline;
  assert.equal(updates.error, "Could not download 0.1.88: no connection to GitHub. Press Install and restart to try again.");
  const other = updates.install();
  await settle();
  u.failDownload("signature verification failed");
  await other;
  assert.equal(updates.error, "Could not install 0.1.88. Press Install and restart to try again; the details are on the Logs page.");
  assert.equal(updates.state, "available");
  assert.ok(app.mock.logs.some((l) => l.target === "update" && l.message.includes("signature verification failed")));
});

test("the update check: a clock set back does not hold it off, a failed check says why in words and holds nothing off, and none runs over a download (D-304 (13) F5, D-303 (2), D-302 (7) F13, D-281 (5))", async (t) => {
  const app = await freshApp(t);
  // The last check was stamped by a clock six hours ahead, since put right.
  app.mock.handle("settings_get", settings({ lastUpdateCheckMs: NOW + 6 * 3600_000 }));
  app.mock.reply("plugin:updater|check", null);
  const { updates } = await app.load("state/updates.svelte");
  const checks = () => app.mock.count("plugin:updater|check");

  await updates.autoCheck(); // the start-up check
  assert.equal(checks(), 1, "a time later than now does not hold the start's check off");
  assert.equal(updates.state, "none");

  // "Check for updates" with no connection.
  app.mock.fail("plugin:updater|check", OFFLINE);
  await updates.checkNow();
  assert.equal(updates.state, "error");
  assert.equal(updates.error, "Could not reach GitHub to check for updates. Check your connection and try again.");
  assert.ok(app.mock.logs.some((l) => l.target === "update" && l.message.includes("error sending request")), "the request's own text is in the log");

  // An hour later the window comes back into focus: the check runs from that error.
  await advance(t, 3600_000 + 1_000, 600_000);
  app.mock.reply("plugin:updater|check", fakeUpdate("0.1.88"));
  updates.focusCheck();
  await settle();
  assert.equal(checks(), 3, "a failed check holds no later one off");
  assert.equal(updates.state, "available");
  assert.equal(updates.error, null);

  // Installing: the start-up check asked again (a re-run effect, D-281) finds a download.
  app.mock.handle("plugin:updater|check", () => fakeUpdate("0.1.88"));
  void updates.install();
  await settle();
  assert.equal(updates.state, "downloading");
  const during = checks();
  await updates.autoCheck();
  updates.focusCheck();
  await settle();
  assert.equal(checks(), during, "no check while the download runs");
  assert.equal(updates.state, "downloading", "and Install is not put back beside it");
});

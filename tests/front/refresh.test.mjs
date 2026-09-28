// The automatic refresh (servers.svelte.ts `maybeAutoRefresh`, `refresh`, `#onRefreshDone`):
// when a refresh the player did not press runs, runs again, and never runs. Each of these
// once left the list stale for a session, asked a throttling master server back to back,
// or opened the 24 MB DZSA fallback for a Steam that was there.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, done, freshApp, settle, steam } from "./harness.mjs";

async function started(t, setup) {
  const app = await freshApp(t);
  app.mock.handle("servers_refresh", true);
  setup?.(app.mock);
  const { servers } = await app.load("state/servers.svelte");
  await servers.start();
  await settle();
  return { app, servers, refreshes: () => app.mock.count("servers_refresh") };
}

async function status(app, s) {
  app.mock.emit("steam:status", s);
  await settle();
}

test("a refresh Steam could not take runs again at its next usable status, never while the session is released, three times a session at most (D-302 (5), D-236)", async (t) => {
  const { app, servers, refreshes } = await started(t);
  await status(app, steam());
  assert.equal(refreshes(), 1, "the start-up refresh");
  assert.deepEqual(app.mock.argsOf("servers_refresh")[0], { force: false, full: false });

  // Steam closes under it: the host answers that there was no session.
  app.mock.send(done({ rejected: true, reason: "no-session", complete: false }));
  assert.equal(servers.error, "Steam did not answer the refresh, so the list was not updated. Press Refresh to try again.");
  await status(app, steam({ initialized: false, error: "Steam is not running" }));
  assert.equal(refreshes(), 1, "nothing while Steam is gone");
  await status(app, steam({ idle: true }));
  assert.equal(refreshes(), 1, "nor while the session is released");
  await status(app, steam());
  assert.equal(refreshes(), 2, "Steam is back: the list is refreshed");

  for (let i = 0; i < 3; i++) {
    app.mock.send(done({ rejected: true, reason: "no-session", complete: false }));
    await status(app, steam());
  }
  assert.equal(refreshes(), 4, "three re-arms a session, not a fourth");
  assert.equal(app.mock.count("servers_dzsa"), 0, "and never the DZSA list, which is for a Steam that never started");
});

test("a refresh Steam left unanswered is asked again after five minutes, not at the status that follows (D-307 (1), row 17)", async (t) => {
  const { app, refreshes } = await started(t);
  await status(app, steam());
  assert.equal(refreshes(), 1);

  app.mock.send(done({ rejected: true, reason: "no-answer", complete: false }));
  // The worker's `refreshing: false` status follows the rejection straight away.
  await status(app, steam());
  assert.equal(refreshes(), 1, "not back to back against a throttling master server");
  await advance(t, 5 * 60_000 - 1_000, 30_000);
  await status(app, steam());
  assert.equal(refreshes(), 1, "nor at any status inside the five minutes");
  await advance(t, 1_000);
  assert.equal(refreshes(), 2, "five minutes on");
});

test("the start-up refresh is spent only once one has run: a throttled one is tried again, a press declined as busy brings nothing back (D-307 (1), D-222)", async (t) => {
  const { app, refreshes } = await started(t, (m) => m.reply("servers_refresh", false));
  // Started inside the worker's 60 s throttle (the updater's relaunch): declined.
  await status(app, steam());
  assert.equal(refreshes(), 1);
  await advance(t, 62_000, 1_000);
  assert.equal(refreshes(), 2, "tried again once the throttle is over");

  // The player presses Refresh while that one runs; the worker declines it as busy.
  const { servers } = await app.load("state/servers.svelte");
  app.mock.reply("servers_refresh", false);
  await servers.refresh(true, true);
  assert.equal(refreshes(), 3);

  await status(app, steam());
  await advance(t, 2 * 60_000, 10_000);
  await status(app, steam());
  assert.equal(refreshes(), 3, "no second automatic refresh after the player's");
  await status(app, steam({ initialized: false, error: "Steam is not running" }));
  assert.equal(app.mock.count("servers_dzsa"), 0, "and no DZSA fallback when Steam leaves later");
});

// Row 27, the app shell: the close-time work every way of closing the window waits for,
// links through the host, and Escape's order between the pages and the notices. The host's
// half (the close handler, the hand-over to a running launcher, the window's fit) has its
// tests in lib.rs and commands.rs.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, freshApp, settle } from "./harness.mjs";

test("a close waits for the page's unsaved work, then tells the host (row 27)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("close_ready", null);
  const { beforeClose, answerCloseRequests } = await app.load("state/closing");
  const order = [];
  beforeClose(async () => {
    await new Promise((r) => setTimeout(r, 100));
    order.push("saved");
  });
  const off = answerCloseRequests();
  await settle();
  app.mock.emit("app:closing");
  await settle();
  assert.equal(app.mock.count("close_ready"), 0, "not before the work is done");
  await advance(t, 150);
  assert.deepEqual(order, ["saved"]);
  assert.equal(app.mock.count("close_ready"), 1);
  off();
});

test("a flush survives a callback that throws at once, and one that never ends (row 27)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("close_ready", null);
  const { beforeClose, flushBeforeClose, answerCloseRequests } = await app.load("state/closing");
  const off = beforeClose(() => {
    throw new Error("before its first await");
  });
  let flushed = false;
  void flushBeforeClose().then(() => (flushed = true));
  await settle();
  assert.equal(flushed, true, "the throw is one failed callback, not a failed flush");
  off();
  beforeClose(() => new Promise(() => {}));
  answerCloseRequests();
  await settle();
  app.mock.emit("app:closing");
  await advance(t, 1_200);
  assert.equal(app.mock.count("close_ready"), 1, "within the page's time, whatever a callback does");
});

test("Escape goes to the notices only when nothing else took it (row 27)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { noticesTakeEscape } = await app.load("escape");
  const key = (over = {}) => ({ key: "Escape", defaultPrevented: false, target: { tagName: "DIV" }, ...over });
  assert.equal(noticesTakeEscape(key(), false), true);
  assert.equal(noticesTakeEscape(key({ defaultPrevented: true }), false), false, "the grid or a page closed the details pane with it");
  assert.equal(noticesTakeEscape(key(), true), false, "a modal keeps its Escape");
  assert.equal(noticesTakeEscape(key({ target: { tagName: "INPUT" } }), false), false, "a field keeps its Escape");
  assert.equal(noticesTakeEscape(key({ key: "Enter" }), false), false);
});

test("links go through the host, and one that cannot be opened says so (row 27)", async (t) => {
  const app = await freshApp(t, { timers: false });
  app.mock.handle("open_link", null);
  const { openExternal } = await app.load("external");
  const workshop = "https://steamcommunity.com/sharedfiles/filedetails/?id=1559212036";
  assert.equal(await openExternal(workshop), true);
  assert.deepEqual(app.mock.argsOf("open_link"), [{ url: workshop }]);
  app.mock.fail("open_link", "The page could not be opened in your browser. The details are on the Logs page.");
  assert.equal(await openExternal("https://example.com/"), false);
});

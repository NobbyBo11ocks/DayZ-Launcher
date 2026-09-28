// The page's half of the log (src/lib/log.ts): every thrown thing becomes a line the host
// accepts, and lines carry the page's own time.
import test from "node:test";
import assert from "node:assert/strict";
import { freshApp, settle } from "./harness.mjs";

test("anything thrown becomes a line, undefined included, at the page's own time (row 25)", async (t) => {
  const app = await freshApp(t, { timers: false });
  app.mock.handle("log_ui", null);
  const { describe, logError } = await app.load("log");
  assert.equal(describe(undefined), "undefined", "a rejection without a reason");
  assert.equal(describe(() => 1).length > 0, true, "a function");
  assert.equal(describe("plain"), "plain");
  assert.equal(describe(new TypeError("x")), "TypeError: x");
  const before = Date.now();
  logError("promise", describe(undefined));
  await settle();
  const sent = app.mock.logs.at(-1);
  assert.equal(typeof sent.message, "string", "the host refuses a line without a message");
  assert.equal(sent.at >= before && sent.at <= Date.now(), true, "stamped by the page");
});

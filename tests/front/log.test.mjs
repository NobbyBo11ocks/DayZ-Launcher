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

test("Copy's report: a header with what a fix needs, then the lines oldest first at UTC times (row 25)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { formatReport, stamp } = await app.load("logreport");
  assert.equal(stamp(Date.UTC(2026, 8, 22, 12, 34, 56, 789)), "2026-09-22 12:34:56.789", "the file's own stamp");
  const lines = [
    { at: Date.UTC(2026, 8, 22, 12, 0, 2), level: "error", target: "launch", message: "DayZ could not be started: boom" },
    { at: Date.UTC(2026, 8, 22, 12, 0, 1), level: "info", target: "app", message: "start v0.1.96" },
  ];
  const text = formatReport(lines, { version: "0.1.96", windows: "10.0.26200.6584", webview: "140.0.3485.54", elevated: false });
  assert.deepEqual(text.split("\n"), [
    "DZSA CrayZ Launcher 0.1.96 · Windows 10.0.26200.6584 · WebView2 140.0.3485.54 · not as administrator · times UTC",
    "2026-09-22 12:00:01.000 INFO  app start v0.1.96",
    "2026-09-22 12:00:02.000 ERROR launch DayZ could not be started: boom",
  ]);
  assert.match(formatReport([], null).split("\n")[0], /^DZSA CrayZ Launcher \? · Windows \? · WebView2 \? ·/, "facts unknown");
});

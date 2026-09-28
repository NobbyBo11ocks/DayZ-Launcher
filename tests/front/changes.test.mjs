// "What's new" after an update (src/lib/changes.ts): which releases' notes a player moving
// from one version to another is shown. Written against CHANGES as it stands, so a release
// that adds its notes (tools/check_changes.js) never has to touch this file.
import test from "node:test";
import assert from "node:assert/strict";
import { freshApp } from "./harness.mjs";

test("What's new shows the releases since the one last seen, newest first, five at most, and nothing going back (D-301)", async (t) => {
  const app = await freshApp(t, { timers: false });
  const { CHANGES, compareVersions, unseenChanges } = await app.load("changes");
  const sign = (a, b) => Math.sign(compareVersions(a, b));
  assert.equal(sign("0.1.9", "0.1.10"), -1, "by number, not by text");
  assert.equal(sign("0.1.10", "0.1.9"), 1);
  assert.equal(sign("0.2", "0.1.99"), 1, "a missing part is 0");
  assert.equal(sign("1.0.0", "0.99.99"), 1);
  assert.equal(sign("0.1.87", "0.1.87-harness"), 0, "anything after a dash is ignored");
  assert.equal(sign("0.1.87-rc.2", "0.1.87"), 0, "dots after the dash included");

  const versions = CHANGES.map((r) => r.version);
  const [newest, second, third] = versions;
  const ids = (list) => list.map((r) => r.version);
  assert.deepEqual(ids(unseenChanges(third, newest)), [newest, second]);
  assert.deepEqual(ids(unseenChanges(newest, newest)), [], "the same version");
  assert.deepEqual(ids(unseenChanges(newest, versions.at(-1))), [], "going back to an older version");
  assert.deepEqual(ids(unseenChanges("", newest)), [newest], "a file from before the notes: this release's only");
  assert.deepEqual(ids(unseenChanges("0.0.1", newest)), versions.slice(0, 5), "several updates behind: the newest five");
  // Every release with notes is 0.1.78 or later: past "0.1.9" by number, before it as text.
  assert.deepEqual(ids(unseenChanges("0.1.9", newest)), versions.slice(0, 5));
  assert.deepEqual(ids(unseenChanges(second, `${newest}-dev`)), [newest], "a build of the newest counts as it");
});

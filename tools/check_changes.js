// Checks the "What's new" notes in src/lib/changes.ts (D-301), in CI and before a
// release: the version in package.json has an entry, entries run newest first with no
// version twice, and every line is short and in a DayZ player's words, not the
// launcher's own.
//   node tools/check_changes.js
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const { version } = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
// Node runs the TypeScript file as it is: it holds data and two small functions, and
// nothing in it needs more than type stripping.
const { CHANGES, compareVersions } = await import(pathToFileURL(resolve(root, "src/lib/changes.ts")).href);

const KINDS = new Set(["added", "fixed", "changed", "removed"]);
/** Two lines at most in the window's 520 px card. */
const MAX_CHARS = 80;
const MAX_NOTES = 5;
/** Words a player should never have to read in the notes: the stack, the machinery and
 *  the project's own bookkeeping. */
const JARGON =
  /\b(IPC|API|JSON|WebView\d*|SQLite|Tauri|Svelte|Rust|NSIS|A2S|renderer|backend|frontend|front end|endpoint|payload|junctions?|registry|mutex|async|runtime|heap|regex|cache|refactor\w*|commit|CI|D-\d+|Q\d+|AI|Claude)\b/i;
/** Code-shaped words: backticks, snake_case and camelCase. */
const CODE = /`|\b[a-z]+_[a-z_]+\b|\b[a-z]+[A-Z]\w*\b/;

const errors = [];
if (!CHANGES.some((r) => r.version === version)) errors.push(`${version} (package.json) has no entry`);
const seen = new Set();
CHANGES.forEach((r, i) => {
  if (seen.has(r.version)) errors.push(`${r.version} is listed twice`);
  seen.add(r.version);
  const prev = CHANGES[i - 1];
  if (prev && compareVersions(prev.version, r.version) <= 0) errors.push(`${r.version} comes after ${prev.version}: newest first`);
  if (r.notes.length === 0 || r.notes.length > MAX_NOTES) errors.push(`${r.version} has ${r.notes.length} notes (1–${MAX_NOTES})`);
  for (const n of r.notes) {
    const where = `${r.version} "${n.text}"`;
    if (!KINDS.has(n.kind)) errors.push(`${where}: kind "${n.kind}" is not one of ${[...KINDS].join(", ")}`);
    if (n.text.length > MAX_CHARS) errors.push(`${where}: ${n.text.length} characters (at most ${MAX_CHARS})`);
    if (!/^[A-Z0-9"']/.test(n.text)) errors.push(`${where}: must start with a capital letter`);
    const jargon = n.text.match(JARGON) ?? n.text.match(CODE);
    if (jargon) errors.push(`${where}: "${jargon[0]}" is not a word players use`);
  }
});

if (errors.length) {
  console.error(`change notes: ${errors.length} problem${errors.length === 1 ? "" : "s"}`);
  for (const e of errors) console.error(`  ${e}`);
  process.exit(1);
}
console.log(`change notes: ok (${CHANGES.length} release${CHANGES.length === 1 ? "" : "s"}, ${version} included)`);

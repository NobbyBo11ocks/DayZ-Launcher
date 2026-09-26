// Checks that src-tauri/nsis/installer.nsi is Tauri's stock template for the installed
// @tauri-apps/cli version plus exactly the changes the file marks as its own.
//
// Each deviation is wrapped in the template itself:
//
//   ; >>> dzl-change: <the upstream line(s) it replaces, backslash-n between them>
//   ; why we changed it
//   <our lines>
//   ; <<< dzl-change
//
// so the check is simply "put every upstream line back and see whether the result is
// upstream". Nothing here carries a second copy of what we wrote — the earlier version
// did, and keeping a twelve-line comment in step across two files is a job nobody will
// remember to do (D-205).
//
// Run after every CLI upgrade; `--write` refreshes the file from the new tag with the
// marked blocks re-applied, so the diff can be reviewed.
// Usage: node tools/nsis_template_check.js [--write]
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const localPath = path.join(root, "src-tauri", "nsis", "installer.nsi");
const MARKER = "; --- end of DayZ Launcher header; everything below is upstream ---\n";
const OPEN = "; >>> dzl-change: ";
const CLOSE = "; <<< dzl-change";

// process.exit() right after a fetch trips a libuv assertion on Windows; set exitCode instead.
process.exitCode = await main(process.argv.includes("--write"));

/**
 * Splits a marked body into the blocks we changed and the upstream text each replaces.
 * `restored` is what upstream should look like once ours are put back.
 */
/**
 * The restore check is blind to anything added *inside* a marked block: each block is
 * swapped for the upstream text its anchor carries, so extra lines there simply vanish.
 * That is not hypothetical — a build of 0.1.30 carried `!insertmacro MUI_PAGE_FINISH`
 * twice, once as a stray copy of the upstream line at the top of a block and once
 * where we re-added it, and this checker passed it green. The installer showed the
 * finish page twice. A page inserted twice is the shape that mistake takes, so it gets
 * its own assertion (D-214).
 */
function duplicatePages(body) {
  const seen = new Map();
  for (const line of body.split("\n")) {
    const m = /^\s*!insertmacro\s+(MUI_(?:UN)?PAGE_[A-Z_]+)/.exec(line);
    if (m) seen.set(m[1], (seen.get(m[1]) ?? 0) + 1);
  }
  return [...seen].filter(([, n]) => n > 1);
}
function unmark(body) {
  const lines = body.split("\n");
  const out = [];
  const blocks = [];
  for (let i = 0; i < lines.length; i++) {
    const at = lines[i].indexOf(OPEN);
    if (at < 0) {
      out.push(lines[i]);
      continue;
    }
    // A block may replace more than one upstream line. The sentinel has to fit on one
    // line itself, so it writes those as a literal backslash-n.
    const upstream = lines[i]
      .slice(at + OPEN.length)
      .split(String.raw`\n`)
      .join("\n");
    const start = i;
    while (i < lines.length && !lines[i].includes(CLOSE)) i++;
    if (i >= lines.length) throw new Error(`unclosed ${OPEN.trim()} at line ${start + 1}`);
    blocks.push({ upstream, ours: lines.slice(start, i + 1).join("\n") });
    out.push(upstream);
  }
  return { restored: out.join("\n"), blocks };
}

async function main(write) {
  const cliVersion = JSON.parse(readFileSync(path.join(root, "node_modules/@tauri-apps/cli/package.json"), "utf8")).version;
  const tag = `tauri-cli-v${cliVersion}`;
  const url = `https://raw.githubusercontent.com/tauri-apps/tauri/${tag}/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`;

  const res = await fetch(url);
  if (!res.ok) return fail(`${url} → HTTP ${res.status}`);
  const upstream = (await res.text()).replace(/\r\n/g, "\n");

  const local = readFileSync(localPath, "utf8").replace(/\r\n/g, "\n");
  const cut = local.indexOf(MARKER);
  if (cut < 0) return fail("local template lacks the header marker line");
  const header = local.slice(0, cut + MARKER.length);
  const body = local.slice(cut + MARKER.length).replace(/^\n/, "");

  let restored;
  let blocks;
  try {
    ({ restored, blocks } = unmark(body));
  } catch (e) {
    return fail(String(e?.message ?? e));
  }
  if (blocks.length === 0) return fail("no `; >>> dzl-change:` blocks found; the template claims no deviations");

  const dupes = duplicatePages(body);
  if (dupes.length > 0) {
    return fail(
      "a wizard page is inserted more than once, which the restore check below cannot see: " +
        dupes.map(([name, n]) => `${name} x${n}`).join(", "),
    );
  }

  if (restored === upstream && header.includes(tag)) {
    console.log(`ok: upstream ${tag} + ${blocks.length} marked changes (${body.split("\n").length} lines)`);
    for (const b of blocks) console.log(`     · ${b.upstream.trim().slice(0, 78)}`);
    return 0;
  }
  if (!write) {
    if (!header.includes(tag)) {
      console.error(`mismatch: the header names another tag than ${tag}; run with --write`);
      return 1;
    }
    const a = restored.split("\n");
    const b = upstream.split("\n");
    const at = a.findIndex((l, i) => l !== b[i]);
    console.error(
      `mismatch: with our ${blocks.length} marked changes put back, the body still differs from upstream ` +
        `(first at line ${at + 1}):\n  ours:     ${a[at] ?? "<end of file>"}\n  upstream: ${b[at] ?? "<end of file>"}\n` +
        "run with --write to refresh, then review the diff",
    );
    return 1;
  }

  // Re-apply each marked block to the freshly fetched upstream.
  let rebuilt = upstream;
  for (const b of blocks) {
    if (rebuilt.split(b.upstream).length !== 2) {
      return fail(`upstream no longer has the line this change anchors on: ${b.upstream.trim()}`);
    }
    // A function, not a string: a string replacement expands `$$`, `$&` and friends, and
    // `$$` is how NSIS writes a literal dollar sign (D-279).
    rebuilt = rebuilt.replace(b.upstream, () => b.ours);
  }
  writeFileSync(localPath, header.replace(/tauri-cli-v[\d.]+/g, tag) + "\n" + rebuilt);
  console.log(`refreshed from upstream ${tag} with ${blocks.length} marked changes; review the diff`);
  return 0;
}

function fail(msg) {
  console.error(`nsis_template_check: ${msg}`);
  return 2;
}

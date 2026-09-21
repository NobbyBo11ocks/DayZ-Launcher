// Checks that src-tauri/nsis/installer.nsi is Tauri's stock template for the installed
// @tauri-apps/cli version plus exactly our one change (the per-user default install
// directory, D-067). Run after every CLI upgrade; `--write` refreshes the file from the
// new tag and re-applies the change so the diff can be reviewed.
// Usage: node tools/nsis_template_check.js [--write]
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const localPath = path.join(root, "src-tauri", "nsis", "installer.nsi");
const MARKER = "; --- end of DayZ Launcher header; everything below is upstream ---\n";
const BEFORE = 'StrCpy $INSTDIR "$LOCALAPPDATA\\${PRODUCTNAME}"';
const AFTER = 'StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${PRODUCTNAME}"';

// process.exit() right after a fetch trips a libuv assertion on Windows; set exitCode instead.
process.exitCode = await main(process.argv.includes("--write"));

async function main(write) {
  const cliVersion = JSON.parse(readFileSync(path.join(root, "node_modules/@tauri-apps/cli/package.json"), "utf8")).version;
  const tag = `tauri-cli-v${cliVersion}`;
  const url = `https://raw.githubusercontent.com/tauri-apps/tauri/${tag}/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`;

  const res = await fetch(url);
  if (!res.ok) return fail(`${url} → HTTP ${res.status}`);
  const upstream = (await res.text()).replace(/\r\n/g, "\n");
  if (upstream.split(BEFORE).length !== 2) return fail("upstream no longer has the expected currentUser default line; review it by hand");
  const expectedBody = upstream.replace(BEFORE, AFTER);

  const local = readFileSync(localPath, "utf8").replace(/\r\n/g, "\n");
  const cut = local.indexOf(MARKER);
  if (cut < 0) return fail("local template lacks the header marker line");
  const header = local.slice(0, cut + MARKER.length);
  const body = local.slice(cut + MARKER.length).replace(/^\n/, "");

  if (body === expectedBody && header.includes(tag)) {
    console.log(`ok: src-tauri/nsis/installer.nsi = upstream ${tag} + Programs default (${body.split("\n").length} lines)`);
    return 0;
  }
  if (!write) {
    const why = header.includes(tag) ? "body differs from upstream + our change" : `header names another tag than ${tag}`;
    console.error(`mismatch: ${why}; run with --write to refresh, then review the diff`);
    return 1;
  }
  writeFileSync(localPath, header.replace(/tauri-cli-v[\d.]+/g, tag) + "\n" + expectedBody);
  console.log(`refreshed src-tauri/nsis/installer.nsi from upstream ${tag}; review the diff`);
  return 0;
}

function fail(msg) {
  console.error(`nsis_template_check: ${msg}`);
  return 2;
}

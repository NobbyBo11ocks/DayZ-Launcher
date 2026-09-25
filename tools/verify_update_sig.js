// Verifies a Tauri updater manifest end to end, the way the app will see it:
// fetches latest.json from the endpoint in src-tauri/tauri.conf.json (or from a given
// URL/file), downloads the Windows installer it points at, and checks the minisign
// signature against the app's public key. Tauri signs with minisign's prehashed "ED"
// algorithm: Ed25519 over BLAKE2b-512 of the file, plus a global signature over
// (file signature || trusted comment). No dependencies beyond node:crypto.
// Usage: node tools/verify_update_sig.js [manifest-url-or-file] [--installer <local-file>] [--expect <version>]
// Exit code 0 only when the key ids match, both signatures verify, and the manifest
// and its signed trusted comment both name the expected version (by default the one in
// tauri.conf.json). The last check is what the app enforces too (`requireSignedVersion`,
// D-163), and without it a cached copy of the previous release's manifest — which
// verifies perfectly — passed the release gate on its first attempt (D-240).
import { createHash, createPublicKey, verify } from "node:crypto";
import { readFileSync, unlinkSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
let manifestSrc;
let installerPath;
let expected;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--installer") installerPath = args[++i];
  else if (args[i] === "--expect") expected = args[++i];
  else if (args[i].startsWith("--")) fail(`unknown option ${args[i]}`);
  else manifestSrc = args[i];
}

const conf = JSON.parse(readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
manifestSrc ??= conf.plugins.updater.endpoints[0];
expected ??= conf.version;

const manifest = JSON.parse(await readText(manifestSrc));
// The updater looks for `<os>-<arch>-<installer>` first and falls back to `<os>-<arch>`.
const win = manifest.platforms?.["windows-x86_64-nsis"] ?? manifest.platforms?.["windows-x86_64"];
if (!win?.url || !win?.signature) fail("manifest lacks platforms.windows-x86_64[-nsis].url/signature");
console.log(`manifest  ${manifestSrc}`);
console.log(`version   ${manifest.version}  pub_date ${manifest.pub_date ?? "-"}`);
console.log(`installer ${win.url}`);

let downloaded = false;
if (!installerPath) {
  downloaded = true;
  installerPath = path.join(os.tmpdir(), `dayz-launcher-verify-${process.pid}.exe`);
  // The header the updater sends: an api.github.com asset URL answers it with the file
  // and anything else with JSON metadata (D-147, D-240).
  writeFileSync(installerPath, Buffer.from(await (await fetchOk(win.url, { Accept: "application/octet-stream" })).arrayBuffer()));
}
const file = readFileSync(installerPath);
if (downloaded) unlinkSync(installerPath);

// Public key: base64(text) where the second line is base64(alg[2] || keyId[8] || key[32]).
const pubLine = Buffer.from(conf.plugins.updater.pubkey, "base64")
  .toString("utf8")
  .split(/\r?\n/)
  .find((l) => l && !l.startsWith("untrusted comment:"));
const pub = Buffer.from(pubLine, "base64");
const pubKeyId = pub.subarray(2, 10);
const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), pub.subarray(10, 42)]);
const key = createPublicKey({ key: spki, format: "der", type: "spki" });

// Signature: the `.sig` content is base64 of the minisign text block (4 lines).
const lines = Buffer.from(win.signature, "base64").toString("utf8").split(/\r?\n/);
const sigLine = Buffer.from(lines[1], "base64");
const trusted = (lines[2] ?? "").replace(/^trusted comment: /, "");
const globalSig = Buffer.from(lines[3] ?? "", "base64");
const alg = sigLine.subarray(0, 2).toString();
const sigKeyId = sigLine.subarray(2, 10);
const sig = sigLine.subarray(10, 74);

const message = alg === "ED" ? createHash("blake2b512").update(file).digest() : file;
const fileOk = verify(null, message, key, sig);
const globalOk = verify(null, Buffer.concat([sig, Buffer.from(trusted)]), key, globalSig);
const keyOk = pubKeyId.equals(sigKeyId);

console.log(`bytes     ${file.length}  sha256 ${createHash("sha256").update(file).digest("hex")}`);
console.log(`algorithm ${alg}  key id ${pubKeyId.toString("hex")} (${keyOk ? "matches" : "MISMATCH"})`);
console.log(`trusted   ${trusted}`);
console.log(`file signature   ${fileOk ? "valid" : "INVALID"}`);
console.log(`global signature ${globalOk ? "valid" : "INVALID"}`);
const signedVersion = /(?:^|\t)version:([^\t]*)/.exec(trusted)?.[1] ?? null;
const versionOk = manifest.version === expected && signedVersion === expected;
console.log(`version          ${versionOk ? "matches" : "MISMATCH"} (expected ${expected}, manifest ${manifest.version}, signed ${signedVersion ?? "-"})`);
process.exit(fileOk && globalOk && keyOk && versionOk ? 0 : 1);

async function readText(src) {
  return /^https?:\/\//.test(src) ? (await fetchOk(src)).text() : readFileSync(src, "utf8");
}
async function fetchOk(url, headers = {}) {
  const res = await fetch(url, { redirect: "follow", headers });
  if (!res.ok) fail(`${url} → HTTP ${res.status}`);
  return res;
}
function fail(msg) {
  console.error(`verify_update_sig: ${msg}`);
  process.exit(2);
}

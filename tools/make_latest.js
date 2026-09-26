// Builds the static updater manifest (latest.json) for a release.
// Usage: node tools/make_latest.js [tag] [--offline] [--notes <file>] [--out <file>]
//   tag defaults to v<version> from src-tauri/tauri.conf.json.
// Reads the local `.sig` from src-tauri/target/release/bundle/nsis/ and writes the
// manifest documented in docs/08 S-55. The installer URL is built, not read back:
// `<repo>/releases/download/<tag>/<asset>`, with <repo> from the updater endpoint in
// tauri.conf.json and <asset> the installer's name as GitHub stores it (spaces become
// dots, D-062). A draft's own asset URLs change when it is published; this one is final
// from the start, so the manifest can be written and checked while the release is still
// a draft (D-279). Online, the release must hold an asset of that name and its body is
// the notes; --offline skips GitHub entirely (the workflow's manual builds).
// Then: gh release upload <tag> <out> --clobber
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const options = { notes: undefined, out: undefined, offline: false };
let tag;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--notes" || a === "--out") options[a.slice(2)] = args[++i];
  else if (a === "--offline") options.offline = true;
  else if (a.startsWith("--")) fail(`unknown option ${a}`);
  else tag = a;
}

const conf = JSON.parse(readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
tag ??= `v${version}`;
if (tag !== `v${version}`) fail(`tag ${tag} does not match tauri.conf.json version ${version}`);

const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? "";
const repo = /^https:\/\/github\.com\/([^/]+\/[^/]+)\/releases\/latest\/download\/latest\.json$/.exec(endpoint)?.[1];
if (!repo) fail(`the updater endpoint is not a GitHub latest-release manifest: ${endpoint}`);

const bundleDir = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
const installerName = `${conf.productName}_${version}_x64-setup.exe`;
const assetName = installerName.replaceAll(" ", ".");
const signature = readFileSync(path.join(bundleDir, `${installerName}.sig`), "utf8").trim();
const url = `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(assetName)}`;

let notes = options.notes ? readFileSync(options.notes, "utf8").trim() : "";
if (!options.offline) {
  const release = JSON.parse(
    execFileSync("gh", ["release", "view", tag, "--json", "assets,body"], { encoding: "utf8", windowsHide: true }),
  );
  const names = release.assets.map((a) => a.name);
  if (!names.includes(assetName)) fail(`release ${tag} has no asset named ${assetName} (it has: ${names.join(", ") || "none"})`);
  if (!options.notes) notes = (release.body ?? "").trim();
}

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: { "windows-x86_64": { signature, url } },
};

const out = options.out ?? path.join(bundleDir, "latest.json");
writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${out}`);
console.log(`  version ${version}`);
console.log(`  url     ${url}`);
if (!options.offline) console.log(`next: gh release upload ${tag} "${out}" --clobber`);

function fail(msg) {
  console.error(`make_latest: ${msg}`);
  process.exit(2);
}

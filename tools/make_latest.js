// Builds the static updater manifest (latest.json) for a published GitHub release.
// Usage: node tools/make_latest.js [tag] [--notes <file>] [--out <file>]
//   tag defaults to v<version> from src-tauri/tauri.conf.json.
// Reads the local `.sig` from src-tauri/target/release/bundle/nsis/, asks `gh` for the
// installer asset URL of that release (GitHub renames spaces in asset names to dots,
// so the URL must come from the API, D-062), and writes the manifest documented in
// docs/08 S-55. Then: gh release upload <tag> <out> --clobber
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const options = { notes: undefined, out: undefined };
let tag;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--notes" || a === "--out") options[a.slice(2)] = args[++i];
  else if (a.startsWith("--")) fail(`unknown option ${a}`);
  else tag = a;
}

const conf = JSON.parse(readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
tag ??= `v${version}`;
if (tag !== `v${version}`) fail(`tag ${tag} does not match tauri.conf.json version ${version}`);

const bundleDir = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
const installerName = `${conf.productName}_${version}_x64-setup.exe`;
const sigPath = path.join(bundleDir, `${installerName}.sig`);
const signature = readFileSync(sigPath, "utf8").trim();

const release = JSON.parse(
  execFileSync("gh", ["release", "view", tag, "--json", "assets,body"], { encoding: "utf8", windowsHide: true }),
);
const asset = release.assets.find((a) => a.name.endsWith("_x64-setup.exe"));
if (!asset) fail(`release ${tag} has no *_x64-setup.exe asset; upload the installer first`);

const notes = options.notes ? readFileSync(options.notes, "utf8").trim() : (release.body ?? "").trim();
const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: { "windows-x86_64": { signature, url: asset.url } },
};

const out = options.out ?? path.join(bundleDir, "latest.json");
writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${out}`);
console.log(`  version ${version}`);
console.log(`  url     ${asset.url}`);
console.log(`next: gh release upload ${tag} "${out}" --clobber`);

function fail(msg) {
  console.error(`make_latest: ${msg}`);
  process.exit(2);
}

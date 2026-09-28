#!/usr/bin/env node
// Front-end regression tests (row 20 of docs/13). The stores and helpers under src/lib are
// built the way the app's bundle builds them — TypeScript stripped by Node itself, the
// `.svelte.ts` runes compiled by the project's own Svelte (node_modules/svelte,
// `compileModule`) — into a temp folder, with every `@tauri-apps/*` import pointed at a
// stand-in (tests/front/tauri-mock.mjs), and then tests/front/*.test.mjs run under Node's
// own test runner. Nothing beyond package.json's dependencies; the repository is only read.
//
//   node tools/front_test.mjs                 build, run every test, exit non-zero on a failure
//   node tools/front_test.mjs uiprefs         only the test files whose name contains "uiprefs"
//   node tools/front_test.mjs --keep          keep the build folder and print where it is
//   node tools/front_test.mjs --test-name-pattern=refresh   (any --test-* flag goes to node --test)
//
// A test file run on its own (`node --test tests/front/servers.test.mjs`) builds for itself.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import * as nodeModule from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const { createRequire, stripTypeScriptTypes } = nodeModule;

/** Where tests/front lives: this checkout. */
export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
/** The source tree built: this checkout. */
export const REPO = ROOT;
const LIB = path.join(REPO, "src", "lib");
const MOCK = path.join(ROOT, "tests", "front", "tauri-mock.mjs");

/** The module every `@tauri-apps/*` import becomes, at the root of the built folder. */
const MOCK_NAME = "__tauri__.js";

/** Picks an `exports` entry by condition, the way a bundler targeting the WebView does. */
function pickExport(entry, conditions) {
  if (typeof entry === "string") return entry;
  if (!entry || typeof entry !== "object") return null;
  for (const [cond, value] of Object.entries(entry)) {
    if (cond === "types") continue;
    if (cond === "default" || conditions.includes(cond)) {
      const hit = pickExport(value, conditions);
      if (hit) return hit;
    }
  }
  return null;
}

function packageDir(name) {
  try {
    return path.dirname(createRequire(path.join(REPO, "package.json")).resolve(`${name}/package.json`));
  } catch {
    const dir = path.join(REPO, "node_modules", ...name.split("/"));
    if (existsSync(path.join(dir, "package.json"))) return dir;
    throw new Error(`front_test: ${name} is not installed; run npm ci first`);
  }
}

/**
 * A package import (`svelte/reactivity`, say) as the WebView's bundle resolves it — the
 * `browser` build, not the server one — as a file URL, or null when there is none.
 */
function resolvePackage(spec) {
  const m = /^((?:@[^/]+\/)?[^/@][^/]*)(\/.+)?$/.exec(spec);
  if (!m) return null;
  let dir;
  try {
    dir = packageDir(m[1]);
  } catch {
    return null;
  }
  const pkg = JSON.parse(readFileSync(path.join(dir, "package.json"), "utf8"));
  const sub = m[2] ? `.${m[2]}` : ".";
  const ex = pkg.exports;
  const conditions = ["browser", "import"];
  let target = null;
  if (!ex) target = sub === "." ? (pkg.module ?? pkg.main ?? "index.js") : sub;
  else if (typeof ex !== "object" || !Object.keys(ex).some((k) => k.startsWith("."))) target = sub === "." ? pickExport(ex, conditions) : null;
  else if (ex[sub] !== undefined) target = pickExport(ex[sub], conditions);
  else {
    // A subpath pattern such as @tauri-apps/api's "./*".
    for (const [key, value] of Object.entries(ex)) {
      const [pre, post, extra] = key.split("*");
      if (post === undefined || extra !== undefined || !sub.startsWith(pre) || !sub.endsWith(post) || sub.length < pre.length + post.length) continue;
      target = pickExport(value, conditions)?.replaceAll("*", sub.slice(pre.length, sub.length - post.length)) ?? null;
      if (target) break;
    }
  }
  return target ? pathToFileURL(path.join(dir, target)).href : null;
}

/**
 * Rewrites the specifier of every import and re-export statement with `map(spec)`, and
 * throws on one it cannot place. Statements only, found where a line starts: the sources'
 * comments say things like `went from "could not be read" to …`.
 */
function rewriteImports(code, map, file) {
  const fix = (spec) => {
    const to = map(spec);
    if (to == null) throw new Error(`front_test: ${file} imports "${spec}", which the build does not know how to provide`);
    return to;
  };
  if (/(^|[^.\w$])import\s*\(/.test(code.replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\//g, ""))) {
    throw new Error(`front_test: ${file} has a dynamic import(), which the build does not rewrite`);
  }
  return code
    .replace(/(^|\n)([ \t]*(?:import|export)\b[\w*{}\s,$]*?\bfrom\s*)(["'])([^"'\n]+)\3/g, (_m, nl, pre, q, spec) => `${nl}${pre}${q}${fix(spec)}${q}`)
    .replace(/(^|\n)([ \t]*import\s*)(["'])([^"'\n]+)\3/g, (_m, nl, pre, q, spec) => `${nl}${pre}${q}${fix(spec)}${q}`);
}

function listSources(dir, base = dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) out.push(...listSources(full, base));
    else if (name.endsWith(".ts") && !name.endsWith(".d.ts")) out.push(path.relative(base, full));
  }
  return out;
}

/**
 * Builds src/lib into `out/app` and returns `out`. `transform(rel, source)` may rewrite a
 * source file before it is built (rel like "src/lib/state/servers.svelte.ts") — how a
 * mutation check reverts one fix in a throw-away build.
 */
export async function build({ out = mkdtempSync(path.join(tmpdir(), "front-test-")), transform } = {}) {
  if (typeof stripTypeScriptTypes !== "function") throw new Error(`front_test: Node ${process.version} cannot strip TypeScript; it needs 22.13 or later (CI uses 24)`);
  const { compileModule, VERSION } = await import(resolvePackage("svelte/compiler"));
  const tauriCore = resolvePackage("@tauri-apps/api/core");
  const app = path.join(out, "app");
  mkdirSync(app, { recursive: true });

  // Node warns once that the stripper is experimental; the build says what it used instead.
  const emitWarning = process.emitWarning;
  process.emitWarning = (w, ...rest) => (String(w?.message ?? w).includes("stripTypeScriptTypes") ? undefined : emitWarning.call(process, w, ...rest));
  try {
    for (const rel of listSources(LIB)) {
      const srcRel = path.posix.join("src/lib", rel.split(path.sep).join("/"));
      let code = readFileSync(path.join(LIB, rel), "utf8");
      if (transform) code = transform(srcRel, code);
      code = stripTypeScriptTypes(code, { mode: "strip" });
      const runes = rel.endsWith(".svelte.ts");
      // What the vite plugin does after its TypeScript step: runes to plain JS (docs/05 §7).
      if (runes) code = compileModule(code, { filename: srcRel, generate: "client", dev: false }).js.code;
      const outRel = rel.replace(/\.ts$/, ".js");
      const outFile = path.join(app, outRel);
      const fromDir = path.dirname(path.join(LIB, rel));
      const toMock = path.relative(path.dirname(outFile), path.join(app, MOCK_NAME)).split(path.sep).join("/");
      code = rewriteImports(
        code,
        (spec) => {
          if (spec.startsWith("@tauri-apps/")) return toMock.startsWith(".") ? toMock : `./${toMock}`;
          if (spec.startsWith("./") || spec.startsWith("../")) return existsSync(path.join(fromDir, `${spec}.ts`)) ? `${spec}.js` : null;
          return resolvePackage(spec);
        },
        srcRel,
      );
      mkdirSync(path.dirname(outFile), { recursive: true });
      writeFileSync(outFile, code);
    }
  } finally {
    process.emitWarning = emitWarning;
  }
  // The stand-in, its one real import pointed at @tauri-apps/api's own Channel.
  const mock = rewriteImports(readFileSync(MOCK, "utf8"), (spec) => (spec === "@tauri-apps/api/core" ? tauriCore : null), "tests/front/tauri-mock.mjs");
  writeFileSync(path.join(app, MOCK_NAME), mock);
  writeFileSync(path.join(out, "build.json"), JSON.stringify({ repo: REPO, svelte: VERSION, node: process.version, builtAt: new Date().toISOString() }, null, 2));
  return out;
}

/** A fresh copy of the built modules, so each test gets its own stores and mock. */
export function copyApp(buildDir, dest) {
  cpSync(path.join(buildDir, "app"), dest, { recursive: true });
  return dest;
}

async function main(argv) {
  const keep = argv.includes("--keep");
  const passthrough = argv.filter((a) => a.startsWith("--test"));
  const filters = argv.filter((a) => !a.startsWith("--"));
  const testDir = path.join(ROOT, "tests", "front");
  const files = readdirSync(testDir)
    .filter((f) => f.endsWith(".test.mjs") && (filters.length === 0 || filters.some((x) => f.includes(x))))
    .sort()
    .map((f) => path.join(testDir, f));
  if (files.length === 0) {
    console.error(`front_test: no test file matches ${filters.join(", ")}`);
    return 1;
  }
  const started = performance.now();
  const out = await build();
  const built = performance.now();
  console.log(`front_test: built src/lib in ${Math.round(built - started)} ms${keep ? ` into ${out}` : ""}`);
  let status = 1;
  try {
    // Every test takes well under a second; one that hangs fails at 15 s instead of holding
    // CI, and nothing a store left running keeps a test process alive.
    const r = spawnSync(process.execPath, ["--test", "--test-reporter=spec", "--test-force-exit", "--test-timeout=15000", ...passthrough, ...files], {
      stdio: "inherit",
      // The stores see a production build, as the app does (esm-env's DEV is off).
      env: { ...process.env, FRONT_TEST_BUILD: out, NODE_ENV: "production" },
    });
    status = r.status ?? 1;
    console.log(`front_test: ${files.length} file(s) in ${Math.round(performance.now() - started)} ms, ${status === 0 ? "all passed" : "FAILED"}`);
  } finally {
    if (!keep) rmSync(out, { recursive: true, force: true });
  }
  return status;
}

if (import.meta.main ?? (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)) {
  process.exitCode = await main(process.argv.slice(2));
}

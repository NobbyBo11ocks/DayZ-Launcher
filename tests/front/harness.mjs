// Shared by tests/front/*.test.mjs: a fresh copy of the built stores per test, the page
// globals they touch, time under the test's control, and small builders for rows and
// updates. The built stores come from tools/front_test.mjs (FRONT_TEST_BUILD); a test file
// run on its own builds them once for its process.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { build, copyApp } from "../../tools/front_test.mjs";

let built = null;
function buildDir() {
  if (process.env.FRONT_TEST_BUILD) return Promise.resolve(process.env.FRONT_TEST_BUILD);
  built ??= (async () => {
    const out = mkdtempSync(path.join(tmpdir(), "front-test-"));
    process.on("exit", () => rmSync(out, { recursive: true, force: true }));
    return build({ out });
  })();
  return built;
}

/** A fixed wall clock for every test: 2026-09-28 12:00 UTC. */
export const NOW = Date.UTC(2026, 8, 28, 12, 0, 0);

let generation = 0;

/**
 * A new set of stores with their own mock host. The stores' constructors run on first
 * `load`, so script start-up answers on `app.mock` before loading. Timers and the clock
 * are node:test's mock timers from here on (`advance` moves them), unless `timers: false`.
 */
export async function freshApp(t, { timers = true, now = NOW, storage = {}, online = true } = {}) {
  if (timers) t.mock.timers.enable({ apis: ["setTimeout", "setInterval", "Date"], now });
  const base = await buildDir();
  const dir = copyApp(base, path.join(base, "gen", `${process.pid}-${++generation}`));

  // The page as the stores see it: `window` events, storage, the network flag.
  const win = new EventTarget();
  globalThis.window = win;
  const store = new Map(Object.entries(storage));
  const localStorage = {
    getItem: (k) => (store.has(k) ? store.get(k) : null),
    setItem: (k, v) => void store.set(k, String(v)),
    removeItem: (k) => void store.delete(k),
    clear: () => store.clear(),
  };
  Object.defineProperty(globalThis, "localStorage", { value: localStorage, configurable: true, writable: true });
  const env = { online };
  Object.defineProperty(globalThis.navigator, "onLine", { get: () => env.online, configurable: true });

  const { mock } = await import(pathToFileURL(path.join(dir, "__tauri__.js")).href);
  // A command nothing answered reached the store as a failure the test did not script:
  // the test is then about something else than it says.
  t.after(() => {
    if (mock.unhandled.length) throw new Error(`the stores called ${[...new Set(mock.unhandled)].join(", ")}, which this test does not answer`);
  });
  return {
    mock,
    window: win,
    storage: store,
    env,
    /** A module of src/lib by its path there, without the extension: "state/servers.svelte". */
    load: (rel) => import(pathToFileURL(path.join(dir, `${rel}.js`)).href),
    /** Fires a page event: "online", "offline", "focus". */
    fire: (type) => win.dispatchEvent(new Event(type)),
  };
}

/** Lets every pending promise run (the stores' awaits are microtasks). */
export async function settle(rounds = 5) {
  for (let i = 0; i < rounds; i++) await new Promise((r) => setImmediate(r));
}

/** Moves the mock clock `ms` forward in `step` slices, letting promises run after each. */
export async function advance(t, ms, step = ms) {
  let left = ms;
  while (left > 0) {
    const d = Math.min(step, left);
    t.mock.timers.tick(d);
    left -= d;
    await settle();
  }
}

/** A promise the test resolves or rejects by hand: a host answer held in flight. */
export function deferred() {
  let resolve, reject;
  const promise = new Promise((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

const TAGS = { battleye: true, firstPersonOnly: false, privateHive: false, modded: false, dlc: false, allowedFilePatching: false };
let seq = 0;

/** A server row as the host sends one; addresses from TEST-NET-2, never a LAN range. */
export function row(over = {}) {
  const n = ++seq;
  const ip = over.ip ?? `198.51.100.${(n % 250) + 1}`;
  const queryPort = over.queryPort ?? 27000 + n;
  return {
    ip,
    gamePort: 2300 + n,
    queryPort,
    name: `Server ${n}`,
    map: "chernarusplus",
    description: "",
    players: 10,
    maxPlayers: 60,
    password: false,
    serverVersion: 0,
    version: "1.29.163709",
    pingMs: 40,
    steamEmpty: false,
    verifiedPlayers: null,
    verifiedAt: null,
    verdict: null,
    ...over,
    id: over.id ?? `${ip}:${queryPort}`,
    tags: { ...TAGS, ...over.tags },
  };
}

/** Steam's answer for `steam:status`, session open and idle unless told otherwise. */
export function steam(over = {}) {
  return { initialized: true, error: null, appId: 221100, steamId: 1, persona: "Player", refreshing: false, lastRefreshSecsAgo: null, idle: false, ...over };
}

/** A `done` message of the row stream. */
export function done(over = {}) {
  return { kind: "done", data: { source: "steam", total: 0, responded: 0, failed: 0, inflated: 0, elapsedMs: 1, partitions: [], capped: false, stoppedEarly: false, complete: true, ...over } };
}

/**
 * An update as @tauri-apps/plugin-updater hands one out. Its download stays open until
 * the test calls `finish()` or `failDownload(e)`; `onEvent` is the store's progress handler.
 */
export function fakeUpdate(version = "9.9.9") {
  let open;
  const u = {
    version,
    currentVersion: "0.1.87",
    downloads: 0,
    installs: 0,
    closes: 0,
    onEvent: null,
    download(onEvent, options) {
      u.downloads++;
      u.onEvent = onEvent;
      u.options = options;
      return new Promise((resolve, reject) => (open = { resolve, reject }));
    },
    async install() {
      u.installs++;
      if (u.installError) throw u.installError;
    },
    async close() {
      u.closes++;
    },
    finish: () => open.resolve(),
    failDownload: (e) => open.reject(e),
  };
  return u;
}

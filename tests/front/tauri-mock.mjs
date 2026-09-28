// The host, as far as the front end's stores can tell. tools/front_test.mjs points every
// `@tauri-apps/*` import at a copy of this file, one per fresh set of stores
// (harness.mjs), so no test sees another's calls, listeners or replies.
//
// Commands go through `invoke`, which records them and answers from what the test
// scripted: `mock.handle(cmd, fnOrValue)` for a standing answer, `mock.reply(cmd, ...)`
// and `mock.fail(cmd, error)` for one-off answers taken in order. The plugins the stores
// use (updater, process, notification, opener) are commands too, so they are scripted and
// recorded the same way. `mock.emit(event, payload)` delivers an event to `listen`, and
// `mock.send(message)` sends one message on the row stream through @tauri-apps/api's own
// Channel, so its ordering rule — a message counts as delivered only once its handler
// returns — is the real one (D-297, row 14 F2).
export { Channel } from "@tauri-apps/api/core";

const callbacks = new Map();
let nextCallback = 1;
// What @tauri-apps/api/core's `transformCallback` needs from the page.
globalThis.window.__TAURI_INTERNALS__ = {
  transformCallback(cb) {
    const id = nextCallback++;
    callbacks.set(id, cb);
    return id;
  },
  unregisterCallback(id) {
    callbacks.delete(id);
  },
};

const standing = new Map();
const queued = new Map();
const listeners = new Map();

/** Answers every page needs at start-up; a test replaces what it is about. */
const defaults = {
  settings_get: () => ({
    profileName: "",
    extraArgs: "",
    skipIntro: true,
    noSplash: true,
    noPause: false,
    launchProfiles: [],
    steamIdleMinutes: 5,
    logging: true,
    logMuted: [],
    ui: { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen: 0, news: true, lastSeenVersion: "", openOn: "news" },
  }),
  ui_prefs_set: ({ patch }) => ({ ...defaults.settings_get().ui, ...patch }),
  log_ui: () => null,
  servers_cached: () => ({ keys: [], tagKeys: [], rows: [], lastRefresh: null }),
  favourites_list: () => [],
  rows_subscribe: ({ channel }) => {
    mock.rows = channel;
    return null;
  },
  steam_status: () => ({ initialized: false, error: null, appId: 221100, steamId: null, persona: null, refreshing: false, lastRefreshSecsAgo: null, idle: false }),
  local_game_version: () => "1.29.163709",
  friends_list: () => [],
  cache_status: () => ({ inMemory: false }),
  "plugin:window|is_visible": () => true,
  "plugin:window|is_focused": () => true,
};

export const mock = {
  /** Every command in order, as `[cmd, args]`. */
  calls: [],
  /** What the stores wrote to the log (`log_ui`): `{ level, target, message }`. */
  logs: [],
  /** Commands nothing answers: a test that meets one is testing the wrong thing. */
  unhandled: [],
  /** Throws from a row-stream handler, which in the WebView are uncaught errors. */
  uncaught: [],
  /** The row stream the servers store subscribed with, and the host's next index on it. */
  rows: null,
  rowIndex: 0,

  handle(cmd, answer) {
    standing.set(cmd, typeof answer === "function" ? answer : () => answer);
    return mock;
  },
  /** One-off replies, taken in order before the standing answer. */
  reply(cmd, ...values) {
    const q = queued.get(cmd) ?? [];
    for (const v of values) q.push({ ok: true, v });
    queued.set(cmd, q);
    return mock;
  },
  /** A one-off failure, or with `{ always: true }` a standing one. */
  fail(cmd, error, { always = false } = {}) {
    if (always) standing.set(cmd, () => Promise.reject(error));
    else {
      const q = queued.get(cmd) ?? [];
      q.push({ ok: false, v: error });
      queued.set(cmd, q);
    }
    return mock;
  },
  /** The arguments of every call to `cmd`. */
  argsOf(cmd) {
    return mock.calls.filter(([c]) => c === cmd).map(([, a]) => a);
  },
  count(cmd) {
    return mock.calls.filter(([c]) => c === cmd).length;
  },
  emit(event, payload) {
    for (const cb of [...(listeners.get(event) ?? [])]) cb({ event, id: 0, payload });
  },
  /** One message on the row stream, as the host's `send_rows` sends it. */
  send(message) {
    if (!mock.rows) throw new Error("tauri-mock: nothing subscribed to the row stream");
    const cb = callbacks.get(mock.rows.id);
    if (!cb) throw new Error("tauri-mock: the row stream's callback is gone");
    try {
      cb({ index: mock.rowIndex++, message });
    } catch (e) {
      mock.uncaught.push(e);
    }
  },
};

export async function invoke(cmd, args) {
  mock.calls.push([cmd, args]);
  if (cmd === "log_ui") mock.logs.push(args);
  const q = queued.get(cmd);
  if (q?.length) {
    const { ok, v } = q.shift();
    if (!ok) throw v;
    return typeof v === "function" ? v(args ?? {}) : v;
  }
  const h = standing.get(cmd) ?? defaults[cmd];
  if (!h) {
    mock.unhandled.push(cmd);
    throw new Error(`tauri-mock: no answer for ${cmd}`);
  }
  return h(args ?? {});
}

// @tauri-apps/api/event
export async function listen(event, cb) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event).add(cb);
  return () => listeners.get(event)?.delete(cb);
}

// @tauri-apps/api/window
export const UserAttentionType = { Critical: 1, Informational: 2 };
export function getCurrentWindow() {
  return {
    label: "main",
    isVisible: () => invoke("plugin:window|is_visible"),
    isFocused: () => invoke("plugin:window|is_focused"),
    show: () => invoke("plugin:window|show").catch(() => {}),
    setFocus: () => invoke("plugin:window|set_focus").catch(() => {}),
    requestUserAttention: (kind) => invoke("plugin:window|request_user_attention", { kind }).catch(() => {}),
  };
}

// @tauri-apps/plugin-updater: `check` answers what the test scripted for the command,
// usually an update built by harness.mjs's `fakeUpdate`.
export async function check(options) {
  return invoke("plugin:updater|check", options ?? {});
}

// @tauri-apps/plugin-process
export async function relaunch() {
  return invoke("plugin:process|restart");
}

// @tauri-apps/plugin-notification
export async function isPermissionGranted() {
  return invoke("plugin:notification|is_permission_granted");
}
export async function requestPermission() {
  return invoke("plugin:notification|request_permission");
}
export function sendNotification(options) {
  void invoke("plugin:notification|notify", { options }).catch(() => {});
}

// @tauri-apps/plugin-opener
export async function openUrl(url) {
  return invoke("plugin:opener|open_url", { url });
}
export async function revealItemInDir(path) {
  return invoke("plugin:opener|reveal_item_in_dir", { path });
}

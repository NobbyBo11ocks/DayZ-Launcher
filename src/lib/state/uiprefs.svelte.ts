// UI preferences (theme, accent, filters, onboarding, last update check) live in the
// launcher's settings.json on the Rust side (D-070). localStorage is only an
// instant-start cache kept by the individual stores; this module is the single
// writer to the file. Patches are coalesced so a burst of filter toggles is one write.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import type { Settings, UiPrefs } from "../types";

const FLUSH_MS = 150;

export const defaultUiPrefs = (): UiPrefs => ({
  theme: "slate",
  accent: "lime",
  onboarded: false,
  filters: null,
  lastUpdateCheckMs: 0,
  newsSeen: 0,
  news: true,
});

/** Attempts to read the settings before giving up on the backend (D-112). */
const READ_TRIES = 8;
const READ_RETRY_MS = 250;

/**
 * JSON with object keys sorted, so a comparison does not depend on key order. The host
 * sends `filters` back with its keys sorted, so unchanged filters never compared equal
 * and every save of them, one per debounced search, cost an IPC round trip (D-256).
 */
function stable(v: unknown): string {
  return JSON.stringify(v, (_k, x: unknown) =>
    x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) : x,
  );
}

class UiPrefsStore {
  /** Last state confirmed by the backend, or the defaults if it could not be read. */
  current: UiPrefs | null = null;
  /** True once the backend answered; false means `current` is the defaults, not the file. */
  readOk = false;
  /** Resolves once the file has been read; stores reconcile their caches against it. */
  readonly ready: Promise<UiPrefs>;
  #pending: Partial<UiPrefs> = {};
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    this.ready = this.read();
    window.addEventListener("beforeunload", () => void this.flush());
  }

  /**
   * The first IPC call of the page. The window can exist before the backend has
   * finished its setup on a slow start (D-112), so a failure is retried briefly
   * instead of being mistaken for "no settings yet".
   */
  private async read(): Promise<UiPrefs> {
    for (let i = 0; i < READ_TRIES; i++) {
      try {
        const s = await invoke<Settings>("settings_get");
        this.readOk = true;
        // A change made before the file arrived is still on its way to it; the copy
        // has to show it, or `patch` compares the next change against stale values.
        this.current = { ...(s.ui ?? defaultUiPrefs()), ...this.#pending };
        return this.current;
      } catch {
        await new Promise((r) => setTimeout(r, READ_RETRY_MS));
      }
    }
    this.current = { ...defaultUiPrefs(), ...this.#pending };
    return this.current;
  }

  /** Merges a partial update; no-op keys are dropped, the rest is written shortly. */
  patch(p: Partial<UiPrefs>) {
    const cur = this.current;
    let changed = false;
    for (const [k, v] of Object.entries(p) as [keyof UiPrefs, UiPrefs[keyof UiPrefs]][]) {
      if (cur && stable(cur[k]) === stable(v)) continue;
      (this.#pending as Record<string, unknown>)[k] = v;
      changed = true;
    }
    if (!changed) return;
    // Never mutate the object `ready` resolved with: stores compare against it.
    if (cur) this.current = { ...cur, ...this.#pending };
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.flush(), FLUSH_MS);
  }

  async flush() {
    clearTimeout(this.#timer);
    const p = this.#pending;
    if (Object.keys(p).length === 0) return;
    this.#pending = {};
    await this.ready;
    try {
      const saved = await invoke<UiPrefs>("ui_prefs_set", { patch: p });
      // The file as saved, plus whatever changed while that was in flight. Taking the
      // reply alone put a value the user had just moved away from back into the copy:
      // moving back to it was then dropped as "no change", and the queued write saved
      // the other one, so the screen and the file disagreed (D-256).
      this.current = { ...saved, ...this.#pending };
    } catch {
      /* the caches still hold the values; the next patch retries the file */
      this.#pending = { ...p, ...this.#pending };
    }
  }
}

export const uiPrefs = new UiPrefsStore();

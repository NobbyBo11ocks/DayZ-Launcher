// UI preferences (theme, accent, filters, onboarding, last update check) live in the
// launcher's settings.json on the Rust side (D-070). localStorage is only an
// instant-start cache kept by the individual stores; this module is the single
// writer to the file. Patches are coalesced so a burst of filter toggles is one write.
import { invoke } from "@tauri-apps/api/core";
import type { Settings, UiPrefs } from "../types";

const FLUSH_MS = 150;

export const defaultUiPrefs = (): UiPrefs => ({
  theme: "slate",
  accent: "lime",
  onboarded: false,
  filters: null,
  lastUpdateCheckMs: 0,
  newsSeen: 0,
});

/** Attempts to read the settings before giving up on the backend (D-112). */
const READ_TRIES = 8;
const READ_RETRY_MS = 250;

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
        this.current = s.ui ?? defaultUiPrefs();
        return this.current;
      } catch {
        await new Promise((r) => setTimeout(r, READ_RETRY_MS));
      }
    }
    this.current = defaultUiPrefs();
    return this.current;
  }

  /** Merges a partial update; no-op keys are dropped, the rest is written shortly. */
  patch(p: Partial<UiPrefs>) {
    const cur = this.current;
    let changed = false;
    for (const [k, v] of Object.entries(p) as [keyof UiPrefs, UiPrefs[keyof UiPrefs]][]) {
      if (cur && JSON.stringify(cur[k]) === JSON.stringify(v)) continue;
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
      this.current = await invoke<UiPrefs>("ui_prefs_set", { patch: p });
    } catch {
      /* the caches still hold the values; the next patch retries the file */
      this.#pending = { ...p, ...this.#pending };
    }
  }
}

export const uiPrefs = new UiPrefsStore();

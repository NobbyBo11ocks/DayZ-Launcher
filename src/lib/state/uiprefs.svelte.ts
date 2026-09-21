// UI preferences (theme, accent, filters, onboarding, last update check) live in the
// launcher's settings.json on the Rust side (D-070). localStorage is only an
// instant-start cache kept by the individual stores; this module is the single
// writer to the file. Patches are coalesced so a burst of filter toggles is one write.
import { invoke } from "@tauri-apps/api/core";
import type { Settings, UiPrefs } from "../types";

const FLUSH_MS = 150;

export const defaultUiPrefs = (): UiPrefs => ({
  theme: "slate",
  accent: "amber",
  onboarded: false,
  filters: null,
  lastUpdateCheckMs: 0,
  newsSeen: 0,
});

class UiPrefsStore {
  /** Last state confirmed by the backend, or the defaults if it could not be read. */
  current: UiPrefs | null = null;
  /** Resolves once the file has been read; stores reconcile their caches against it. */
  readonly ready: Promise<UiPrefs>;
  #pending: Partial<UiPrefs> = {};
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    this.ready = invoke<Settings>("settings_get")
      .then((s) => (this.current = s.ui ?? defaultUiPrefs()))
      .catch(() => (this.current = defaultUiPrefs()));
    window.addEventListener("beforeunload", () => void this.flush());
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

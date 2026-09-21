// Theme and accent. The settings file is the source of truth (D-070); localStorage
// is read synchronously at start-up so the first paint already has the right theme,
// then the file's values win as soon as they arrive. Every storage access is guarded.
import { uiPrefs } from "./uiprefs.svelte";

export type Theme = "slate" | "light";
export type Accent = "amber" | "teal" | "red" | "green";

const KEY = "dayz-launcher.prefs.v1";
const ACCENTS: Accent[] = ["amber", "teal", "red", "green"];

const normTheme = (t: unknown): Theme => (t === "light" ? "light" : "slate");
const normAccent = (a: unknown): Accent => (ACCENTS.includes(a as Accent) ? (a as Accent) : "amber");

function loadCache(): { theme: Theme; accent: Accent } {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = JSON.parse(raw) as { theme?: unknown; accent?: unknown };
      return { theme: normTheme(p.theme), accent: normAccent(p.accent) };
    }
  } catch {
    /* storage unavailable */
  }
  return { theme: "slate", accent: "amber" };
}

class Prefs {
  theme = $state<Theme>("slate");
  accent = $state<Accent>("amber");
  #fromFile = false;

  constructor() {
    const c = loadCache();
    this.theme = c.theme;
    this.accent = c.accent;
    void uiPrefs.ready.then((u) => {
      // A file that still has the defaults and no onboarding mark has never been
      // written by this build: seed it from the cache instead of overriding the cache
      // (users of earlier builds keep their theme). Afterwards the file wins.
      const unset = u.theme === "slate" && u.accent === "amber" && !u.onboarded;
      if (!unset) {
        this.theme = normTheme(u.theme);
        this.accent = normAccent(u.accent);
      }
      this.#fromFile = true;
      uiPrefs.patch({ theme: this.theme, accent: this.accent });
    });
  }

  /** Applies to <html> and persists. Called from an $effect in App.svelte. */
  apply() {
    const root = document.documentElement;
    root.dataset.theme = this.theme;
    root.dataset.accent = this.accent;
    try {
      localStorage.setItem(KEY, JSON.stringify({ theme: this.theme, accent: this.accent }));
    } catch {
      /* ignore */
    }
    // Do not push the cache over the file before the file has been read.
    if (this.#fromFile) uiPrefs.patch({ theme: this.theme, accent: this.accent });
  }
}

export const prefs = new Prefs();

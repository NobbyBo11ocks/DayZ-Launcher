// Theme and accent. The settings file is the source of truth (D-070); localStorage
// is read synchronously at start-up so the first paint already has the right theme,
// then the file's values win as soon as they arrive. Every storage access is guarded.
import { uiPrefs } from "./uiprefs.svelte";

export type Theme = "slate" | "light";
export type Accent = "amber" | "orange" | "red" | "rose" | "pink" | "violet" | "indigo" | "blue" | "sky" | "teal" | "green" | "lime";

const KEY = "dayz-launcher.prefs.v1";
const NEWS_KEY = "dayz-launcher.news.v1";
/** Colour-wheel order, as shown in Settings. Tokens live in app.css; the host list in settings.rs. */
export const ACCENTS: Accent[] = ["amber", "orange", "red", "rose", "pink", "violet", "indigo", "blue", "sky", "teal", "green", "lime"];

const normTheme = (t: unknown): Theme => (t === "light" ? "light" : "slate");
/** Lime is the default (D-132); it must match `UiPrefs::default` in settings.rs. */
const DEFAULT_ACCENT: Accent = "lime";
const normAccent = (a: unknown): Accent => (ACCENTS.includes(a as Accent) ? (a as Accent) : DEFAULT_ACCENT);

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
  return { theme: "slate", accent: DEFAULT_ACCENT };
}

class Prefs {
  theme = $state<Theme>("slate");
  accent = $state<Accent>(DEFAULT_ACCENT);
  /**
   * Whether the News page exists at all (D-174). Cached in localStorage like the
   * theme so the sidebar paints the right shape on the first frame, with the settings
   * file winning as soon as it is read.
   */
  news = $state(true);
  #fromFile = false;

  constructor() {
    const c = loadCache();
    this.theme = c.theme;
    this.accent = c.accent;
    try {
      this.news = localStorage.getItem(NEWS_KEY) !== "off";
    } catch {
      /* storage unavailable */
    }
    void uiPrefs.ready.then((u) => {
      // A file that still has the defaults and no onboarding mark has never been
      // written by this build: seed it from the cache instead of overriding the cache
      // (users of earlier builds keep their theme). Afterwards the file wins.
      const unset = u.theme === "slate" && u.accent === DEFAULT_ACCENT && !u.onboarded;
      if (!unset) {
        this.theme = normTheme(u.theme);
        this.accent = normAccent(u.accent);
      }
      // Only when the file was actually read. `read()` falls back to the defaults
      // after its retries, and the default is `news: true` — so a backend slow past
      // ~2 s used to switch the News page back on for someone who had turned it off,
      // and with it the fetches Settings promises never happen (D-194).
      if (uiPrefs.readOk) this.news = u.news !== false;
      this.#fromFile = true;
      uiPrefs.patch({ theme: this.theme, accent: this.accent });
    });
  }

  /** Shows or hides the News page, and stops the feed being fetched (D-174). */
  setNews(on: boolean) {
    this.news = on;
    try {
      localStorage.setItem(NEWS_KEY, on ? "on" : "off");
    } catch {
      /* ignore */
    }
    uiPrefs.patch({ news: on });
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

// Per-viewer UI preferences (theme, accent, filter defaults) in localStorage.
// Every access is guarded: storage can be unavailable or blocked (docs/05 §7).

export type Theme = "slate" | "light";
export type Accent = "amber" | "teal" | "red" | "green";

const KEY = "dayz-launcher.prefs.v1";

type Stored = { theme: Theme; accent: Accent };

function load(): Stored {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<Stored>;
      return {
        theme: p.theme === "light" ? "light" : "slate",
        accent: (["amber", "teal", "red", "green"] as Accent[]).includes(p.accent as Accent) ? (p.accent as Accent) : "amber",
      };
    }
  } catch {
    /* storage unavailable */
  }
  return { theme: "slate", accent: "amber" };
}

class Prefs {
  theme = $state<Theme>("slate");
  accent = $state<Accent>("amber");

  constructor() {
    const s = load();
    this.theme = s.theme;
    this.accent = s.accent;
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
  }
}

export const prefs = new Prefs();

// In-app updates via tauri-plugin-updater (signed artifacts, docs/04 ADR-001).
// The endpoint in tauri.conf.json points at the GitHub release manifest; the last
// check time is kept in the settings file (D-070) with localStorage as a cache.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { uiPrefs } from "./uiprefs.svelte";

const LAST_CHECK_KEY = "dayz-launcher.update-check";
const AUTO_CHECK_INTERVAL_MS = 24 * 3600 * 1000;

class Updates {
  state = $state<"idle" | "checking" | "none" | "available" | "downloading" | "ready" | "error">("idle");
  version = $state<string | null>(null);
  notes = $state<string | null>(null);
  error = $state<string | null>(null);
  progress = $state(0);
  #update: Update | null = null;

  /** Silent startup check, at most once a day. */
  async autoCheck() {
    let last = (await uiPrefs.ready).lastUpdateCheckMs ?? 0;
    try {
      last = Math.max(last, Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0));
    } catch {
      /* storage unavailable */
    }
    if (Date.now() - last < AUTO_CHECK_INTERVAL_MS) return;
    await this.checkNow(true);
  }

  async checkNow(silent = false) {
    this.state = "checking";
    this.error = null;
    try {
      const u = await check({ timeout: 10_000 });
      const now = Date.now();
      uiPrefs.patch({ lastUpdateCheckMs: now });
      try {
        localStorage.setItem(LAST_CHECK_KEY, String(now));
      } catch {
        /* ignore */
      }
      if (u) {
        this.#update = u;
        this.version = u.version;
        this.notes = u.body ?? null;
        this.state = "available";
      } else {
        this.state = "none";
      }
    } catch (e) {
      this.error = silent ? null : String(e);
      this.state = silent ? "idle" : "error";
    }
  }

  async install() {
    const u = this.#update;
    if (!u) return;
    this.state = "downloading";
    let total = 0;
    let got = 0;
    try {
      await u.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") {
          got += ev.data.chunkLength;
          this.progress = total ? Math.round((100 * got) / total) : 0;
        } else if (ev.event === "Finished") this.progress = 100;
      });
      this.state = "ready";
      await relaunch();
    } catch (e) {
      this.error = String(e);
      this.state = "error";
    }
  }
}

export const updates = new Updates();

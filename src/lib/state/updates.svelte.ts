// In-app updates via tauri-plugin-updater (signed artifacts, docs/04 ADR-001).
// The endpoint in tauri.conf.json must point at the release JSON before this
// can succeed; until then `check()` fails and the UI says so honestly.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

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
    try {
      const last = Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0);
      if (Date.now() - last < AUTO_CHECK_INTERVAL_MS) return;
    } catch {
      /* storage unavailable */
    }
    await this.checkNow(true);
  }

  async checkNow(silent = false) {
    this.state = "checking";
    this.error = null;
    try {
      const u = await check({ timeout: 10_000 });
      try {
        localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
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

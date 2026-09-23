// In-app updates via tauri-plugin-updater (signed artifacts, docs/04 ADR-001).
// The endpoint in tauri.conf.json points at the GitHub release manifest; the last
// check time is kept in the settings file (D-070) with localStorage as a cache.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { uiPrefs } from "./uiprefs.svelte";
import { describe, logError, logInfo, logWarn } from "../log";

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
        logInfo("update", `version ${u.version} is available`);
      } else {
        this.state = "none";
      }
    } catch (e) {
      this.error = silent ? null : String(e);
      this.state = silent ? "idle" : "error";
      logWarn("update", `check failed: ${describe(e)}`);
    }
  }

  async install() {
    const u = this.#update;
    if (!u) return;
    this.state = "downloading";
    logInfo("update", `installing ${u.version}`);
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
    } catch (e) {
      // Back to "available", not "error" (D-184): the update is still there and still
      // installable, and the error card offered only "Check for updates", which is
      // not what failed. The message says which step it was.
      this.error = `Could not install ${u.version}: ${describe(e)}`;
      this.state = "available";
      this.progress = 0;
      logWarn("update", `install of ${u.version} failed: ${describe(e)}`);
      return;
    }
    // Outside the try: the installer has already run by now, so a failure here is a
    // failure to *restart*, and reporting it as "could not install 0.1.26" sent the
    // user to install an update that is on disk already (D-194).
    try {
      await relaunch();
    } catch (e) {
      this.error = `${u.version} is installed; the launcher could not restart itself (${describe(e)}). Close it and start it again.`;
      logWarn("update", `relaunch after ${u.version} failed: ${describe(e)}`);
    }
  }
}

export const updates = new Updates();

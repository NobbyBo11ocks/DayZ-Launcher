// In-app updates via tauri-plugin-updater (signed artifacts, docs/04 ADR-001).
// The endpoint in tauri.conf.json points at the GitHub release manifest; the last
// check time is kept in the settings file (D-070) with localStorage as a cache.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { uiPrefs } from "./uiprefs.svelte";
import { describe, logInfo, logWarn } from "../log";

const LAST_CHECK_KEY = "dayz-launcher.update-check";
const AUTO_CHECK_INTERVAL_MS = 24 * 3600 * 1000;
/** The whole download, not a gap between chunks (reqwest's `timeout`): 7.5 MB in
 *  30 minutes is 4 KB/s, slower than any connection that can play DayZ. */
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000;

class Updates {
  state = $state<"idle" | "checking" | "none" | "available" | "downloading" | "ready" | "error">("idle");
  version = $state<string | null>(null);
  error = $state<string | null>(null);
  progress = $state(0);
  #update: Update | null = null;

  #autoRun: Promise<void> | null = null;

  /** Silent startup check, at most once a day. One at a time and only from a settled
   *  state: two calls close together ran two checks, and one during a download turned
   *  "downloading" back into "checking" and then "available", putting the Install
   *  button back beside a download still running (D-281). */
  autoCheck(): Promise<void> {
    return (this.#autoRun ??= this.#autoCheck().finally(() => (this.#autoRun = null)));
  }

  async #autoCheck() {
    const settled = () => this.state === "idle" || this.state === "none";
    if (!settled()) return;
    await uiPrefs.ready;
    // The file's current value, not the snapshot taken at start-up, which never saw
    // this session's own checks (D-281).
    let last = uiPrefs.current?.lastUpdateCheckMs ?? 0;
    try {
      last = Math.max(last, Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0));
    } catch {
      /* storage unavailable */
    }
    if (Date.now() - last < AUTO_CHECK_INTERVAL_MS || !settled()) return;
    await this.checkNow(true);
  }

  /** The start-up check again when the window comes back into focus, so a launcher left
   *  open learns of a release without a restart; still at most once a day, and only from
   *  a settled state, never over an offer, a download or an error on screen (D-280). */
  focusCheck() {
    if (this.state === "idle" || this.state === "none") void this.autoCheck();
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
      // The handle a previous check left is replaced, and released (D-281).
      if (this.#update && this.#update !== u) void this.#update.close().catch(() => {});
      this.#update = u;
      if (u) {
        this.version = u.version;
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
    // A second click, or a second confirmation, while one install runs (D-281).
    if (!this.#update || this.state === "downloading" || this.state === "ready") return;
    this.state = "downloading";
    // A retry must not keep the last attempt's error next to its progress (D-279).
    this.error = null;
    this.progress = 0;
    // The offer can be days old, and only the newest release is kept (D-274): once the
    // next one shipped, the one offered here was deleted and every retry was a 404.
    // Ask again first and install whatever is newest now; the fresh handle also
    // replaces one a failed install may have left invalid (D-279).
    try {
      const fresh = await check({ timeout: 10_000 });
      void this.#update?.close().catch(() => {});
      this.#update = fresh;
      this.version = fresh?.version ?? null;
    } catch {
      /* the offer in hand stands; its own download says what is wrong */
    }
    const u = this.#update;
    if (!u) {
      this.state = "none";
      return;
    }
    logInfo("update", `installing ${u.version}`);
    let total = 0;
    let got = 0;
    // Download, then install, as two steps: a failure in the first leaves the launcher as
    // it was, one in the second does not (below, D-280).
    try {
      await u.download(
        (ev) => {
          if (ev.event === "Started") total = ev.data.contentLength ?? 0;
          else if (ev.event === "Progress") {
            got += ev.data.chunkLength;
            this.progress = total ? Math.round((100 * got) / total) : 0;
          } else if (ev.event === "Finished") this.progress = 100;
        },
        // The download had no limit: one that stalled without closing sat at
        // "Downloading… N%" for good, with the Check button disabled (D-279).
        { timeout: DOWNLOAD_TIMEOUT_MS },
      );
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
    try {
      // On Windows this starts the setup and ends the process: it returns only when
      // the setup could not be started.
      await u.install();
      this.state = "ready";
    } catch (e) {
      // Before it starts the setup, the updater hides the window and closes the app's
      // resources, the Steam session with them, and it does not undo that when Windows
      // refuses the setup (an antivirus holding the unsigned file, for one): the launcher
      // was left running with no window and no Steam (tauri-plugin-updater 2.12.0,
      // D-279). The window comes back with what happened and what to do (D-280).
      this.state = "error";
      this.error = `Could not start the ${u.version} setup (${describe(e)}). Restart the launcher to use it again.`;
      logWarn("update", `the ${u.version} setup did not start: ${describe(e)}`);
      try {
        const w = getCurrentWindow();
        await w.show();
        await w.setFocus();
      } catch {
        /* the message is there for whenever the window is next shown */
      }
      return;
    }
    // Outside the try: the installer has already run by now, so a failure here is a
    // failure to *restart*, and reporting it as "could not install 0.1.26" sent the
    // user to install an update that is on disk already (D-194).
    try {
      await relaunch();
    } catch (e) {
      // Not "ready" any more: Settings renders "Installed, restarting…" for that
      // state, which would sit next to this message for ever (D-197).
      this.state = "error";
      this.error = `${u.version} is installed; the launcher could not restart itself (${describe(e)}). Close it and start it again.`;
      logWarn("update", `relaunch after ${u.version} failed: ${describe(e)}`);
    }
  }
}

export const updates = new Updates();

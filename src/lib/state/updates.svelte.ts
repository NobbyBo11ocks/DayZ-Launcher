// In-app updates via tauri-plugin-updater (signed artifacts, docs/04 ADR-001).
// The endpoint in tauri.conf.json points at the GitHub release manifest; the last
// check time is kept in the settings file (D-070) with localStorage as a cache.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { uiPrefs } from "./uiprefs.svelte";
import { describe, logInfo, logWarn } from "../log";

const LAST_CHECK_KEY = "dayz-launcher.update-check";
/** How long after a check coming back to the window asks again. It was a day for the
 *  start as well, and releases ship several times a day: most starts never learnt of one
 *  until "Check for updates" was pressed (user report, D-300). A start always asks now;
 *  the check is one small request for the release manifest. */
const FOCUS_CHECK_INTERVAL_MS = 3600 * 1000;
/** The whole download, not a gap between chunks (reqwest's `timeout`): 7.5 MB in
 *  30 minutes is 4 KB/s, slower than any connection that can play DayZ. */
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000;
/** A download with no data for this long is given up on (row 14, F5). */
const DOWNLOAD_STALL_MS = 60_000;
/** How the updater's HTTP client says there was no connection: reqwest's text for a
 *  refused, unresolved or timed-out request (S-111). */
const OFFLINE_ERROR = /error sending request|timed out|dns error|connection (?:refused|reset|closed)/i;
const CHECK_OFFLINE = "Could not reach GitHub to check for updates. Check your connection and try again.";

class Updates {
  state = $state<"idle" | "checking" | "none" | "available" | "downloading" | "ready" | "error">("idle");
  version = $state<string | null>(null);
  error = $state<string | null>(null);
  progress = $state(0);
  #update: Update | null = null;
  /** The error on screen came from a check, not from an install: a failed "Check for
   *  updates" held every later automatic check off for the session, and its message
   *  never cleared by itself (row 14, F13). */
  #checkFailed = false;
  /** Bumped when a download is given up on, so its late progress is not shown. */
  #downloadGen = 0;

  #autoRun: Promise<void> | null = null;

  /** Nothing in hand to lose: no offer, no download, no install error on screen. */
  #settled() {
    return this.state === "idle" || this.state === "none" || (this.state === "error" && this.#checkFailed);
  }

  /** Silent check, at every start (D-300), and on coming back to the window once
   *  `FOCUS_CHECK_INTERVAL_MS` has passed. One at a time and only from a settled state:
   *  two calls close together ran two checks, and one during a download turned
   *  "downloading" back into "checking" and then "available", putting the Install button
   *  back beside a download still running (D-281). */
  autoCheck(minGapMs = 0): Promise<void> {
    return (this.#autoRun ??= this.#autoCheck(minGapMs).finally(() => (this.#autoRun = null)));
  }

  async #autoCheck(minGapMs: number) {
    const settled = () => this.#settled();
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
    if (Date.now() - last < minGapMs || !settled()) return;
    await this.checkNow(true);
  }

  /** The check again when the window comes back into focus, so a launcher left open
   *  learns of a release without a restart; at most once an hour, and only from a
   *  settled state, never over an offer, a download or an error on screen (D-280,
   *  D-300). */
  focusCheck() {
    if (this.#settled()) void this.autoCheck(FOCUS_CHECK_INTERVAL_MS);
  }

  async checkNow(silent = false) {
    // A silent check over a failed one keeps its message until it has an answer.
    const shown = this.state === "error" ? this.error : null;
    this.state = "checking";
    this.error = null;
    this.#checkFailed = false;
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
      // No connection is said in words; the request's own text, with its URL, stays in
      // the log below (row 14, F6, approved).
      const message = silent ? shown : OFFLINE_ERROR.test(String(e)) ? CHECK_OFFLINE : String(e);
      this.error = message;
      this.state = message ? "error" : "idle";
      this.#checkFailed = !!message;
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
    // The 30-minute limit is on the whole request, and the plugin offers no limit on the
    // gap between chunks: a download that stopped moving sat at "Downloading… N%" with
    // Check disabled for up to half an hour (row 14, F5). A minute without data gives up
    // here; the request runs on until its own limit, and what it brings back is never
    // installed, nor its progress shown.
    const gen = ++this.#downloadGen;
    let lastData = Date.now();
    // A plain string: `describe` prints an Error as "Error: …" inside the message.
    let stalled: ((reason: string) => void) | undefined;
    const watchdog = setInterval(() => {
      if (Date.now() - lastData > DOWNLOAD_STALL_MS) stalled?.("nothing arrived for a minute");
    }, 5_000);
    // Download, then install, as two steps: a failure in the first leaves the launcher as
    // it was, one in the second does not (below, D-280).
    try {
      await Promise.race([
        u.download(
          (ev) => {
            if (gen !== this.#downloadGen) return;
            lastData = Date.now();
            if (ev.event === "Started") total = ev.data.contentLength ?? 0;
            else if (ev.event === "Progress") {
              got += ev.data.chunkLength;
              this.progress = total ? Math.round((100 * got) / total) : 0;
            } else if (ev.event === "Finished") this.progress = 100;
          },
          // The download had no limit: one that stalled without closing sat at
          // "Downloading… N%" for good, with the Check button disabled (D-279).
          { timeout: DOWNLOAD_TIMEOUT_MS },
        ),
        new Promise<never>((_, reject) => (stalled = reject)),
      ]);
    } catch (e) {
      this.#downloadGen++;
      // Back to "available", not "error" (D-184): the update is still there and still
      // installable, and the error card offered only "Check for updates", which is
      // not what failed. The message says which step it was.
      this.error = `Could not install ${u.version}: ${describe(e)}`;
      this.state = "available";
      this.progress = 0;
      logWarn("update", `install of ${u.version} failed: ${describe(e)}`);
      return;
    } finally {
      clearInterval(watchdog);
    }
    try {
      // On Windows this starts the setup and ends the process: it returns only when
      // the setup could not be started.
      await u.install();
      this.state = "ready";
    } catch (e) {
      // The setup is written to %TEMP% first, and only then does the updater hide the
      // window and close the app's resources (tauri 2.11.6 `cleanup_before_exit` hides
      // every window on Windows; S-111). A window still showing is a setup that could not
      // even be saved — a full disk — with the launcher and its update intact, so Install
      // is offered again instead of a restart (row 14, F13/H13c, approved).
      let saved = true;
      try {
        saved = !(await getCurrentWindow().isVisible());
      } catch {
        /* the old message is the safe one */
      }
      if (!saved) {
        this.state = "available";
        this.error = `Could not save the ${u.version} setup (${describe(e)}). Press Install and restart to try again.`;
        logWarn("update", `the ${u.version} setup could not be saved: ${describe(e)}`);
        return;
      }
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

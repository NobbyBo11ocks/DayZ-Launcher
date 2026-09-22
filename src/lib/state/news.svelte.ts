// DayZ news (D-099, D-100): Steam's feed for app 221100, cached by the backend for
// an instant first paint, refreshed every 30 minutes while the launcher runs.
// Official update posts that arrive after the user last looked raise an in-app
// toast and, when the window is not focused, a Windows notification. The store
// also holds the signed-in user's avatar for the welcome header.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { SvelteMap } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import type { NewsCached, NewsItem } from "../types";

const REFRESH_MS = 30 * 60_000;
/** On the very first run, only this many of the newest official posts count as unread. */
const FIRST_RUN_UNREAD = 5;

export type NewsAlert = { gid: string; title: string; url: string; at: number };
/** `updates`: official game-update posts (default); `official`: every Bohemia post; `press`: plus third-party feeds. */
export type NewsView = "updates" | "official" | "press";

async function notifyIfUnfocused(n: NewsItem) {
  try {
    if (await getCurrentWindow().isFocused()) return;
    if (!(await isPermissionGranted()) && (await requestPermission()) !== "granted") return;
    sendNotification({ title: "DayZ update", body: n.title });
  } catch {
    /* notifications unavailable; the in-app toast remains */
  }
}

class NewsStore {
  items = $state<NewsItem[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Unix seconds of the fetch behind `items`. */
  fetchedAt = $state<number | null>(null);
  /** Unix seconds of the newest official post the user has looked at (settings.json). */
  seen = $state(0);
  /** The seen mark when the current visit to the tab began: newer posts keep their "New" pill. */
  visitSeen = $state(0);
  /** The landing page shows the latest game updates only unless the user widens it (D-101). */
  view = $state<NewsView>("updates");
  /** Update posts not yet dismissed, newest last (at most three). */
  alerts = $state<NewsAlert[]>([]);
  /** PNG data URL of the signed-in user's Steam avatar, once Steam has it. */
  /** Object URLs of downscaled post pictures by gid (D-111); the backend caches the files. */
  thumbs = new SvelteMap<string, string>();
  #thumbPending = new Set<string>();
  #thumbFailed = new Set<string>();
  #started = false;
  #visiting = false;

  list = $derived(this.items.filter((n) => (this.view === "press" ? true : n.official && (this.view === "official" || n.update))));
  /** Official posts newer than the seen mark: the rail badge. */
  unread = $derived(this.items.filter((n) => n.official && n.date > this.seen).length);

  async start() {
    if (this.#started) return;
    this.#started = true;
    try {
      const c = await invoke<NewsCached>("news_cached");
      this.items = c.items;
      this.fetchedAt = c.fetchedAt;
    } catch {
      /* nothing cached yet */
    }
    const u = await uiPrefs.ready;
    this.seen = u.newsSeen ?? 0;
    await this.refresh();
    setInterval(() => void this.refresh(), REFRESH_MS);
  }

  async refresh() {
    if (this.loading) return;
    this.loading = true;
    const before = this.seen;
    const known = new Set(this.items.map((n) => n.gid));
    try {
      const c = await invoke<NewsCached>("news_fetch");
      this.items = c.items;
      this.fetchedAt = c.fetchedAt;
      this.error = null;
      if (before === 0) {
        // First run: the newest few are "new", not the whole archive.
        const official = c.items.filter((n) => n.official);
        const mark = official[FIRST_RUN_UNREAD]?.date ?? 0;
        if (mark > 0) {
          this.seen = mark;
          if (this.#visiting && this.visitSeen === 0) this.visitSeen = mark;
          uiPrefs.patch({ newsSeen: mark });
        }
      } else {
        for (const n of c.items) {
          if (n.official && n.update && n.date > before && !known.has(n.gid)) this.announce(n);
        }
      }
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  private announce(n: NewsItem) {
    this.alerts = [...this.alerts.filter((a) => a.gid !== n.gid), { gid: n.gid, title: n.title, url: n.url, at: Date.now() }].slice(-3);
    void getCurrentWindow()
      .requestUserAttention(UserAttentionType.Informational)
      .catch(() => {});
    void notifyIfUnfocused(n);
  }

  dismissAlert(gid: string) {
    this.alerts = this.alerts.filter((a) => a.gid !== gid);
  }

  /** The tab is on screen: freeze the "New" boundary for this visit. */
  beginVisit() {
    this.#visiting = true;
    this.visitSeen = this.seen;
    // At start-up the tab mounts before the seen mark is read from the file.
    void uiPrefs.ready.then(() => {
      if (this.#visiting && this.visitSeen === 0) this.visitSeen = this.seen;
    });
  }

  endVisit() {
    this.#visiting = false;
  }

  /** The user looked at the list: nothing currently shown counts as new any more. */
  markSeen() {
    const newest = this.items.reduce((m, n) => (n.official && n.date > m ? n.date : m), 0);
    if (newest > this.seen) {
      this.seen = newest;
      uiPrefs.patch({ newsSeen: newest });
    }
  }

  /**
   * What to show as a post's picture (D-111). A card prefers the small YouTube
   * preview when the post has a video (one small fetch, no thumbnail work); the
   * featured post and video-less posts use the backend's 640 px thumbnail, which
   * arrives asynchronously (null until then). Steam's originals are 3840×2160.
   */
  thumbUrl(n: NewsItem, featured = false): string | null {
    const yt = n.video ? `https://i.ytimg.com/vi/${n.video}/${featured ? "hqdefault" : "mqdefault"}.jpg` : null;
    if (!featured && yt) return yt;
    if (!n.image) return yt;
    const have = this.thumbs.get(n.gid);
    if (have) return have;
    if (this.#thumbFailed.has(n.gid)) return yt;
    if (!this.#thumbPending.has(n.gid)) {
      this.#thumbPending.add(n.gid);
      // Cards draw at ~340 px, the featured picture at roughly twice that; asking
      // for 640 px everywhere decoded ~920 KB per card in the WebView (D-160).
      void invoke<ArrayBuffer>("news_thumb", { gid: n.gid, url: n.image, max: featured ? 640 : 360 })
        .then((buf) => {
          const old = this.thumbs.get(n.gid);
          this.thumbs.set(n.gid, URL.createObjectURL(new Blob([buf], { type: "image/jpeg" })));
          if (old) URL.revokeObjectURL(old);
        })
        .catch(() => this.#thumbFailed.add(n.gid))
        .finally(() => this.#thumbPending.delete(n.gid));
    }
    return null;
  }

}

export const news = new NewsStore();

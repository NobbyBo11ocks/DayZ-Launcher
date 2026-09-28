// DayZ news (D-099, D-100): Steam's feed for app 221100, cached by the backend for
// an instant first paint, refreshed every 30 minutes while the launcher runs.
// Official update posts that arrive after the user last looked raise an in-app
// toast and, when the window is not focused, a Windows notification.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { SvelteMap } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import type { NewsCached, NewsItem } from "../types";

const REFRESH_MS = 30 * 60_000;
/** The least time between two retries of a failed fetch (row 14, F6). */
const RETRY_GAP_MS = 60_000;
/** On the very first run, only this many of the newest official posts count as unread. */
const FIRST_RUN_UNREAD = 5;

export type NewsAlert = { gid: string; title: string; url: string };
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
  /** Unix seconds of the newest official post the user has looked at (settings.json). */
  seen = $state(0);
  /** The seen mark when the current visit to the tab began: newer posts keep their "New" pill. */
  visitSeen = $state(0);
  /** The file's mark has been adopted; until then nothing counts as read (D-245). */
  seenLoaded = $state(false);
  /** The stored posts have been applied, or could not be read: until then the page has
   *  nothing to call empty. It said "No posts to show." from the first frame until they
   *  came, through a slow settings read as well (row 24). */
  loaded = $state(false);
  /** The landing page shows the latest game updates only unless the user widens it (D-101). */
  view = $state<NewsView>("updates");
  /** Update posts not yet dismissed, newest last (at most three). */
  alerts = $state<NewsAlert[]>([]);
  /** Object URLs of downscaled post pictures by `gid:size` (D-111, D-256); the backend
   *  caches the files. */
  thumbs = new SvelteMap<string, string>();
  #thumbPending = new Set<string>();
  #thumbFailed = new Set<string>();
  #started = false;
  /** Which `start()` is current: a switch-off and on during the first fetch left two
   *  of them running, each arming its own timer, and `stop()` cleared one (D-240). */
  #run = 0;
  #timer: ReturnType<typeof setInterval> | undefined;
  #visiting = false;

  list = $derived(this.items.filter((n) => (this.view === "press" ? true : n.official && (this.view === "official" || n.update))));
  /** Official posts newer than the seen mark: the rail badge. */
  unread = $derived(this.items.filter((n) => n.official && n.date > this.seen).length);

  async start() {
    if (this.#started) return;
    this.#started = true;
    const run = ++this.#run;
    // The stored posts are asked for at once. Asked for after the settings read, they
    // queued behind the whole server list, whose read holds the cache lock for 65–117 ms
    // at 40 000–71 000 rows: the order D-284 wanted for the start page never took effect
    // (row 24).
    const stored = invoke<NewsCached>("news_cached").catch(() => null);
    // The file's mark first, and only then the cached posts: with the order the other
    // way round, `markSeen` moved the mark to the newest cached post before the file
    // was read, or `beginVisit`'s fallback froze the boundary at 0 — every post
    // "New", or none, at every start (D-245).
    const u = await uiPrefs.ready;
    // `ready` holds the file as it was at start-up: taken alone, a switch-off and on
    // later in the session put back a mark the user had since moved, and posts read
    // meanwhile counted as unread again (D-240).
    this.seen = Math.max(this.seen, u.newsSeen ?? 0);
    this.seenLoaded = true;
    const c = await stored;
    if (!this.#started || run !== this.#run) return;
    if (c) this.#setItems(c.items);
    this.loaded = true;
    // A switch-off between here and now must not be overtaken by this first fetch.
    if (!this.#started || run !== this.#run) return;
    await this.refresh();
    // `stop()` may have run while that was in flight; arming now would leave an
    // interval nothing can clear (D-197), and so would a newer `start()`.
    if (!this.#started || run !== this.#run) return;
    this.#timer = setInterval(() => void this.refresh(), REFRESH_MS);
    window.addEventListener("online", this.#retryIfFailed);
    window.addEventListener("focus", this.#retryIfFailed);
  }

  /** A failed fetch is tried again when the connection or the window comes back, at most
   *  once a minute: started offline, the page showed its error for up to half an hour
   *  after the connection returned, with no Refresh to press (D-122; row 14, F6). */
  #lastRetry = 0;
  #retryIfFailed = () => {
    if (!this.error || !this.#started || Date.now() - this.#lastRetry < RETRY_GAP_MS) return;
    this.#lastRetry = Date.now();
    void this.refresh();
  };

  /** Stops fetching for the rest of the session; `start()` arms it again. */
  stop() {
    this.#started = false;
    window.removeEventListener("online", this.#retryIfFailed);
    window.removeEventListener("focus", this.#retryIfFailed);
    if (this.#timer !== undefined) {
      clearInterval(this.#timer);
      this.#timer = undefined;
    }
    // Nothing should be left pointing at a page that is no longer in the sidebar: the
    // alerts, and the list with every picture's object URL, which were kept for the rest
    // of the session; `start()` reads the stored copy again (row 24).
    this.alerts = [];
    this.#setItems([]);
    this.#thumbFailed.clear();
    this.loaded = false;
  }

  async refresh() {
    // The switch is checked here as well as at the call sites: this is the one place
    // every fetch passes through, so it is the honest place to enforce the promise.
    if (!this.#started || this.loading) return;
    this.loading = true;
    const before = this.seen;
    const known = new Set(this.items.map((n) => n.gid));
    try {
      const c = await invoke<NewsCached>("news_fetch");
      // Switched off while that was in flight: no toast, no taskbar flash and no
      // Windows notification for a page that has left the sidebar (D-185, D-256).
      if (!this.#started) return;
      this.#setItems(c.items);
      this.error = null;
      // Pictures that failed are asked for again: one that failed while offline, or on
      // a slow line, stayed missing for the whole session (row 14, H10).
      this.#thumbFailed.clear();
      if (before === 0) {
        // First run: the newest few are "new", not the whole archive.
        const official = c.items.filter((n) => n.official);
        const mark = official[FIRST_RUN_UNREAD]?.date ?? 0;
        if (mark > 0) {
          this.seen = mark;
          if (this.#visiting && this.visitSeen === 0) this.visitSeen = mark;
          // Only over a mark that was actually read: when the settings file could not
          // be read (D-112), 0 is not "never looked", and this wrote an older date over
          // the file's newer one (D-194, D-256).
          if (uiPrefs.readOk) uiPrefs.patch({ newsSeen: mark });
        }
      } else {
        this.#announce(c.items.filter((n) => n.official && n.update && n.date > before && !known.has(n.gid)));
      }
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  /** New update posts from one fetch: the newest three as toasts, newest last, with one
   *  taskbar flash and one Windows notification, for the newest. Announced one by one in
   *  the feed's order (newest first), the three kept were the oldest, the newest post was
   *  the one dropped, and each asked for its own flash and notification (row 24). */
  #announce(fresh: NewsItem[]) {
    if (fresh.length === 0) return;
    const newest = [...fresh].sort((a, b) => b.date - a.date).slice(0, 3);
    const gids = new Set(newest.map((n) => n.gid));
    this.alerts = [...this.alerts.filter((a) => !gids.has(a.gid)), ...newest.reverse().map((n) => ({ gid: n.gid, title: n.title, url: n.url }))].slice(-3);
    void getCurrentWindow()
      .requestUserAttention(UserAttentionType.Informational)
      .catch(() => {});
    void notifyIfUnfocused(newest[newest.length - 1]!);
  }

  dismissAlert(gid: string) {
    this.alerts = this.alerts.filter((a) => a.gid !== gid);
  }

  /** The tab is on screen: freeze the "New" boundary for this visit. */
  beginVisit() {
    this.#visiting = true;
    this.visitSeen = this.seen;
    // At start-up the tab mounts before the seen mark is read from the file. The
    // file's own mark, not `seen`: whichever promise reaction runs first, `seen`
    // cannot have moved past it while `markSeen` waits for `seenLoaded` (D-245).
    void uiPrefs.ready.then((u) => {
      if (this.#visiting && this.visitSeen === 0) this.visitSeen = Math.max(this.seen, u.newsSeen ?? 0);
    });
  }

  endVisit() {
    this.#visiting = false;
  }

  /** The user looked at the list: nothing currently shown counts as new any more. */
  markSeen() {
    // Read inside the News effect, so the effect re-runs when the mark arrives.
    if (!this.seenLoaded) return;
    const newest = this.items.reduce((m, n) => (n.official && n.date > m ? n.date : m), 0);
    if (newest > this.seen) {
      this.seen = newest;
      uiPrefs.patch({ newsSeen: newest });
    }
  }

  /** The feed replaced, and the pictures of posts that left it released: each is an object
   *  URL holding its bytes until revoked, and they were kept for the whole run (D-282). */
  #setItems(items: NewsItem[]) {
    this.items = items;
    const keep = new Set(items.map((n) => n.gid));
    const gone = [...this.thumbs.keys()].filter((k) => !keep.has(k.slice(0, k.lastIndexOf(":"))));
    for (const k of gone) {
      const url = this.thumbs.get(k);
      if (url) URL.revokeObjectURL(url);
      this.thumbs.delete(k);
    }
    for (const k of [...this.#thumbFailed]) if (!keep.has(k.slice(0, k.lastIndexOf(":")))) this.#thumbFailed.delete(k);
  }

  /**
   * What to show as a post's picture (D-111). A card prefers the small YouTube
   * preview when the post has a video (one small fetch, no thumbnail work); otherwise
   * the backend makes a thumbnail, 640 px for the featured post and 360 px for a card,
   * which arrives asynchronously (null until then). Steam's originals are 3840×2160.
   */
  thumbUrl(n: NewsItem, featured = false): string | null {
    const yt = n.video ? `https://i.ytimg.com/vi/${n.video}/${featured ? "hqdefault" : "mqdefault"}.jpg` : null;
    if (!featured && yt) return yt;
    if (!n.image) return yt;
    // Cards draw at ~340 px, the featured picture at roughly twice that; asking for
    // 640 px everywhere decoded ~920 KB per card in the WebView (D-164). Keyed by size
    // as well: keyed by post alone, a card that became the featured post reused its
    // 360 px picture in the 640 px hero, blurred (D-256).
    const max = featured ? 640 : 360;
    const key = `${n.gid}:${max}`;
    const have = this.thumbs.get(key);
    if (have) return have;
    if (this.#thumbFailed.has(key)) return yt;
    if (!this.#thumbPending.has(key)) {
      this.#thumbPending.add(key);
      void invoke<ArrayBuffer>("news_thumb", { gid: n.gid, url: n.image, max })
        .then((buf) => {
          // Switched off meanwhile: nothing is kept for a page that has left the sidebar.
          if (!this.#started) return;
          const old = this.thumbs.get(key);
          this.thumbs.set(key, URL.createObjectURL(new Blob([buf], { type: "image/jpeg" })));
          if (old) URL.revokeObjectURL(old);
        })
        .catch(() => this.#thumbFailed.add(key))
        .finally(() => this.#thumbPending.delete(key));
    }
    return null;
  }

}

export const news = new NewsStore();

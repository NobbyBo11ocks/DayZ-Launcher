// Does each front-end test guard its fix? For every entry below the stores are built with
// that one fix reverted (in a throw-away build; the sources are only read), the test that
// claims it is run, and the test must FAIL. Not part of CI: run it after reworking a store,
// `node tests/front/mutations.mjs [filter]`. An entry whose text is no longer in the source
// is reported as "stale" (the code moved; rewrite the entry), never as a pass.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build, ROOT } from "../../tools/front_test.mjs";

const S = "src/lib/state/servers.svelte.ts";
const U = "src/lib/state/uiprefs.svelte.ts";
const P = "src/lib/state/prefs.svelte.ts";
const UP = "src/lib/state/updates.svelte.ts";
const N = "src/lib/state/news.svelte.ts";
const T = "src/lib/types.ts";
const C = "src/lib/changes.ts";
const VD = "src/lib/verdict.ts";
const AV = "src/lib/avatar.ts";
const PN = "src/lib/pane.ts";
const CL = "src/lib/state/closing.ts";
const ES = "src/lib/escape.ts";
const EX = "src/lib/external.ts";
const NG = "src/lib/newsgrid.ts";

/** [test file, the test's name as it starts, the fix, the source, text, replacement, what the revert does] */
const MUTATIONS = [
  // servers.test.mjs
  ["servers", "one bad row-stream message", "D-302 (4)", S,
    'stream.onmessage = (m) => {\n      try {\n        this.#onRows(m);\n      } catch (e) {\n        logError("rows", `a ${m.kind} message failed: ${describe(e)}`);\n      }\n    };',
    "stream.onmessage = (m) => this.#onRows(m);", "no try/catch around the row-stream handler"],
  ["servers", "the shared error line comes down", "D-302 (9)", S,
    "    if (this.#errorBy?.from !== from) return;\n    if (this.error === this.#errorBy.message) this.error = null;",
    "    return;", "a success never takes the line down (before row 14)"],
  ["servers", "the shared error line comes down", "D-302 (9)", S,
    "    if (this.#errorBy?.from !== from) return;", "    if (!this.#errorBy) return;", "any action's success takes down another's line"],
  ["servers", '"Refreshing', "D-306 (7)", S,
    "if (!dzsa && r.steamEmpty != null) this.#listedThisRefresh?.add(r.id);", "if (!dzsa) this.#listedThisRefresh?.add(r.id);", "rows Steam did not list are counted"],
  ["servers", '"Refreshing', "D-306 (7)", S,
    'if (d.source === "steam" && !(d.rejected && d.reason === "busy")) this.#listedThisRefresh = null;', "this.#listedThisRefresh = null;", "a busy press or a DZSA import ends the count"],
  ["servers", "the connection notice stays up", "D-307 (5)", S,
    '    window.addEventListener("online", () => (this.#osOffline = false));',
    '    window.addEventListener("online", () => ((this.#osOffline = false), (this.#hostNetDown = false)));', "one flag again: Windows' online clears the host's finding"],
  ["servers", "an import with no official file", "D-307 (4)", S,
    "      if (!r.missing) {\n        await this.loadFavourites();\n        this.favouritesUnread = false;\n      }",
    "      if (!r.missing) await this.loadFavourites();\n      this.favouritesUnread = false;", "any import clears 'could not be read'"],
  ["servers", "'not scanned yet' counts", "D-276", S,
    "      if (!r.tags.modded) continue;\n      if (r.steamEmpty === true && r.players > 0) continue; // rule R0\n      if (!(r.verifiedPlayers != null ? r.verifiedPlayers > 0 : r.steamEmpty === false && r.players > 0)) continue;\n      // The reactive maps last, for the rows that can count: read first, they were asked\n      // for every modded farm row and were most of a flush with a mod picked, 31 of 18 ms\n      // at 105 000 rows (row 19).\n      if (this.modsByServer.has(r.id) || this.modsUnreadable.has(r.id)) continue;\n      n++;",
    "      if (r.tags.modded && trustedPlayers(r) > 0 && !this.modsByServer.has(r.id)) n++;", "counted by the store's head-count again (before D-276)"],
  ["servers", "'not scanned yet' counts", "D-310", S,
    "      if (!r.tags.modded) continue;\n      if (r.steamEmpty === true && r.players > 0) continue; // rule R0\n      if (!(r.verifiedPlayers != null ? r.verifiedPlayers > 0 : r.steamEmpty === false && r.players > 0)) continue;\n      // The reactive maps last, for the rows that can count: read first, they were asked\n      // for every modded farm row and were most of a flush with a mod picked, 31 of 18 ms\n      // at 105 000 rows (row 19).\n      if (this.modsByServer.has(r.id) || this.modsUnreadable.has(r.id)) continue;\n      n++;",
    "      if (!r.tags.modded || this.modsByServer.has(r.id) || this.modsUnreadable.has(r.id)) continue;\n      if (r.steamEmpty === true && r.players > 0) continue; // rule R0\n      if (r.verifiedPlayers != null ? r.verifiedPlayers > 0 : r.steamEmpty === false && r.players > 0) n++;",
    "the reactive maps asked first, for every modded row (before D-310)"],
  ["servers", "the start-up status read", "D-281 (10)", S,
    "      if (this.#statusEvents === seen) this.steam = status;", "      this.steam = status;", "the snapshot always wins"],
  ["servers", "a Join never replaces", "D-295 F8", S,
    "    if (this.joiningId === null) this.joiningId = id;", "    this.joiningId = id;", "a Join replaces the open dialog"],
  ["servers", "a Join never replaces", "D-296 (5)", S,
    "      if (joinGone !== null) this.error = `${joinGone} dropped out of the server list, so its join was closed. Join it again from Recent or with Direct connect.`;",
    "", "the dialog closes without a word"],
  ["servers", "a row whose check cannot count", "D-281 (3)", S,
    '      const last = Math.max(r.verifiedAt ?? 0, this.#checkedAt.get(id) ?? 0);\n      const uncounted = r.verdict === "offline" || r.verdict === "unverifiable" || r.verdict === "synthetic";\n      return last === 0 || now - last > (uncounted ? UNCOUNTED_STALE_SECS : STALE_SECS);',
    "      return r.verifiedAt == null || now - r.verifiedAt > STALE_SECS;", "stale by `verifiedAt` alone, every two minutes (before D-281)"],
  ["servers", "a row whose check cannot count", "D-302 (3)", S,
    "    if (!navigator.onLine) return;\n    const now = Math.floor(Date.now() / 1000);", "    const now = Math.floor(Date.now() / 1000);", "checks sent while Windows is offline"],
  ["servers", "mod lists that arrive while", "D-281 (8)", S,
    "    if (this.#heldMods) {\n      this.#heldMods.push([list, names, failed]);\n      return;\n    }\n", "", "batches applied during the read, under the snapshot"],
  // refresh.test.mjs
  ["refresh", "a refresh Steam could not take", "D-302 (5)", S,
    '        if (this.#rearms < MAX_AUTO_REARMS) {\n          this.#rearms++;\n          if (d.reason === "no-answer") this.#rearmLater();\n          else this.#retryArmed = true;\n        }',
    "", "a failed refresh re-arms nothing (before row 14)"],
  ["refresh", "a refresh Steam could not take", "D-302 (5)", S,
    "        if (this.#rearms < MAX_AUTO_REARMS) {", "        if (true) {", "no limit on re-arms"],
  ["refresh", "a refresh Steam could not take", "D-236", S,
    "    if (this.steam?.initialized && !this.steam.idle && !this.steam.refreshing && (!this.#autoRefreshed || this.#retryArmed)) {",
    "    if (this.steam?.initialized && !this.steam.refreshing && (!this.#autoRefreshed || this.#retryArmed)) {", "the retry runs on a released session"],
  ["refresh", "a refresh Steam left unanswered", "D-307 (1)", S,
    '          if (d.reason === "no-answer") this.#rearmLater();\n          else this.#retryArmed = true;', "          this.#retryArmed = true;", "no-answer re-armed at once (before row 17)"],
  ["refresh", "the start-up refresh is spent only", "D-307 (1)", S,
    "      if (started) this.#autoRefreshed = true;", "      this.#autoRefreshed = started;", "a declined press clears the flag (before row 17)"],
  ["refresh", "the start-up refresh is spent only", "D-222", S,
    "      void this.refresh(false, false);", "      this.#autoRefreshed = true;\n      void this.refresh(false, false);", "spent before the worker answered (before D-222)"],
  // filters.test.mjs
  ["filters", "a saved filter set is taken key by key", "D-304 (8)", S,
    '  return {\n    ...r,\n    search: "",',
    '  return { ...d, ...(r as Partial<Filters>), search: "", map: ((r.map ?? d.map) as string).toLowerCase() };\n  return {\n    ...r,\n    search: "",',
    "taken as it came, map lower-cased (before row 15)"],
  ["filters", "typing a search before the settings file", "D-304 (7)", "src/lib/search.ts",
    '        servers.filters.search = "";\n        return;\n      }\n      timer = setTimeout(() => {\n        servers.filters.search = value;\n      }, delayMs);',
    '        servers.filters.search = "";\n        servers.saveFilters();\n        return;\n      }\n      timer = setTimeout(() => {\n        servers.filters.search = value;\n        servers.saveFilters();\n      }, delayMs);',
    "the search saves the filters (before row 15)"],
  ["filters", "typing a search before the settings file", "D-222", "src/lib/search.ts",
    "      timer = setTimeout(() => {\n        servers.filters.search = value;\n      }, delayMs);", "      servers.filters.search = value;", "no debounce"],
  ["filters", "typing a search before the settings file", "D-222", "src/lib/search.ts",
    '      if (value === "") {\n        servers.filters.search = "";\n        return;\n      }\n', "", "clearing waits for the debounce"],
  ["filters", "typing a search before the settings file", "D-195", "src/lib/maps.ts",
    '    s = [...new Set(parts)].join(" ");', "    s = key;", "maps found by their folder id only"],
  // uiprefs.test.mjs
  ["uiprefs", "preference writes go one at a time", "D-307 (3)", U,
    "    const before = this.#writing;\n    let finished!: () => void;\n    this.#writing = new Promise<void>((r) => (finished = r));\n    try {\n      await before;\n      await this.#write();\n    } finally {\n      finished();\n    }",
    "    await this.#write();", "two writes out at once (before row 17)"],
  ["uiprefs", "an answer standing in for an unreadable", "D-302 (12) H4", U,
    "        this.readOk = !s.unreadable && !s.reset;", "        this.readOk = true;", "any answer counts as a read (before row 14)"],
  ["uiprefs", "an answer standing in for an unreadable", "D-304 (7) F4", U,
    "      if (cur && this.readOk && stable(cur[k]) === stable(v)) continue;", "      if (cur && stable(cur[k]) === stable(v)) continue;", "a change matching the stand-in defaults is dropped"],
  ["uiprefs", "an answer standing in for an unreadable", "D-304 (7) F4", U,
    "    await this.ready;\n    // One write at a time.",
    "    const early = this.#pending;\n    this.#pending = {};\n    await this.ready;\n    this.#pending = { ...early, ...this.#pending };\n    // One write at a time.",
    "the changes taken before the read (before row 15)"],
  ["uiprefs", "a write that failed is tried again", "D-302 (12) H9", U,
    "      if (this.#retries < MAX_RETRIES) {\n        this.#retries++;\n        clearTimeout(this.#timer);\n        this.#timer = setTimeout(() => void this.flush(), RETRY_MS);\n      }",
    "", "no timed retry (before row 14)"],
  ["uiprefs", "a write that failed is tried again", "D-302 (12) H9", U,
    "      if (this.#retries < MAX_RETRIES) {", "      if (true) {", "retries without end"],
  // prefs.test.mjs
  ["prefs", "the start page and News follow", "D-304 (6) F3", P,
    '        try {\n          localStorage.setItem(NEWS_KEY, this.news ? "on" : "off");\n          localStorage.setItem(OPEN_KEY, this.openOn);\n        } catch {\n          /* storage unavailable */\n        }',
    "", "the start-up copy does not follow the file"],
  ["prefs", "the start page and News follow", "D-306 (5)", P,
    "        this.openOn = normOpen(u.openOn);\n", "", "the file's start page ignored"],
  ["prefs", "a damaged settings file set aside", "D-304 (6) F1", P,
    "      } else if (uiPrefs.health.reset) {\n        // A damaged file was set aside and its defaults are being written: they get this\n        // copy's choice, as theme and accent do below, or News came back at the next\n        // start with its feed, pictures and videos (row 15, F1).\n        uiPrefs.patch({ news: this.news, openOn: this.openOn });\n      }",
    "      }", "the new file keeps the defaults' News on"],
  ["prefs", "a damaged settings file set aside", "D-194", P,
    "      if (uiPrefs.readOk) {", "      if (true) {", "stand-in defaults taken as the file"],
  // updates.test.mjs
  ["updates", "a download that stops moving", "D-302 (7) F5", UP,
    "      if (Date.now() - lastData > DOWNLOAD_STALL_MS) stalled?.(DOWNLOAD_STALLED);", "", "no stall watchdog (before row 14)"],
  ["updates", "a download that stops moving", "D-302 (7) F5", UP,
    "            if (gen !== this.#downloadGen) return;\n", "", "an abandoned download's progress shown"],
  ["updates", "a download that stops moving", "D-281 (5)", UP,
    '    if (!this.#update || this.state === "downloading" || this.state === "ready") return;', "    if (!this.#update) return;", "a second Install starts a second download"],
  ["updates", "a download that stops moving", "D-306 (18)", UP,
    "      this.error =\n        why === DOWNLOAD_STALLED\n          ? `Could not download ${u.version}: nothing arrived for a minute. Press Install and restart to try again.`\n          : OFFLINE_ERROR.test(why)\n            ? `Could not download ${u.version}: no connection to GitHub. Press Install and restart to try again.`\n            : `Could not install ${u.version}. Press Install and restart to try again; the details are on the Logs page.`;",
    "      this.error = `Could not install ${u.version}: ${describe(e)}`;", "raw host text on screen (before row 16)"],
  ["updates", "the update check:", "D-304 (13) F5", UP,
    "    if (!(last <= Date.now())) last = 0;\n", "", "a future last-check time holds the check off"],
  ["updates", "the update check:", "D-303 (2)", UP,
    "      const message = silent ? shown : OFFLINE_ERROR.test(String(e)) ? CHECK_OFFLINE : String(e);", "      const message = silent ? shown : String(e);", "the request's own text on screen"],
  ["updates", "the update check:", "D-302 (7) F13", UP,
    '    return this.state === "idle" || this.state === "none" || (this.state === "error" && this.#checkFailed);', '    return this.state === "idle" || this.state === "none";', "a failed check holds the automatic ones off"],
  ["updates", "the update check:", "D-281 (5)", UP,
    "    const settled = () => this.#settled();\n    if (!settled()) return;", "    const settled = () => true;", "a check over a download"],
  // trust.test.mjs
  ["trust", "the trust rules", "docs/11 R0", T, "  isInflated(r) ||\n  r.verdict === \"inflated\" ||", '  r.verdict === "inflated" ||', "R0 rows not hidden"],
  ["trust", "the trust rules", "docs/11 R2-R5", T, '  r.verdict === "synthetic" ||\n  r.verdict === "offline" ||', "", "synthetic and offline rows not hidden"],
  ["trust", "the trust rules", "D-268", T, "  r.players > 127 ||\n", "", "no line above 127"],
  ["trust", "the trust rules", "D-233 R9", T, "  (r.verifiedPlayers == null && (r.bots ?? 0) > 0 && r.bots === r.players) ||\n", "", "the bots byte ignored"],
  ["trust", "the trust rules", "D-233 R10", T, "  r.clone === true ||\n", "", "name clones not hidden"],
  ["trust", "the trust rules", "D-233 R6", T,
    '  (r.verdict === "unverifiable" && !(r.steamEmpty === false && r.verifiedPlayers != null));', '  (r.verdict === "unverifiable" && r.steamEmpty !== false);', "Steam's vouch alone trusts a claim (before D-233)"],
  ["trust", "the trust rules", "D-233", T, "  if (r.steamEmpty === true && r.players === 0) return 0;\n", "", "an old head-count over a fresh 'empty'"],
  ["trust", "the trust rules", "D-233, D-237", T, '  if (r.verdict === "unverifiable" || r.verdict === "synthetic") return 0;\n', "", "claims sorted as counts"],
  ["trust", "the trust rules", "D-233", T,
    "r.tags.queue && r.tags.queue > 0 && trustedPlayers(r) >= r.maxPlayers - 2 ? r.tags.queue : 0;", "r.tags.queue && r.tags.queue > 0 ? r.tags.queue : 0;", "any queue keyword believed"],
  // changes.test.mjs
  ["changes", "What's new at start", "row 15 F6", C,
    "  return { releases: [], markSeen: !seen || compareVersions(version, seen) > 0 };",
    "  return { releases: [], markSeen: true };", "an older version lowers the mark"],
  ["changes", "What's new at start", "row 14 H4", C,
    "  if (storedSeen && (!seen || compareVersions(storedSeen, seen) > 0)) seen = storedSeen;\n",
    "", "the stored copy ignored"],
  ["changes", "What's new shows", "D-301", C,
    "export function compareVersions(a: string, b: string): number {", "export function compareVersions(a: string, b: string): number {\n  return a < b ? -1 : a > b ? 1 : 0;", "versions compared as text"],
  ["changes", "What's new shows", "D-301", C,
    "return seen ? toCurrent <= 0 && compareVersions(r.version, seen) > 0 : toCurrent === 0;", "return toCurrent <= 0 && compareVersions(r.version, seen) > 0;", "a file without the mark gets every release"],
  ["changes", "What's new shows", "D-301", C, "  }).slice(0, MAX_SHOWN);", "  });", "no limit of five"],
  ["changes", "What's new shows", "D-301", C, '.split("-")[0]!.split(".")', '.split(".")', "a pre-release suffix read as a newer version"],
  // verdict.test.mjs
  ["verdict", "every rule's reason has its sentence", "D-315", VD,
    '      if (v.reason.includes("entries on a server of")) return "The player list looks fake: it lists more players than the server has slots.";\n', "", "R13 shows the general line"],
  ["verdict", "every rule's reason has its sentence", "D-315", VD,
    '      if (v.reason.includes("under a second beside")) return "The player list looks fake: several players joined within the same second as long-running ones.";\n', "", "R14 shows the general line"],
  ["verdict", "every rule's reason has its sentence", "D-315", VD,
    '      if (v.reason.includes("missing from the list")) return "The player list looks fake: players it lists as older were not there at the previous check.";\n', "", "R11's invariant shows the general line"],
  ["verdict", "every rule's reason has its sentence", "D-268", VD,
    '  v.verdict === "verified" && v.reason.startsWith("INFO reports 0") ? "Empty server" : LABEL[v.verdict];', "  LABEL[v.verdict];", "an empty server headed as a verified count"],
  // pane.test.mjs (row 26, approved)
  ["pane", "the Players line", "row 26 (3)", T,
    "export const playersShown = (r: ServerRow): number => (isUnchecked(r) ? r.players : trustedPlayers(r));",
    "export const playersShown = (r: ServerRow): number => trustedPlayers(r);", "the pane's bold 0 beside the list's claim"],
  ["pane", "the Players line", "row 26 (3)", PN,
    "  if (r.verifiedPlayers == null || playersShown(r) !== r.verifiedPlayers) return null;",
    "  if (r.verifiedPlayers == null) return null;", "'server claims' beside Steam's 0"],
  ["pane", "the other servers at an address", "row 26 (9)", PN,
    " || (self.gamePort > 0 && r.gamePort === self.gamePort)", "", "the pane's own server under its old query port"],
  ["pane", "the other servers at an address", "row 26 (3)", PN,
    "Number(a.untrusted) - Number(b.untrusted) || ", "", "untrusted ones sorted among the trusted"],
  ["pane", "the Connected section counts", "row 26 (2)", PN,
    "  if (secs.filter(zeroLength).length >= 2) secs = secs.filter((s) => !zeroLength(s));\n", "", "R12's zero-length entries counted as sessions"],
  ["pane", "the Connected section counts", "row 26 (2)", PN,
    ' || verdict === "synthetic"', "", "a fake list's sessions shown"],
  ["pane", "each mod once", "row 26 (8)", PN,
    "    if (seen.has(key)) return false;\n", "", "a repeated Workshop id counted twice"],
  ["pane", "each mod once", "row 26 (7)", PN,
    '  if (m.workshopId <= 0) return "server-side";\n', "", "a server-side mod marked missing"],
  ["pane", "each mod once", "row 26 (10)", PN,
    '  return stale.has(m.workshopId) ? "update" : "installed";', '  return "installed";', "no update mark"],
  ["pane", "an unknown server version", "row 26 (6)", T,
    'local != null && version !== "" && version !== local', "local != null && version !== local", "an unknown version warned as different"],
  // shell.test.mjs (row 27)
  ["shell", "a close waits for the page's", "row 27 (11)", CL,
    '      .then(() => invoke("close_ready"))\n', "", "the host is never told the page is done"],
  ["shell", "a flush survives", "row 27 (11)", CL,
    "[...pending].map((f) => Promise.resolve().then(f))", "[...pending].map((f) => f())", "a callback that throws at once rejects the flush"],
  ["shell", "Escape goes to the notices", "row 27 (5)", ES,
    ' || e.defaultPrevented', "", "the notices clear on an Escape a page acted on"],
  ["shell", "Escape goes to the notices", "row 27 (16)", ES,
    " || modalOpen", "", "the notices clear behind a modal"],
  ["shell", "links go through the host", "row 27 (1)", EX,
    '    await invoke("open_link", { url });', '    await invoke("plugin:opener|open_url", { url });', "links opened in-process again"],
  // news.test.mjs, the grid (D-326)
  ["news", "the News grid's rows come out full", "D-326", NG,
    "    if (e < empty) {", "    if (c === most) {", "columns from the width alone: five and three"],
  ["news", "the News grid's rows come out full", "D-326", NG,
    "    return { cols: most, count: Math.min(rows * most, Math.floor(n / most) * most) };", "    return { cols: most, count: CARDS_MAX };", "a part-empty last row in All news"],
  // pages.test.mjs
  ["pages", "favourites are read beside the list", "row 23", S,
    "    this.favouritesLoaded = true;\n",
    "", "the page never learns the favourites were read"],
  ["pages", "favourites are read beside the list", "row 23", S,
    "    } finally {\n      this.listLoaded = true;\n    }",
    "    } finally {\n    }", "the pages wait for the list for ever"],
  ["pages", "favourites that could not be read at start", "row 23", S,
    "    if (!this.favouritesUnread || this.#favRetrying) return;",
    "    return;", "an unread start is never read again"],
  ["pages", "favourites that could not be read at start", "row 23", S,
    "    for (const [id, p] of this.#favPending) {\n      if (p.on) this.favourites.add(id);\n      else this.favourites.delete(id);\n    }\n",
    "", "a read drops a star still in flight"],
  ["pages", "favourite writes go one at a time", "row 23", S,
    "    const done = this.#favWrites.then(write);",
    "    const done = write();", "writes sent at once, in any order"],
  ["pages", "favourite writes go one at a time", "row 23", S,
    "        if (this.#favPending.get(id)?.seq === seq) {\n          if (on) this.favourites.delete(id);",
    "        {\n          if (on) this.favourites.delete(id);", "a failed write undoes a later press"],
  ["pages", "a check's INFO reaches the row", "row 23", S,
    "        if (f.name) next.name = f.name;\n",
    "", "the name stays what the listing wrote"],
  ["pages", "a check's INFO reaches the row", "row 23", S,
    "        if (f.gamePort != null) next.gamePort = f.gamePort;\n",
    "", "the old game port stays"],
  ["pages", "a check's INFO reaches the row", "row 23", S,
    "        if (next.name !== r.name) this.#namesDirty = true;\n",
    "", "a rename keeps its old place in the name sort"],
  ["pages", "a LAN scan is the LAN page's", "row 23", S,
    "        this.refreshListed = 0;\n      }\n      return started;",
    "      }\n      return started;", "the Servers button shows the last refresh's count"],
  ["pages", "a LAN scan is the LAN page's", "row 23", S,
    "      if (this.lanScanning && !dzsa) this.#lanIds.add(r.id);\n",
    "", "a server the scan found through a VPN is on no page"],
  ["pages", "a LAN scan is the LAN page's", "row 23", S,
    "      if (d.source === \"lan\") return;\n",
    "", "a refused scan takes the Servers page's line and indicator"],
  ["pages", "a LAN scan is the LAN page's", "row 23", S,
    "    if (d.source === \"lan\") this.lanScanning = false;\n",
    "", "the scan never ends"],
  ["pages", "Join again takes only a server", "row 23", S,
    "    const r = await this.#probe(h.id, h.gamePort);",
    "    const r = await this.#probe(h.id);", "the entry's game port is not asked for"],
  ["pages", "Join again takes only a server", "row 23", S,
    "    if (typeof r === \"string\" || r.gamePort !== h.gamePort) return",
    "    if (typeof r === \"string\") return", "another server's reply opens its join"],
  ["pages", "a friend's server", "row 23", S,
    "s.queryPort > 0 ? undefined : s.gamePort);",
    "undefined);", "a game address takes whatever answers there"],
  ["pages", "a Direct connect failure stays", "row 23", S,
    "    this.succeeded(\"direct\");\n    if (select) this.selectedId = r.id;",
    "    this.error = null;\n    if (select) this.selectedId = r.id;", "a probe wipes another action's line"],
  ["pages", "Recent shows a new join", "row 23", S,
    "    if (this.historyLoaded) void this.loadHistory();",
    "    void this.loadHistory();", "Recent read before it is opened"],
  ["pages", "Recent shows a new join", "row 23", S,
    "      this.historyLoaded = true;\n",
    "", "a join never reaches an open Recent"],
  ["pages", "the avatar's initial", "row 23", AV,
    "  const first = graphemes ? graphemes.segment(name)[Symbol.iterator]().next().value?.segment : [...name][0];",
    "  const first = name.slice(0, 1);", "half an emoji"],
  ["pages", "a name sort of a short list", "row 23", S,
    "      out.sort((a, b) => dir * (COLLATOR.compare(",
    "      out.sort((a, b) => (COLLATOR.compare(", "descending ignored"],
  ["pages", "the friends count and markers go", "D-222", S,
    "    if (!s?.initialized) {\n      this.#clearFriends();\n      return;\n    }",
    "    if (!s?.initialized) return;", "Steam gone, the count stays (before D-222)"],
  ["pages", "rows at one game address", "D-295 F2", S,
    "    return out.sort((a, b) => Number(a.verdict === \"offline\")",
    "    return out; out.sort((a, b) => Number(a.verdict === \"offline\")", "cache order: an offline id first"],
  ["pages", "the LAN address ranges", "D-256", S,
    "(a === 172 && b >= 16 && b <= 31)",
    "(a === 172 && b >= 16 && b <= 32)", "172.32/16 counted as LAN"],
  ["pages", "a pruned row takes its friend marker", "D-235", S,
    "    for (const id of ids) this.friendsOn.delete(id);\n",
    "", "a pruned server keeps its friend pill"],
  ["pages", "the friends list is kept", "row 23 (1)", S,
    "    this.friendsList = list;\n",
    "", "the page has no list of its own to keep"],
  ["pages", "the friends list is kept", "row 23 (1)", S,
    "        if (!ev.payload.initialized) this.#clearFriends();\n        else if (this.friendsInDayz == null) void this.pollFriends();",
    "        if (this.friendsInDayz == null) void this.pollFriends();", "Steam gone, the list stays until the next poll"],
  ["pages", "the listed row for a friend", "row 23 (5)", S,
    "    const at = this.rowsAtGameAddress(s.ip, s.gamePort);\n    return at.length === 1 && at[0]!.verdict !== \"offline\" ? at[0]! : null;",
    "    return null;", "a game address shows no server"],
  ["pages", "Favourites count before the search", "row 23 (6)", S,
    "    for (const id of this.favourites) if (this.rows.has(id)) n++;",
    "    for (const id of this.favourites) n++;", "a favourite without a row counted"],
  // log.test.mjs
  ["log", "anything thrown becomes a line", "row 25", "src/lib/log.ts",
    "    return JSON.stringify(e) ?? String(e);",
    "    return JSON.stringify(e);", "a rejection without a reason leaves no line"],
  ["log", "Copy's report", "row 25", "src/lib/logreport.ts",
    "  const body = [...lines].sort((a, b) => a.at - b.at)",
    "  const body = [...lines]", "newest first, as the page shows it"],
  ["pages", "a failing friends poll is logged once", "row 25", S,
    "      if (!this.#friendsFailing) logWarn(\"friends\", `poll failed: ${describe(e)}`);",
    "      logWarn(\"friends\", `poll failed: ${describe(e)}`);", "a line every minute while Steam fails"],
  ["pages", "the newest verification of the selected server", "row 26", S,
    "        for (const v of m.data) if (v.id === this.selectedId) this.#lastForSelected = v;\n",
    "", "a check that landed during the pane's own is dropped"],
  ["pages", "onVerified hands each list", "D-297", S,
    "        for (const f of this.#verifiedListeners) f(m.data);\n",
    "", "the pane never hears of later checks"],
  // news.test.mjs
  ["news", "no alert for an update post older than two weeks", "row 24 (1)", N,
    " && n.date >= recent && !known.has(n.gid)",
    " && !known.has(n.gid)", "weeks-old posts toasted"],
  ["news", "no alert for an update post older than two weeks", "row 24 (3)", N,
    "  unread = $derived(this.items.filter((n) => n.official && n.update && n.date > this.seen).length);",
    "  unread = $derived(this.items.filter((n) => n.official && n.date > this.seen).length);", "the badge counts sales and dev blogs"],
  ["news", "a picture that could not be made", "row 24 (2)", N,
    "        .catch(() => this.#thumbFailed.add(key))",
    "        .catch(() => {})", "a failed picture keeps its empty box"],
  ["news", "several new update posts in one fetch", "row 24", N,
    "    const newest = [...fresh].sort((a, b) => b.date - a.date).slice(0, 3);",
    "    const newest = [...fresh].slice(-3);", "the three oldest kept"],
  ["news", "several new update posts in one fetch", "row 24", N,
    "    void notifyIfUnfocused(newest[newest.length - 1]!);",
    "    for (const n of fresh) void notifyIfUnfocused(n);", "a notification per post"],
  ["news", "the stored posts are asked for at once", "row 24", N,
    "    const stored = invoke<NewsCached>(\"news_cached\").catch(() => null);",
    "    const stored = uiPrefs.ready.then(() => invoke<NewsCached>(\"news_cached\")).catch(() => null);", "asked after the settings read"],
  ["news", "the stored posts are asked for at once", "row 24", N,
    "    if (c) this.#setItems(c.items);\n    this.loaded = true;",
    "    if (c) this.#setItems(c.items);", "the page never learns they are in"],
  ["news", "switching News off lets go", "row 24", N,
    "    this.#setItems([]);\n    this.#thumbFailed.clear();\n    this.loaded = false;",
    "    this.#thumbFailed.clear();\n    this.loaded = false;", "the list and its pictures kept"],
  ["news", "a first run counts the newest five", "D-194", N,
    "          if (uiPrefs.readOk) uiPrefs.patch({ newsSeen: mark });",
    "          uiPrefs.patch({ newsSeen: mark });", "an older mark written over a file that was not read"],
  ["news", "switched off during a fetch", "D-185", N,
    "      if (!this.#started) return;\n      this.#setItems(c.items);",
    "      this.#setItems(c.items);", "a toast for a page that is off"],
  ["news", "off and on during the first fetch", "D-240", N,
    "    if (!this.#started || run !== this.#run) return;\n    this.#timer = setInterval(() => void this.refresh(), REFRESH_MS);",
    "    this.#timer = setInterval(() => void this.refresh(), REFRESH_MS);", "two timers"],
  ["news", "the New boundary is frozen", "D-240", N,
    "      this.seen = newest;\n      uiPrefs.patch({ newsSeen: newest });",
    "      this.seen = newest;\n      this.visitSeen = newest;\n      uiPrefs.patch({ newsSeen: newest });", "New pills vanish while looked at"],
  ["news", "a failed news fetch is tried again", "D-302 (11)", N,
    '    window.addEventListener("online", this.#retryIfFailed);\n    window.addEventListener("focus", this.#retryIfFailed);', "", "no retry on reconnect (before row 14)"],
  ["news", "a failed news fetch is tried again", "D-302 (11)", N,
    "    if (!this.error || !this.#started || Date.now() - this.#lastRetry < RETRY_GAP_MS) return;", "    if (!this.error || !this.#started) return;", "retries without the one-minute gap"],
].map(([file, name, fix, src, find, replace, what]) => ({ file, name, fix, src, find, replace, what }));

const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function runTest(m, buildDir) {
  const file = path.join(ROOT, "tests", "front", `${m.file}.test.mjs`);
  return new Promise((resolve) => {
    const p = spawn(process.execPath, ["--test", "--test-force-exit", "--test-timeout=20000", "--test-reporter=spec", `--test-name-pattern=^${esc(m.name)}`, file], {
      env: { ...process.env, FRONT_TEST_BUILD: buildDir, NODE_ENV: "production" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    p.on("close", (code) => resolve({ code, out }));
  });
}

async function check(m) {
  let hits = 0;
  const transform = (rel, code) => {
    if (rel !== m.src) return code;
    const lf = code.replace(/\r\n/g, "\n");
    hits = lf.split(m.find).length - 1;
    return hits === 1 ? lf.replace(m.find, () => m.replace) : lf;
  };
  const out = mkdtempSync(path.join(tmpdir(), "front-mut-"));
  try {
    await build({ out, transform });
    if (hits !== 1) return { result: "stale", detail: `text found ${hits} times in ${m.src}` };
    const r = await runTest(m, out);
    const tests = Number(/tests (\d+)/.exec(r.out)?.[1] ?? 0);
    const failed = Number(/fail (\d+)/.exec(r.out)?.[1] ?? 0);
    const cancelled = Number(/cancelled (\d+)/.exec(r.out)?.[1] ?? 0);
    if (tests !== 1) return { result: "no test", detail: `${tests} tests matched` };
    if (r.code !== 0 && failed === 1) {
      // The first assertion that broke, as the test reported it.
      const why = /(?:AssertionError[^\n]*|\w*Error: [^\n]*)/.exec(r.out)?.[0] ?? "";
      return { result: "caught", detail: why.slice(0, 110) };
    }
    // A test that hangs under the revert fails too, but slowly: worth rewriting.
    if (r.code !== 0 && cancelled === 1) return { result: "caught", detail: "only by timing out" };
    return { result: "MISSED", detail: "" };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

async function main(filters) {
  const todo = MUTATIONS.filter((m) => filters.length === 0 || filters.some((f) => `${m.file} ${m.name} ${m.fix}`.includes(f)));
  const results = new Array(todo.length);
  const started = performance.now();
  // Builds run one at a time (the compiler is synchronous); the test runs overlap.
  const running = new Set();
  for (let i = 0; i < todo.length; i++) {
    const p = check(todo[i]).then((r) => {
      results[i] = r;
      running.delete(p);
    });
    running.add(p);
    if (running.size >= 4) await Promise.race(running);
  }
  await Promise.all(running);
  const verbose = process.env.MUTATIONS_VERBOSE === "1";
  todo.forEach((m, i) => {
    const r = results[i];
    console.log(`${r.result.padEnd(7)} ${m.file.padEnd(8)} ${m.fix.padEnd(14)} ${m.what}${r.result !== "caught" || verbose ? `  [${r.detail}]` : ""}`);
  });
  const bad = results.filter((r) => r.result !== "caught").length;
  console.log(`\n${todo.length - bad}/${todo.length} reverted fixes caught, in ${Math.round((performance.now() - started) / 1000)} s`);
  return bad === 0 ? 0 : 1;
}

if (import.meta.main ?? path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  process.exitCode = await main(process.argv.slice(2));
}

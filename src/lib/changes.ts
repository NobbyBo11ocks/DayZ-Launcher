// What changed in each release, written the way DayZ players read patch notes: a few
// short lines under Added, Fixed, Changed or Removed, in plain words (user request,
// D-301). Newest first. The window after an update (WhatsNew.svelte) shows the releases
// the player has not seen yet. `tools/check_changes.js` fails CI when the version in
// package.json has no entry here, or a line is long or uses the launcher's own jargon.

export type NoteKind = "added" | "fixed" | "changed" | "removed";
export type Note = { kind: NoteKind; text: string };
export type Release = { version: string; notes: Note[] };

export const CHANGES: readonly Release[] = [
  {
    version: "0.1.100",
    notes: [
      { kind: "fixed", text: "Links no longer start your browser as administrator when the launcher is one." },
      { kind: "fixed", text: "Starting the launcher while it is open no longer asks for administrator rights." },
      { kind: "fixed", text: "The window comes back on screen after a monitor or scaling change." },
      { kind: "fixed", text: "Closing with Alt+F4 or from the taskbar keeps the changes you just made." },
      { kind: "fixed", text: "Escape closes the server details without clearing your notices too." },
    ],
  },
  {
    version: "0.1.99",
    notes: [
      { kind: "changed", text: "Server details show the last known mods when a server does not send its list." },
      { kind: "changed", text: "Server details show the same player count as the server list." },
      { kind: "added", text: "Mods with an update waiting are marked in server details." },
      { kind: "fixed", text: "A server whose version is unknown is no longer marked as a different version." },
      { kind: "changed", text: "Connected players in server details stay up to date while you keep them open." },
    ],
  },
  {
    version: "0.1.98",
    notes: [
      { kind: "fixed", text: "Joining a modded server no longer starts DayZ without its mods." },
      { kind: "fixed", text: "A server's details no longer call it unverified after one lost reply." },
      { kind: "fixed", text: "A server's details stay up to date while you keep them open." },
      { kind: "fixed", text: "A server opened in its details gets its population chart at once." },
    ],
  },
  {
    version: "0.1.97",
    notes: [
      { kind: "changed", text: "Muting an area in Logs still keeps its warnings and errors." },
      { kind: "changed", text: "The Logs page says when its list is paused and when the file cannot be saved." },
      { kind: "changed", text: "Error messages say how to keep the details when recording is off." },
    ],
  },
  {
    version: "0.1.96",
    notes: [
      { kind: "changed", text: "Copying the log adds your launcher and Windows versions, oldest line first." },
      { kind: "fixed", text: "The log no longer shows any part of your Windows account name." },
      { kind: "fixed", text: "The log keeps every line when several parts of the launcher write at once." },
      { kind: "fixed", text: "Failures are logged under their own area, like Joining or Mods." },
    ],
  },
  {
    version: "0.1.95",
    notes: [
      { kind: "changed", text: "The News badge counts new game updates only, not sales or dev blogs." },
      { kind: "changed", text: "Only real game updates are marked Update and get a notification." },
      { kind: "changed", text: "News posts no longer jump when their pictures arrive." },
      { kind: "fixed", text: "No update notification for posts more than two weeks old." },
      { kind: "fixed", text: "The greeting no longer shows your Steam name while Steam is closed." },
    ],
  },
  {
    version: "0.1.94",
    notes: [
      { kind: "fixed", text: "Several new DayZ updates at once now show the newest, with one notification." },
      { kind: "fixed", text: "The News page shows its posts sooner when the launcher starts." },
      { kind: "fixed", text: "News summaries no longer show stray spaces, links or odd codes." },
      { kind: "changed", text: "News pictures download once, and only as you scroll to them." },
    ],
  },
  {
    version: "0.1.93",
    notes: [
      { kind: "changed", text: "Friends keeps your last list, with its time, while disconnected from Steam." },
      { kind: "changed", text: "A friend's server shows its name even when Steam gives only its address." },
      { kind: "changed", text: "Favourites shows how many match your search out of all of them." },
      { kind: "changed", text: "Recent says when it lists only your last 100 joins." },
    ],
  },
  {
    version: "0.1.92",
    notes: [
      { kind: "fixed", text: "Favourites and Recent show a server's new name, map and version." },
      { kind: "fixed", text: "Join again and joining a friend no longer open a different server." },
      { kind: "fixed", text: "Favourites, Recent and Friends no longer say \"none\" while still loading." },
      { kind: "fixed", text: "Starring and unstarring quickly keeps the favourite you picked last." },
      { kind: "fixed", text: "The LAN page shows only its own scan, and every server it finds." },
    ],
  },
  {
    version: "0.1.91",
    notes: [{ kind: "changed", text: "A server's details now say what gave its fake player list away." }],
  },
  {
    version: "0.1.90",
    notes: [{ kind: "added", text: "Better at spotting servers that pad their player list with fake players." }],
  },
  {
    version: "0.1.89",
    notes: [{ kind: "fixed", text: "Servers that just restarted are no longer hidden as fakes." }],
  },
  {
    version: "0.1.88",
    notes: [
      { kind: "fixed", text: "Start Steam is safer when the launcher runs as administrator." },
      { kind: "changed", text: "Logs no longer show your Windows account name." },
    ],
  },
  {
    version: "0.1.87",
    notes: [{ kind: "fixed", text: "The server list stays smoother during a Refresh with a mod filter picked." }],
  },
  {
    version: "0.1.86",
    notes: [
      { kind: "fixed", text: "Fake servers are cleared out after a full Refresh instead of piling up." },
      { kind: "changed", text: "The Logs page shows more about each full Refresh and what it removed." },
    ],
  },
  {
    version: "0.1.85",
    notes: [
      { kind: "fixed", text: "Servers no longer drop out of the list right after a full Refresh." },
      { kind: "fixed", text: "Far-away servers' player counts and mod lists are read more reliably." },
      { kind: "fixed", text: "A slow server's ping no longer shows as —." },
      { kind: "fixed", text: "A server you open while it restarts comes back without a Refresh." },
      { kind: "fixed", text: "A port taken over by another game no longer shows that game's numbers." },
    ],
  },
  {
    version: "0.1.84",
    notes: [
      { kind: "fixed", text: "Join works again once Steam is back after a join failed without it." },
      { kind: "fixed", text: "A refresh Steam did not answer is tried again after a pause." },
      { kind: "fixed", text: "The connection notice stays until your connection is really back." },
      { kind: "fixed", text: "The Steam sign-in message no longer stays on screen after Steam closes." },
      { kind: "fixed", text: "The first News highlights are kept for you after the welcome." },
    ],
  },
  {
    version: "0.1.83",
    notes: [
      { kind: "added", text: "Pick the page the launcher opens on: News, Servers or Favourites." },
      { kind: "added", text: "Start Steam from the launcher when Steam is closed." },
      { kind: "changed", text: "Direct connect has its own button and opens the join window." },
      { kind: "changed", text: "Clearer words everywhere, like Hide untrusted and In-game name." },
      { kind: "fixed", text: "The first window fits laptop screens, and missing spaces are back." },
    ],
  },
  {
    version: "0.1.82",
    notes: [
      { kind: "added", text: "The join window says when your name has letters DayZ cannot show." },
      { kind: "changed", text: "Saving a profile over one with the same name now says Replace." },
      { kind: "changed", text: "Deleting a launch profile asks first." },
      { kind: "changed", text: "Settings says clearly when your changes could not be saved." },
      { kind: "fixed", text: "Settings no longer saves defaults over a file it could not read." },
    ],
  },
  {
    version: "0.1.81",
    notes: [
      { kind: "fixed", text: "Joining after you switch Steam accounts uses the right name." },
      { kind: "fixed", text: "The window keeps its size and place after an update." },
      { kind: "fixed", text: "One bad value in your settings file no longer resets all of it." },
      { kind: "fixed", text: "Settings changes are kept when you close the launcher right away." },
      { kind: "fixed", text: "Two launch profiles with the same name no longer break Settings." },
    ],
  },
  {
    version: "0.1.80",
    notes: [
      { kind: "added", text: "The Servers page tells you when your connection seems to be down." },
      { kind: "added", text: "The join window tells you when Steam stops downloading a mod." },
      { kind: "changed", text: "Clearer messages when Steam is signed out or the internet is down." },
      { kind: "fixed", text: "Importing favourites with nothing to import no longer shows an error." },
      { kind: "added", text: "You are told if your settings or saved favourites could not be read." },
    ],
  },
  {
    version: "0.1.79",
    notes: [
      { kind: "fixed", text: "Servers no longer vanish from the list when your internet drops." },
      { kind: "fixed", text: "Favourites and recent joins stay safe when antivirus locks a file." },
      { kind: "fixed", text: "After Steam restarts, the server list refreshes and Join works again." },
      { kind: "fixed", text: "Stalled mod downloads no longer show a frozen speed." },
      { kind: "changed", text: "F5 no longer reloads the launcher and closes the join window." },
    ],
  },
  {
    version: "0.1.78",
    notes: [{ kind: "added", text: "After every update, this window shows what changed." }],
  },
];

/**
 * What the What's new window does at start (D-301): the releases to show, or whether to
 * record this version as seen without showing any. The mark is the newer of the settings
 * file's and the copy in storage, as for the update check (D-281): a file that came back
 * as the defaults must not show the same notes again (row 14, H4). A first run (not yet
 * onboarded) only records the version, the welcome shows then; and the mark is never
 * lowered, or an older version started after a newer one showed the same notes again
 * at the next update (row 15, F6). Lived inside App.svelte, untested (row 24).
 */
export function whatsNewPlan(fileSeen: string, storedSeen: string, version: string, onboarded: boolean): { releases: Release[]; markSeen: boolean } {
  let seen = fileSeen;
  if (storedSeen && (!seen || compareVersions(storedSeen, seen) > 0)) seen = storedSeen;
  if (seen === version) return { releases: [], markSeen: false };
  const releases = onboarded ? unseenChanges(seen, version) : [];
  if (releases.length > 0) return { releases, markSeen: false };
  return { releases: [], markSeen: !seen || compareVersions(version, seen) > 0 };
}

/** At most this many releases in the window; a player several updates behind gets the newest. */
const MAX_SHOWN = 5;

/** Orders "0.1.9" before "0.1.10"; anything after a dash is ignored. */
export function compareVersions(a: string, b: string): number {
  const parts = (v: string) => v.split("-")[0]!.split(".").map((n) => Number.parseInt(n, 10) || 0);
  const pa = parts(a);
  const pb = parts(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

/**
 * The releases a player moving from `seen` to `current` has not been shown, newest
 * first. An empty `seen` is a settings file from before these notes existed, which
 * gets the current release's notes only; a `seen` newer than `current` (going back to
 * an older version) gets none.
 */
export function unseenChanges(seen: string, current: string): Release[] {
  return CHANGES.filter((r) => {
    const toCurrent = compareVersions(r.version, current);
    return seen ? toCurrent <= 0 && compareVersions(r.version, seen) > 0 : toCurrent === 0;
  }).slice(0, MAX_SHOWN);
}

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

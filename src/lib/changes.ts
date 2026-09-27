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

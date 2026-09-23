// Map names as players say them (D-195).
//
// A2S reports the folder name: Livonia is `enoch`, Chernarus is `chernarusplus`, and
// the Frostline map is `sakhal`. Typing "Livonia" into the search box therefore found
// nothing at all — 2 940 servers on this machine's cache were invisible to their own
// name — and the map dropdown read like a directory listing. It is the same confusion
// players hit with DZSA, where the answer on the Steam forums is the single word
// "Enoch" (S-88).
//
// Only maps whose id genuinely differs from the name in use are listed. Anything else
// is shown exactly as the server reports it: inventing a name for a community map
// would be worse than a plain id.

/** Lower-cased map id → what to put on screen. */
const LABELS = new Map<string, string>([
  ["chernarusplus", "Chernarus"],
  ["chernarus", "Chernarus"],
  ["chernarus2035", "Chernarus 2035"],
  ["enoch", "Livonia"],
  ["sakhal", "Sakhal"],
  ["deerisle", "Deer Isle"],
  ["takistanplus", "Takistan"],
  ["exclusionzone", "Exclusion Zone"],
  ["exclusionzoneplus", "Exclusion Zone+"],
  ["greencounty", "Green County"],
  ["novikostok", "Novi Kostok"],
  ["stuartisland", "Stuart Island"],
  ["nhchernobyl", "Chernobyl"],
  ["thezone", "The Zone"],
  ["pnw", "PNW"],
  ["banovfrost", "Banov Frost"],
]);

/** Extra words the search box should match, beyond the id and the label. */
const ALIASES = new Map<string, string>([
  // The DLC is sold as Frostline; the map inside it is Sakhal.
  ["sakhal", "frostline"],
  // Both halves of the pair people actually type.
  ["enoch", "livonia"],
  ["chernarusplus", "chernarus"],
]);

/** Title-cased for the few single-word ids that are already the name. */
function pretty(id: string): string {
  return id.length > 1 && id === id.toLowerCase() && /^[a-z]+$/.test(id)
    ? id[0]!.toUpperCase() + id.slice(1)
    : id;
}

/** What the map is called on screen. Unknown ids are returned unchanged. */
export function mapLabel(id: string): string {
  return LABELS.get(id.toLowerCase()) ?? pretty(id);
}

/**
 * Everything the search box should match for a row's map, lower-cased and already
 * joined: the raw id, the display name, and any alias. Built per row inside the filter
 * loop, so it stays a single `includes` (D-188's rule: no per-row allocation the
 * filter can avoid — this one is a cached string).
 */
const HAYSTACK = new Map<string, string>();
export function mapHaystack(id: string): string {
  const key = id.toLowerCase();
  let s = HAYSTACK.get(key);
  if (s === undefined) {
    const parts = [key, mapLabel(id).toLowerCase()];
    const alias = ALIASES.get(key);
    if (alias) parts.push(alias);
    s = [...new Set(parts)].join(" ");
    HAYSTACK.set(key, s);
  }
  return s;
}

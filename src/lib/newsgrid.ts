// The News grid's shape (D-326). Columns filled in from the width alone (`auto-fill`) left
// the last row part-empty: eight update cards on a maximised window were five and three,
// two gaps at the end. Here the column count also looks at how many cards there are.

/** Card widths the grid keeps to, and the gap between cards, in CSS pixels. */
const MIN_CARD = 280;
const MAX_CARD = 440;
const GAP = 12;
/** Cards below the featured post, at most. */
export const CARDS_MAX = 24;

/**
 * The grid's columns and how many cards it shows, so rows come out full. More posts than
 * the grid holds: the most columns that fit, and a whole number of rows of them. Fewer:
 * the column count, within the card widths above, that leaves the fewest empty cells, the
 * most columns among equals. `cols` is 0 while the width is not known yet; the style
 * sheet's own fill stands in.
 */
export function newsGrid(width: number, available: number): { cols: number; count: number } {
  const n = Math.max(0, Math.floor(available));
  if (!(width > 0)) return { cols: 0, count: Math.min(n, CARDS_MAX) };
  const most = Math.max(1, Math.floor((width + GAP) / (MIN_CARD + GAP)));
  const fewest = Math.min(most, Math.max(1, Math.ceil((width + GAP) / (MAX_CARD + GAP))));
  if (n > CARDS_MAX) {
    const rows = Math.max(1, Math.round(CARDS_MAX / most));
    return { cols: most, count: Math.min(rows * most, Math.floor(n / most) * most) };
  }
  let cols = most;
  let empty = Infinity;
  for (let c = most; c >= fewest; c--) {
    const e = (c - (n % c)) % c;
    if (e < empty) {
      cols = c;
      empty = e;
    }
  }
  return { cols, count: n };
}

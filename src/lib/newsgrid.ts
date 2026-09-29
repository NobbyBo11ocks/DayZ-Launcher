// The News grid's shape (D-326). Columns filled in from the width alone (`auto-fill`) left
// the last row part-empty: eight update cards on a maximised window were five and three,
// two gaps at the end. Here the column count also looks at how many cards there are, and
// a last row that still comes out short is widened to the grid's edge.

/** Card widths the grid keeps to, and the gap between cards, in CSS pixels. */
const MIN_CARD = 280;
const MAX_CARD = 440;
const GAP = 12;
/** Cards below the featured post, about: rounded to whole rows, so 25 at five columns. */
export const CARDS_MAX = 24;
/** How much wider than the others a short last row's cards may grow to reach the edge.
 *  Four cards in a row of five grow by a quarter; one card alone in a row of three would
 *  be three times as wide, with a picture to match, and the gaps stay instead. */
const STRETCH_MAX = 1.7;

export type NewsGrid = {
  /** Columns; 0 while the width is not known yet (the style sheet's own fill stands in). */
  cols: number;
  /** Cards shown. */
  count: number;
  /** Grid tracks: `cols`, or more when a short last row is widened (a multiple of both). */
  tracks: number;
  /** Tracks a card spans, and a card of the widened last row. */
  span: number;
  lastSpan: number;
  /** The index of the first card of a widened last row (`count` when none is). */
  lastFrom: number;
};

const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));

/**
 * The grid's columns and cards, so rows come out full. More posts than the grid holds: the
 * most columns that fit, and a whole number of rows of them. Fewer: the column count, within
 * the card widths above, that leaves the fewest empty cells, the most columns among equals;
 * and a last row still short by little is widened to the edge.
 */
export function newsGrid(width: number, available: number): NewsGrid {
  const n = Math.max(0, Math.floor(available));
  if (!(width > 0)) return { cols: 0, count: Math.min(n, CARDS_MAX), tracks: 0, span: 1, lastSpan: 1, lastFrom: Math.min(n, CARDS_MAX) };
  const most = Math.max(1, Math.floor((width + GAP) / (MIN_CARD + GAP)));
  const fewest = Math.min(most, Math.max(1, Math.ceil((width + GAP) / (MAX_CARD + GAP))));
  let cols = most;
  let count = n;
  if (n > CARDS_MAX) {
    const rows = Math.max(1, Math.round(CARDS_MAX / most));
    count = Math.min(rows * most, Math.floor(n / most) * most);
  } else {
    let empty = Infinity;
    for (let c = most; c >= fewest; c--) {
      const e = (c - (n % c)) % c;
      if (e < empty) {
        cols = c;
        empty = e;
      }
    }
  }
  const short = count % cols;
  if (short === 0 || count < cols || cols / short > STRETCH_MAX) {
    return { cols, count, tracks: cols, span: 1, lastSpan: 1, lastFrom: count };
  }
  const tracks = (cols * short) / gcd(cols, short);
  return { cols, count, tracks, span: tracks / cols, lastSpan: tracks / short, lastFrom: count - short };
}

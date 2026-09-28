// Raw RGBA from Steam (D-100, D-115) to a PNG data URL through a canvas: no image
// codec on either side, and the CSP already allows `data:` images.
type Rgba = { width: number; height: number; rgba: Uint8ClampedArray<ArrayBuffer> };

let graphemes: Intl.Segmenter | null | undefined;

/** A name's first character for the placeholder avatar, whole. `slice(0, 1)` took half
 *  of a surrogate pair, and a name starting with an emoji drew a broken glyph (row 23). */
export function initialOf(name: string): string {
  if (graphemes === undefined) graphemes = typeof Intl.Segmenter === "function" ? new Intl.Segmenter(undefined, { granularity: "grapheme" }) : null;
  const first = graphemes ? graphemes.segment(name)[Symbol.iterator]().next().value?.segment : [...name][0];
  return (first ?? "").toUpperCase();
}

export function avatarDataUrl(a: Rgba): string | null {
  const canvas = document.createElement("canvas");
  canvas.width = a.width;
  canvas.height = a.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.putImageData(new ImageData(a.rgba, a.width, a.height), 0, 0);
  return canvas.toDataURL("image/png");
}

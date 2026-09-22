// Raw RGBA from Steam (D-100, D-115) to a PNG data URL through a canvas: no image
// codec on either side, and the CSP already allows `data:` images.
type Rgba = { width: number; height: number; rgba: Uint8ClampedArray<ArrayBuffer> };

export function avatarDataUrl(a: Rgba): string | null {
  const canvas = document.createElement("canvas");
  canvas.width = a.width;
  canvas.height = a.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.putImageData(new ImageData(a.rgba, a.width, a.height), 0, 0);
  return canvas.toDataURL("image/png");
}

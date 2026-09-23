// Installer artwork (D-201): the NSIS header strip and the welcome sidebar, drawn from
// the same mark as the app icon so the two can never drift apart again. They had:
// `make_icon.js` redrew the icon on 2026-09-22 at 12:26 and these two bitmaps were made
// seven minutes earlier, so the installer greeted people with an amber square-cut "Z"
// while the app itself had a lime rounded one.
//
// NSIS wants BMP, which sharp does not write, so the raw pixels are wrapped in a 24-bit
// BMP header here — thirty lines and no extra dependency beyond the one make_icon.js
// already asks for.
//
// Usage: npm i --no-save sharp && node tools/nsis_art.js
import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let sharp;
try {
  sharp = require("sharp");
} catch {
  console.error("nsis_art: needs sharp. Run:\n  npm i --no-save sharp && node tools/nsis_art.js");
  process.exit(2);
}

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "src-tauri", "nsis");

// Same palette as tools/make_icon.js and src/app.css.
const BG_TOP = "#1d2530";
const BG_BOTTOM = "#0f1216";
const EDGE = "#2f3947";
const ACCENT = "#a3e635";
const ACCENT_DIM = "#6f9e1f";
const FG = "#e6e9ee";
const FG_MUTED = "#8b949e";

/** The icon's mark, as a standalone group scaled to `size` at (x, y). */
function mark(x, y, size) {
  const s = size / 256;
  return `
  <g transform="translate(${x} ${y}) scale(${s})">
    <rect x="8" y="8" width="240" height="240" rx="54" fill="url(#tile)" stroke="${EDGE}" stroke-width="4" />
    <circle cx="128" cy="128" r="84" fill="none" stroke="${ACCENT_DIM}" stroke-width="5" opacity="0.32" />
    <path d="M74 74 H182 L74 182 H182" fill="none" stroke="url(#stroke)" stroke-width="30"
          stroke-linecap="round" stroke-linejoin="round" />
  </g>`;
}

const defs = `
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="${BG_TOP}" />
      <stop offset="1" stop-color="${BG_BOTTOM}" />
    </linearGradient>
    <linearGradient id="stroke" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="${ACCENT}" />
      <stop offset="1" stop-color="${ACCENT_DIM}" />
    </linearGradient>
  </defs>`;

// 150x57: shown top-right on every page after the welcome one. Small, so the mark
// carries it and the name sits beside it.
const header = `<svg xmlns="http://www.w3.org/2000/svg" width="150" height="57" viewBox="0 0 150 57">
  ${defs}
  <rect width="150" height="57" fill="${BG_BOTTOM}" />
  ${mark(6, 5, 47)}
  <text x="60" y="27" font-family="Segoe UI, sans-serif" font-size="13" font-weight="600" fill="${FG}">DZSA CrayZ</text>
  <text x="60" y="42" font-family="Segoe UI, sans-serif" font-size="12" font-weight="500" fill="${ACCENT}">Launcher</text>
</svg>`;

// 164x314: the welcome and finish pages. Room for the mark, the name and one line
// saying what it is.
const sidebar = `<svg xmlns="http://www.w3.org/2000/svg" width="164" height="314" viewBox="0 0 164 314">
  ${defs}
  <rect width="164" height="314" fill="${BG_BOTTOM}" />
  <rect x="0" y="0" width="4" height="314" fill="url(#stroke)" />
  ${mark(32, 52, 100)}
  <text x="82" y="196" text-anchor="middle" font-family="Segoe UI, sans-serif" font-size="15" font-weight="650" fill="${FG}">DZSA CrayZ</text>
  <text x="82" y="216" text-anchor="middle" font-family="Segoe UI, sans-serif" font-size="15" font-weight="650" fill="${ACCENT}">Launcher</text>
  <text x="82" y="248" text-anchor="middle" font-family="Segoe UI, sans-serif" font-size="11" fill="${FG_MUTED}">Server browser,</text>
  <text x="82" y="263" text-anchor="middle" font-family="Segoe UI, sans-serif" font-size="11" fill="${FG_MUTED}">mod sync and launch</text>
  <rect x="52" y="286" width="60" height="3" rx="1.5" fill="url(#stroke)" />
</svg>`;

/** 24-bit BMP, bottom-up, 4-byte aligned rows — what NSIS reads. */
function toBmp(rgb, width, height) {
  const rowBytes = width * 3;
  const pad = (4 - (rowBytes % 4)) % 4;
  const size = 54 + (rowBytes + pad) * height;
  const buf = Buffer.alloc(size);
  buf.write("BM", 0);
  buf.writeUInt32LE(size, 2);
  buf.writeUInt32LE(54, 10); // pixel offset
  buf.writeUInt32LE(40, 14); // DIB header size
  buf.writeInt32LE(width, 18);
  buf.writeInt32LE(height, 22); // positive: rows are bottom-up
  buf.writeUInt16LE(1, 26); // planes
  buf.writeUInt16LE(24, 28); // bits per pixel
  buf.writeUInt32LE((rowBytes + pad) * height, 34);
  let o = 54;
  for (let y = height - 1; y >= 0; y--) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 3;
      buf[o++] = rgb[i + 2]; // BMP is BGR
      buf[o++] = rgb[i + 1];
      buf[o++] = rgb[i];
    }
    o += pad;
  }
  return buf;
}

for (const [name, svg, w, h] of [
  ["header", header, 150, 57],
  ["sidebar", sidebar, 164, 314],
]) {
  const { data } = await sharp(Buffer.from(svg))
    .flatten({ background: BG_BOTTOM })
    .raw()
    .toBuffer({ resolveWithObject: true });
  const file = path.join(OUT, `${name}.bmp`);
  writeFileSync(file, toBmp(data, w, h));
  console.log(`nsis_art: wrote ${file} (${w}x${h})`);
}

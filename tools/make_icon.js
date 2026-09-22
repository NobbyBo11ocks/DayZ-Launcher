// Generates the app icon (D-156): a dark tile with a lime chevron mark that reads as a
// "Z" at 256 px and as a forward arrow at 16 px, over a faint crosshair ring for the
// server-browser idea. Drawn as SVG and rasterised with sharp, then written as a
// multi-size .ico plus the PNG sizes Tauri bundles.
//
// Usage: node tools/make_icon.js            (writes src-tauri/icons/*)
//        node tools/make_icon.js --preview  (also writes icon-preview.png at 256)
import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let sharp;
try {
  sharp = require("sharp");
} catch {
  console.error("make_icon: needs sharp. Run it from a directory where `npm i sharp` has been done, e.g.\n  npm i --no-save sharp && node tools/make_icon.js");
  process.exit(2);
}

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const ICONS = path.join(ROOT, "src-tauri", "icons");

const BG_TOP = "#1d2530";
const BG_BOTTOM = "#0f1216";
const EDGE = "#2f3947";
const ACCENT = "#a3e635";
const ACCENT_DIM = "#6f9e1f";

/** One square mark at any size; `r` is the corner radius as a fraction. */
const svg = (size) => {
  const s = size;
  const u = (n) => (n * s) / 1024; // authored at 1024
  return `<svg width="${s}" height="${s}" viewBox="0 0 ${s} ${s}" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0.3" y2="1">
      <stop offset="0%" stop-color="${BG_TOP}"/>
      <stop offset="100%" stop-color="${BG_BOTTOM}"/>
    </linearGradient>
    <linearGradient id="mark" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="${ACCENT}"/>
      <stop offset="100%" stop-color="${ACCENT_DIM}"/>
    </linearGradient>
  </defs>
  <rect x="${u(8)}" y="${u(8)}" width="${u(1008)}" height="${u(1008)}" rx="${u(208)}" fill="url(#tile)" stroke="${EDGE}" stroke-width="${u(16)}"/>
  <circle cx="${u(512)}" cy="${u(512)}" r="${u(330)}" fill="none" stroke="${ACCENT}" stroke-width="${u(14)}" opacity="0.16"/>
  <path d="M ${u(300)} ${u(300)} H ${u(724)} L ${u(300)} ${u(724)} H ${u(724)}"
        fill="none" stroke="url(#mark)" stroke-width="${u(118)}"
        stroke-linecap="round" stroke-linejoin="round"/>
  <rect x="${u(300)}" y="${u(790)}" width="${u(424)}" height="${u(26)}" rx="${u(13)}" fill="${ACCENT}" opacity="0.5"/>
</svg>`;
};

const png = (size) => sharp(Buffer.from(svg(size))).png({ compressionLevel: 9 }).toBuffer();

/** Minimal ICO container: header, one directory entry per image, PNG payloads. */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  const dir = Buffer.alloc(16 * images.length);
  let offset = header.length + dir.length;
  images.forEach((img, i) => {
    const o = i * 16;
    dir[o] = img.size >= 256 ? 0 : img.size;
    dir[o + 1] = img.size >= 256 ? 0 : img.size;
    dir[o + 2] = 0; // palette
    dir[o + 3] = 0;
    dir.writeUInt16LE(1, o + 4); // colour planes
    dir.writeUInt16LE(32, o + 6); // bits per pixel
    dir.writeUInt32LE(img.data.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += img.data.length;
  });
  return Buffer.concat([header, dir, ...images.map((i) => i.data)]);
}

(async () => {
  const icoSizes = [16, 24, 32, 48, 64, 128, 256];
  const images = [];
  for (const size of icoSizes) images.push({ size, data: await png(size) });
  writeFileSync(path.join(ICONS, "icon.ico"), ico(images));

  // The PNGs the Tauri bundle and the Windows Store assets refer to.
  const pngTargets = {
    "32x32.png": 32,
    "64x64.png": 64,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    "source.png": 1024,
    "StoreLogo.png": 50,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
  };
  for (const [name, size] of Object.entries(pngTargets)) {
    writeFileSync(path.join(ICONS, name), await png(size));
  }

  if (process.argv.includes("--preview")) {
    writeFileSync(path.join(ROOT, "icon-preview.png"), await png(256));
    console.log("wrote icon-preview.png");
  }
  console.log(`wrote icon.ico (${icoSizes.join(", ")}) and ${Object.keys(pngTargets).length} PNG sizes to src-tauri/icons`);
})();

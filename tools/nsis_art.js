// Installer artwork (D-201, D-258): the welcome/finish sidebar, the header strip and the
// splash, made from the launcher's logo (docs/art/logo.png) and the icon's gas mask
// (docs/art/emblem.png) on the launcher's own dark surfaces and lime accent.
//
// Each page picture comes in seven sizes, one per common display scale (100 % to
// 300 %). MUI hands Windows a single bitmap to stretch to the control, and Windows
// stretches it by dropping or doubling pixels, so the installer picks the size that
// matches at run time instead (installer.nsi, DzlPickArt).
//
// NSIS wants BMP, which sharp does not write, so the raw pixels are wrapped in a 24-bit
// BMP header here. Text uses Bebas Neue (tools/fonts, SIL OFL), vendored so the art
// comes out the same on any machine.
//
// Usage: npm i --no-save sharp && node tools/nsis_art.js   (after tools/make_icon.js)
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
const LOGO = path.join(ROOT, "docs", "art", "logo.png");
const EMBLEM = path.join(ROOT, "docs", "art", "emblem.png");
const FONT = path.join(ROOT, "tools", "fonts", "BebasNeue-Regular.ttf");

// The launcher's dark theme (src/app.css) and the installer's colours (installer.nsi):
// HEAD is the raised surface the header band is painted with, so the strip has no edge.
const BG = "#0f1216";
const HEAD = "#161b22";
const TOP = "#1a212b";
const FG = "#e6e9ee";
const MUTED = "#8b949e";
const ACCENT = "#a3e635";
const GLOW = "#7dff2a";
const EMBER = "#ff7a1a";

export const SCALES = [100, 125, 150, 175, 200, 250, 300];

/** A line of Bebas Neue as an RGBA layer, `px` tall at 72 dpi. */
async function text(str, px, colour, spacing = 0) {
  const esc = str.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const markup = `<span foreground="${colour}" letter_spacing="${Math.round(spacing * 1024)}">${esc}</span>`;
  const buf = await sharp({ text: { text: markup, font: `Bebas Neue ${px}`, fontfile: FONT, rgba: true, dpi: 72 } })
    .png()
    .toBuffer();
  const { width, height } = await sharp(buf).metadata();
  return { buf, width, height };
}

/** A picture fitted into a box, as a layer. */
async function fitted(file, w, h) {
  const buf = await sharp(file).resize(Math.round(w), Math.round(h), { fit: "inside", kernel: "lanczos3" }).png().toBuffer();
  const meta = await sharp(buf).metadata();
  return { buf, width: meta.width, height: meta.height };
}

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

async function writeBmp(name, png, w, h, bg = BG) {
  const { data } = await sharp(png).flatten({ background: bg }).removeAlpha().raw().toBuffer({ resolveWithObject: true });
  writeFileSync(path.join(OUT, name), toBmp(data, w, h));
  return name;
}

/** Film grain over the whole picture, so the flat dark reads as a surface. */
const grain = (w, h, s) => `
  <filter id="grain" x="0" y="0" width="100%" height="100%">
    <feTurbulence type="fractalNoise" baseFrequency="${(0.9 / s).toFixed(3)}" numOctaves="2" seed="7" result="n"/>
    <feColorMatrix type="saturate" values="0"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.07"/></feComponentTransfer>
  </filter>
  <rect width="${w}" height="${h}" filter="url(#grain)"/>`;

// 164x314 at 100 %: the welcome and finish pages. The logo, a glow behind it in the
// logo's own green and orange, three lines on what the launcher does, and a lime rule
// down the edge the page text sits against.
async function sidebar(scale) {
  const s = scale / 100;
  const W = Math.round(164 * s);
  const H = Math.round(314 * s);
  const u = (n) => n * s;
  const bgSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${TOP}"/><stop offset="1" stop-color="#0a0c0f"/></linearGradient>
    <radialGradient id="g1" cx="0.5" cy="0.32" r="0.55"><stop offset="0" stop-color="${GLOW}" stop-opacity="0.26"/><stop offset="1" stop-color="${GLOW}" stop-opacity="0"/></radialGradient>
    <radialGradient id="g2" cx="0.5" cy="0.26" r="0.38"><stop offset="0" stop-color="${EMBER}" stop-opacity="0.16"/><stop offset="1" stop-color="${EMBER}" stop-opacity="0"/></radialGradient>
    <linearGradient id="rule" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${ACCENT}" stop-opacity="0"/><stop offset="0.5" stop-color="${ACCENT}"/><stop offset="1" stop-color="${ACCENT}" stop-opacity="0"/></linearGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${ACCENT}" stop-opacity="0"/><stop offset="0.3" stop-color="${ACCENT}" stop-opacity="0.75"/><stop offset="0.7" stop-color="${ACCENT}" stop-opacity="0.75"/><stop offset="1" stop-color="${ACCENT}" stop-opacity="0"/></linearGradient>
  </defs>
  <rect width="${W}" height="${H}" fill="url(#bg)"/>
  <rect width="${W}" height="${H}" fill="url(#g1)"/>
  <rect width="${W}" height="${H}" fill="url(#g2)"/>
  ${grain(W, H, s)}
  <rect x="${u(30)}" y="${u(190)}" width="${u(104)}" height="${Math.max(1, u(1.5))}" fill="url(#rule)"/>
  <rect x="${W - Math.max(1, Math.round(u(2)))}" y="0" width="${Math.max(1, Math.round(u(2)))}" height="${H}" fill="url(#edge)"/>
  ${[0, 1, 2].map((k) => `<rect x="${u(15)}" y="${u(205 + k * 19)}" width="${u(5)}" height="${u(5)}" rx="${u(1)}" fill="${ACCENT}"/>`).join("")}
</svg>`;
  const logo = await fitted(LOGO, u(146), u(146));
  const lines = ["Verified player counts", "Mods synced for you", "One click to join"];
  const layers = [{ input: await fitted(LOGO, u(146), u(146)).then((l) => l.buf), left: Math.round((W - logo.width) / 2), top: Math.round(u(30)) }];
  for (const [k, line] of lines.entries()) {
    const t = await text(line.toUpperCase(), Math.round(u(14)), FG, u(0.5));
    layers.push({ input: t.buf, left: Math.round(u(25)), top: Math.round(u(202 + k * 19) - (t.height - u(14)) / 2 - u(2.5)) });
  }
  const tag = await text("STAY ALIVE OUT THERE", Math.round(u(13)), ACCENT, u(1.4));
  layers.push({ input: tag.buf, left: Math.round((W - tag.width) / 2), top: Math.round(u(276)) });
  const png = await sharp(Buffer.from(bgSvg)).composite(layers).png().toBuffer();
  return writeBmp(scale === 100 ? "sidebar.bmp" : `sidebar-${scale}.bmp`, png, W, H);
}

// 150x57 at 100 %, at the right of the header band on every inner page, on the band's
// own colour (HEAD) so it has no edge: the gas mask, glowing, and the name beside it.
async function header(scale) {
  const s = scale / 100;
  const W = Math.round(150 * s);
  const H = Math.round(57 * s);
  const u = (n) => n * s;
  const bgSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}">
  <defs>
    <radialGradient id="g" cx="0.84" cy="0.5" r="0.42"><stop offset="0" stop-color="${GLOW}" stop-opacity="0.22"/><stop offset="1" stop-color="${GLOW}" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="${W}" height="${H}" fill="${HEAD}"/>
  <rect width="${W}" height="${H}" fill="url(#g)"/>
</svg>`;
  const mask = await fitted(EMBLEM, u(50), u(52));
  const name = await text("DZSA CRAYZ", Math.round(u(22)), FG, u(0.4));
  const sub = await text("LAUNCHER", Math.round(u(13)), ACCENT, u(2.2));
  const right = W - Math.round(u(6)) - mask.width;
  const textRight = right - Math.round(u(5));
  const png = await sharp(Buffer.from(bgSvg))
    .composite([
      { input: mask.buf, left: right, top: Math.round((H - mask.height) / 2) },
      { input: name.buf, left: textRight - name.width, top: Math.round(u(8)) },
      { input: sub.buf, left: textRight - sub.width, top: Math.round(u(31)) },
    ])
    .png()
    .toBuffer();
  return writeBmp(scale === 100 ? "header.bmp" : `header-${scale}.bmp`, png, W, H, HEAD);
}

// The splash an interactive install opens with: the logo on its own, floating. The
// splash window takes one colour as transparent and nothing in between, so every pixel
// is either the logo, laid over the dark it is drawn against, or exactly that colour.
const KEY = { r: 255, g: 0, b: 255 };
async function splash() {
  const SIZE = 400;
  const { data, info } = await sharp(LOGO).resize(SIZE, SIZE, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 }, kernel: "lanczos3" }).raw().toBuffer({ resolveWithObject: true });
  const rgb = Buffer.alloc(info.width * info.height * 3);
  const dark = [11, 13, 16];
  for (let i = 0; i < info.width * info.height; i++) {
    const a = data[i * 4 + 3] / 255;
    const px = a < 0.5 ? [KEY.r, KEY.g, KEY.b] : [0, 1, 2].map((k) => Math.round(data[i * 4 + k] * a + dark[k] * (1 - a)));
    rgb[i * 3] = px[0];
    rgb[i * 3 + 1] = px[1];
    rgb[i * 3 + 2] = px[2];
  }
  writeFileSync(path.join(OUT, "splash.bmp"), toBmp(rgb, info.width, info.height));
  return "splash.bmp";
}

const written = [];
for (const scale of SCALES) written.push(await sidebar(scale), await header(scale));
written.push(await splash());
let bytes = 0;
for (const f of written) bytes += (await import("node:fs")).statSync(path.join(OUT, f)).size;
console.log(`nsis_art: wrote ${written.length} bitmaps to src-tauri/nsis (${(bytes / 1024 / 1024).toFixed(2)} MB before compression)`);

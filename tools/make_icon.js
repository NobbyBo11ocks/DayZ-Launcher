// Generates the app icon (D-258): the skull in its gas mask, cut out of the launcher's
// logo, with its eyes glowing. The whole logo was tried first and is a coloured blob at
// the 16-32 px the title bar and taskbar use; the mask alone still reads as a skull with
// two lit eyes at 16 px, and the user asked for exactly that.
//
// The cut: shapes traced on a 10 px grid over docs/art/logo.png give the outline, and it
// is pulled in wherever it runs into the orange and green glow around the mask. Then a
// dark outline and a green halo go under it, so it holds on a light taskbar as well as
// a dark one, and a light on each eye.
//
// Writes docs/art/emblem.png (the cut-out, reused by the installer art, the README and
// the site), in src-tauri/icons a multi-size .ico plus the PNG sizes Tauri bundles, and
// the app's own two marks in src/assets: the emblem for the title bar, where a framed
// window shows its icon, and the logo for the welcome dialog, both at 2x.
//
// Usage: npm i --no-save sharp && node tools/make_icon.js [--preview]
//        (after tools/logo_cutout.js when the logo changes)
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
const LOGO = path.join(ROOT, "docs", "art", "logo.png");
const EMBLEM = path.join(ROOT, "docs", "art", "emblem.png");
const ASSETS = path.join(ROOT, "src", "assets");

// Where the mask sits in logo.png, and its outline: cranium, cheeks, the mask itself,
// and its three filters. Traced a little outside the dark rim; the glow trims the rest.
const REGION = { left: 246, top: 190, width: 232, height: 262 };
const OUTLINE = `
  <ellipse cx="361" cy="274" rx="75" ry="71"/>
  <rect x="289" y="292" width="144" height="62" rx="18"/>
  <polygon points="300,346 422,346 446,378 410,430 361,438 312,430 276,378"/>
  <ellipse cx="288" cy="398" rx="28" ry="39"/>
  <ellipse cx="433" cy="397" rx="28" ry="39"/>
  <circle cx="361" cy="396" r="34"/>`;
const EYES = [
  [326, 318],
  [397, 318],
];
const HALO = "#7dff2a";
const RIM = "#07090b";

/** Single-channel buffer from a sharp pipeline (blur hands a grey image back as RGB). */
const grey = (img) => img.extractChannel(0).raw().toBuffer();

async function cutEmblem() {
  const R = REGION;
  const W = R.width;
  const H = R.height;
  const N = W * H;
  const { data } = await sharp(LOGO).extract(R).raw().toBuffer({ resolveWithObject: true });
  // The traced outline, drawn at 4x and brought down, so its own edge is smooth.
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W * 4}" height="${H * 4}" viewBox="${R.left} ${R.top} ${W} ${H}"><g fill="#fff">${OUTLINE}</g></svg>`;
  const traced = await sharp(Buffer.from(svg)).resize(W, H, { kernel: "lanczos3" }).extractChannel(3).raw().toBuffer();
  // Inside the outline and not glow. The eyes glow too, but they are walled in, so the
  // hole filling below brings them back.
  const keep = new Uint8Array(N);
  for (let i = 0; i < N; i++) {
    const r = data[i * 4];
    const g = data[i * 4 + 1];
    const b = data[i * 4 + 2];
    const mx = Math.max(r, g, b);
    const mn = Math.min(r, g, b);
    const glow = mx > 0.45 * 255 && (mx - mn) / mx > 0.55;
    keep[i] = traced[i] > 127 && !glow ? 1 : 0;
  }
  // The largest piece: a stray fleck of dark ring inside the outline is not the mask.
  const comp = new Int32Array(N).fill(-1);
  let best = -1;
  let bestN = 0;
  for (let s = 0, id = 0; s < N; s++) {
    if (!keep[s] || comp[s] >= 0) continue;
    const stack = [s];
    comp[s] = id;
    let n = 0;
    while (stack.length) {
      const i = stack.pop();
      n++;
      const x = i % W;
      for (const j of [x > 0 ? i - 1 : -1, x < W - 1 ? i + 1 : -1, i - W, i + W]) {
        if (j >= 0 && j < N && keep[j] && comp[j] < 0) {
          comp[j] = id;
          stack.push(j);
        }
      }
    }
    if (n > bestN) {
      bestN = n;
      best = id;
    }
    id++;
  }
  // Holes are whatever the border cannot reach.
  const outside = new Uint8Array(N);
  const stack = [];
  const seed = (i) => {
    if (comp[i] !== best && !outside[i]) {
      outside[i] = 1;
      stack.push(i);
    }
  };
  for (let x = 0; x < W; x++) {
    seed(x);
    seed((H - 1) * W + x);
  }
  for (let y = 0; y < H; y++) {
    seed(y * W);
    seed(y * W + W - 1);
  }
  while (stack.length) {
    const i = stack.pop();
    const x = i % W;
    if (x > 0) seed(i - 1);
    if (x < W - 1) seed(i + 1);
    if (i >= W) seed(i - W);
    if (i < N - W) seed(i + W);
  }
  const hard = Buffer.alloc(N);
  for (let i = 0; i < N; i++) hard[i] = outside[i] ? 0 : 255;
  const soft = await grey(sharp(hard, { raw: { width: W, height: H, channels: 1 } }).blur(0.6));
  const rgba = Buffer.from(data);
  for (let i = 0; i < N; i++) rgba[i * 4 + 3] = outside[i] ? soft[i] : 255;

  // Room for the halo, then halo and rim under the mask and a light on each eye.
  const P = 24;
  const CW = W + 2 * P;
  const CH = H + 2 * P;
  const cut = await sharp(rgba, { raw: { width: W, height: H, channels: 4 } })
    .extend({ top: P, bottom: P, left: P, right: P, background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();
  const shape = await sharp(cut).extractChannel(3).raw().toBuffer();
  const tint = async (hex, blur, gain) => {
    const a = await grey(sharp(shape, { raw: { width: CW, height: CH, channels: 1 } }).blur(blur));
    const [r, g, b] = [1, 3, 5].map((k) => parseInt(hex.slice(k, k + 2), 16));
    const out = Buffer.alloc(CW * CH * 4);
    for (let i = 0; i < CW * CH; i++) {
      out[i * 4] = r;
      out[i * 4 + 1] = g;
      out[i * 4 + 2] = b;
      out[i * 4 + 3] = Math.min(255, a[i] * gain);
    }
    return sharp(out, { raw: { width: CW, height: CH, channels: 4 } }).png().toBuffer();
  };
  const eyes = `<svg xmlns="http://www.w3.org/2000/svg" width="${CW}" height="${CH}">
    <defs><radialGradient id="g">
      <stop offset="0" stop-color="#fbff9a" stop-opacity="1"/>
      <stop offset="0.35" stop-color="#d8ff3c" stop-opacity="0.85"/>
      <stop offset="1" stop-color="#8cff1a" stop-opacity="0"/>
    </radialGradient></defs>
    ${EYES.map(([x, y]) => `<ellipse cx="${x - R.left + P}" cy="${y - R.top + P}" rx="17" ry="12" fill="url(#g)"/>`).join("")}
  </svg>`;
  return sharp({ create: { width: CW, height: CH, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
    .composite([{ input: await tint(HALO, 9, 0.9) }, { input: await tint(RIM, 1.6, 3.2) }, { input: cut }, { input: Buffer.from(eyes), blend: "screen" }])
    .png()
    .toBuffer();
}

const emblem = await cutEmblem();
writeFileSync(EMBLEM, await sharp(emblem).png({ compressionLevel: 9 }).toBuffer());

/**
 * The icon is framed on what can be seen, not on the canvas: `cutEmblem` keeps 24 px
 * round the mask for the halo's blur, and that faint tail took ~15 % of every icon
 * square — 3½ of a 24 px taskbar button's pixels (user: "slightly bigger", D-264).
 * Cropped to where the halo's alpha passes `threshold`, the mask is ~18 % larger at
 * every size; the few faint pixels past the crop are invisible at icon sizes.
 */
async function visible(img, threshold) {
  const { data, info } = await sharp(img).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  let [x0, y0, x1, y1] = [info.width, info.height, -1, -1];
  for (let y = 0; y < info.height; y++) {
    for (let x = 0; x < info.width; x++) {
      if (data[(y * info.width + x) * 4 + 3] > threshold) {
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      }
    }
  }
  return sharp(img).extract({ left: x0, top: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 }).png().toBuffer();
}
const framed = await visible(emblem, 4);
const meta = await sharp(framed).metadata();

/** The framed emblem centred on a transparent square; small sizes get a touch of sharpening. */
async function png(size) {
  const scale = size / Math.max(meta.width, meta.height);
  const w = Math.max(1, Math.round(meta.width * scale));
  const h = Math.max(1, Math.round(meta.height * scale));
  let fit = sharp(framed).resize(w, h, { kernel: "lanczos3" });
  if (size <= 32) fit = fit.sharpen({ sigma: 0.5 });
  return sharp({ create: { width: size, height: size, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
    .composite([{ input: await fit.png().toBuffer(), left: (size - w) >> 1, top: (size - h) >> 1 }])
    .png({ compressionLevel: 9 })
    .toBuffer();
}

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

// 20, 40 and 96 are what Windows asks for at 125 % and 150 % scaling; without them it
// scales the next size down itself, softly. 32 goes first: Tauri's codegen embeds the
// ICO's first entry as the window icon, which the taskbar draws at 32 px, and a 16 px
// first entry reached the taskbar as a blurred double (D-260). Windows itself picks
// from the group by size, whatever the order.
const icoSizes = [32, 16, 20, 24, 40, 48, 64, 96, 128, 256];
const images = [];
for (const size of icoSizes) images.push({ size, data: await png(size) });
const icoFile = ico(images);
writeFileSync(path.join(ICONS, "icon.ico"), icoFile);

// The PNGs the Tauri bundle and the Windows Store assets refer to.
const pngTargets = {
  "32x32.png": 32,
  "64x64.png": 64,
  "128x128.png": 128,
  "128x128@2x.png": 256,
  // Past the mask's own ~300 px these are upscaled, so no larger than Tauri's default
  // icon; source.png (1024) went with the drawn mark, the source is docs/art now.
  "icon.png": 512,
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
  // Nothing on Windows reads the 512 one; a quantised copy keeps it out of the way.
  const data = size >= 512 ? await sharp(await png(size)).png({ palette: true, quality: 90, effort: 10, compressionLevel: 9 }).toBuffer() : await png(size);
  writeFileSync(path.join(ICONS, name), data);
}

// The title bar shows the emblem 20 px tall and the welcome dialog the logo at 132 px.
writeFileSync(path.join(ASSETS, "emblem.png"), await sharp(emblem).resize({ height: 40, kernel: "lanczos3" }).png({ compressionLevel: 9 }).toBuffer());
writeFileSync(
  path.join(ASSETS, "logo.png"),
  await sharp(LOGO).resize(264, 264, { kernel: "lanczos3" }).png({ palette: true, quality: 92, effort: 10, dither: 0.6, compressionLevel: 9 }).toBuffer(),
);

if (process.argv.includes("--preview")) {
  writeFileSync(path.join(ROOT, "icon-preview.png"), await png(256));
  console.log("wrote icon-preview.png");
}
console.log(
  `make_icon: wrote docs/art/emblem.png, icons framed on its visible ${meta.width}x${meta.height}, icon.ico (${icoSizes.join(", ")}; ${(icoFile.length / 1024).toFixed(1)} KB) ` +
    `and ${Object.keys(pngTargets).length} PNG sizes to src-tauri/icons`,
);

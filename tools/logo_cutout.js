// Cuts the launcher logo out of its source picture (D-258). The source came as a JPEG
// with a transparency checkerboard painted into it — white and light-grey squares 11.7
// px wide — so there is no alpha to keep, only a background to take away:
//
//  1. Bright, colourless pixels reached from the border are background.
//  2. Pockets of the same, walled in by the barbed wire and the spikes, are background
//     when they are as neutral as the board. The white lettering is bright too, but
//     cream: warmer by 9 to 11 units of red over blue, so it stays.
//  3. The pixels along the cut are blends of logo and checker. Their alpha comes from
//     where each sits between the local background and the logo colour just inside it,
//     and the checker's share is taken back out of the colour, so no pale fringe is left
//     around the dark outline on a dark page.
//
// Writes docs/art/logo.png (RGBA, trimmed): the master every other picture is made from
// (make_icon.js, nsis_art.js, readme_banner.js). Only needed again for a new source.
//
// Usage: npm i --no-save sharp && node tools/logo_cutout.js [source.jpg] [--debug]
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let sharp;
try {
  sharp = require("sharp");
} catch {
  console.error("logo_cutout: needs sharp. Run:\n  npm i --no-save sharp && node tools/logo_cutout.js");
  process.exit(2);
}

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SRC = process.argv.slice(2).find((a) => !a.startsWith("--")) ?? path.join(ROOT, "docs", "art", "logo-source.jpg");
const OUT = path.join(ROOT, "docs", "art", "logo.png");
const DEBUG = process.argv.includes("--debug");

const { data, info } = await sharp(SRC).removeAlpha().raw().toBuffer({ resolveWithObject: true });
const W = info.width;
const H = info.height;
const N = W * H;
const rgb = (i) => [data[i * 3], data[i * 3 + 1], data[i * 3 + 2]];

// Step 1: bright and colourless, then a flood fill from the border.
const cand = new Uint8Array(N);
for (let i = 0; i < N; i++) {
  const [r, g, b] = rgb(i);
  const mx = Math.max(r, g, b);
  const mn = Math.min(r, g, b);
  cand[i] = mn >= 196 && mx - mn <= 14 ? 1 : 0;
}
const bg = new Uint8Array(N);
{
  const stack = [];
  const seed = (i) => {
    if (cand[i] && !bg[i]) {
      bg[i] = 1;
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
}

// Step 2: pockets of the same, walled in by the barbed wire and the spikes. The board is
// neutral grey: red and blue level to within a fraction of a unit on average. The logo's
// white lettering is cream, 9 to 11 units warmer. So a pocket as neutral as the board is
// background showing through, and a warm one is lettering. (The board cannot be matched
// square by square: it was painted in patches whose colours swap along ragged seams.)
const NEUTRAL = 4;
let pockets = 0;
let pocketPixels = 0;
{
  const seen = new Uint8Array(N);
  for (let s = 0; s < N; s++) {
    if (!cand[s] || bg[s] || seen[s]) continue;
    const members = [];
    const stack = [s];
    seen[s] = 1;
    let warmth = 0;
    while (stack.length) {
      const i = stack.pop();
      members.push(i);
      warmth += data[i * 3] - data[i * 3 + 2];
      const x = i % W;
      for (const j of [x > 0 ? i - 1 : -1, x < W - 1 ? i + 1 : -1, i - W, i + W]) {
        if (j >= 0 && j < N && cand[j] && !bg[j] && !seen[j]) {
          seen[j] = 1;
          stack.push(j);
        }
      }
    }
    warmth /= members.length;
    const background = Math.abs(warmth) < NEUTRAL;
    if (DEBUG && members.length >= 40) {
      const x = members[0] % W;
      console.log(`  pocket at ${x},${(members[0] - x) / W}: ${members.length} px, warmth ${warmth.toFixed(1)} -> ${background ? "background" : "kept"}`);
    }
    if (background) {
      pockets++;
      pocketPixels += members.length;
      for (const i of members) bg[i] = 1;
    }
  }
}

// JPEG noise leaves board pixels a little darker or tinted beside the dark wire, just
// outside the test above. Taken in a few rings at a time where they touch the
// background: light and near neutral. The logo's own light parts are walled off by
// its dark outline, so this never reaches them.
let grown = 0;
for (let pass = 0; pass < 4; pass++) {
  const add = [];
  for (let i = 0; i < N; i++) {
    if (bg[i]) continue;
    const [r, g, b] = rgb(i);
    const mx = Math.max(r, g, b);
    const mn = Math.min(r, g, b);
    if ((r + g + b) / 3 < 160 || mx - mn > 30) continue;
    const x = i % W;
    if ((x > 0 && bg[i - 1]) || (x < W - 1 && bg[i + 1]) || (i >= W && bg[i - W]) || (i < N - W && bg[i + W])) add.push(i);
  }
  for (const i of add) bg[i] = 1;
  grown += add.length;
  if (!add.length) break;
}
// Specks: islands of a few pixels left standing in the background.
let specks = 0;
{
  const seen = new Uint8Array(N);
  for (let s = 0; s < N; s++) {
    if (bg[s] || seen[s]) continue;
    const members = [];
    const stack = [s];
    seen[s] = 1;
    while (stack.length && members.length <= 24) {
      const i = stack.pop();
      members.push(i);
      const x = i % W;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const xx = x + dx;
          const j = i + dy * W + dx;
          if ((dx || dy) && xx >= 0 && xx < W && j >= 0 && j < N && !bg[j] && !seen[j]) {
            seen[j] = 1;
            stack.push(j);
          }
        }
      }
    }
    if (members.length <= 24 && !stack.length) {
      specks += members.length;
      for (const i of members) bg[i] = 1;
    }
  }
}

// Step 3: soft edge. Distance (in pixels, 8-connected) from the background, up to 4.
const dist = new Uint8Array(N).fill(255);
{
  let frontier = [];
  for (let i = 0; i < N; i++) if (bg[i]) { dist[i] = 0; frontier.push(i); }
  for (let d = 1; d <= 4; d++) {
    const next = [];
    for (const i of frontier) {
      const x = i % W;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (!dx && !dy) continue;
          const xx = x + dx;
          if (xx < 0 || xx >= W) continue;
          const j = i + dy * W + dx;
          if (j < 0 || j >= N || dist[j] !== 255) continue;
          dist[j] = d;
          next.push(j);
        }
      }
    }
    frontier = next;
  }
}
const out = Buffer.alloc(N * 4);
let softened = 0;
for (let i = 0; i < N; i++) {
  const [r, g, b] = rgb(i);
  let a = 255;
  let c = [r, g, b];
  if (bg[i]) a = 0;
  else if (dist[i] <= 2) {
    // Local background: the background pixels around it. Logo colour: the pixels a
    // little further in, which the checker never touched.
    const x = i % W;
    const y = (i - x) / W;
    let bs = [0, 0, 0], bn = 0, fs = [0, 0, 0], fn = 0;
    for (let rad = 2; rad <= 4 && (bn === 0 || fn === 0); rad++) {
      bs = [0, 0, 0]; bn = 0; fs = [0, 0, 0]; fn = 0;
      for (let dy = -rad; dy <= rad; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= H) continue;
        for (let dx = -rad; dx <= rad; dx++) {
          const xx = x + dx;
          if (xx < 0 || xx >= W) continue;
          const j = yy * W + xx;
          const p = rgb(j);
          if (bg[j]) { bs[0] += p[0]; bs[1] += p[1]; bs[2] += p[2]; bn++; }
          else if (dist[j] >= 3) { fs[0] += p[0]; fs[1] += p[1]; fs[2] += p[2]; fn++; }
        }
      }
    }
    if (bn && fn) {
      const B = bs.map((v) => v / bn);
      const F = fs.map((v) => v / fn);
      const d0 = F[0] - B[0], d1 = F[1] - B[1], d2 = F[2] - B[2];
      const dd = d0 * d0 + d1 * d1 + d2 * d2;
      if (dd >= 40 * 40) {
        const t = ((r - B[0]) * d0 + (g - B[1]) * d1 + (b - B[2]) * d2) / dd;
        const alpha = Math.min(1, Math.max(0, t));
        a = Math.round(alpha * 255);
        if (alpha >= 0.08) c = [r, g, b].map((v, k) => Math.min(255, Math.max(0, Math.round(B[k] + (v - B[k]) / alpha))));
        else c = F.map(Math.round);
        softened++;
      }
    }
  }
  out[i * 4] = c[0];
  out[i * 4 + 1] = c[1];
  out[i * 4 + 2] = c[2];
  out[i * 4 + 3] = a;
}

// Trim to what is left, with a little room.
let minX = W, minY = H, maxX = -1, maxY = -1;
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    if (out[(y * W + x) * 4 + 3] > 8) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }
}
const PAD = 4;
minX = Math.max(0, minX - PAD);
minY = Math.max(0, minY - PAD);
maxX = Math.min(W - 1, maxX + PAD);
maxY = Math.min(H - 1, maxY + PAD);
await sharp(out, { raw: { width: W, height: H, channels: 4 } })
  .extract({ left: minX, top: minY, width: maxX - minX + 1, height: maxY - minY + 1 })
  .png({ compressionLevel: 9 })
  .toFile(OUT);
console.log(
  `logo_cutout: ${pockets} walled-in pockets (${pocketPixels} px), ${grown} px of noisy board and ${specks} px of specks taken, ${softened} edge pixels softened; ` +
    `wrote ${path.relative(ROOT, OUT)} (${maxX - minX + 1}x${maxY - minY + 1})`,
);
if (DEBUG) {
  const dbg = Buffer.alloc(N * 3);
  for (let i = 0; i < N; i++) {
    const a = out[i * 4 + 3] / 255;
    // On magenta, so a pale fringe or a missed pocket shows at once.
    dbg[i * 3] = Math.round(out[i * 4] * a + 255 * (1 - a));
    dbg[i * 3 + 1] = Math.round(out[i * 4 + 1] * a);
    dbg[i * 3 + 2] = Math.round(out[i * 4 + 2] * a + 255 * (1 - a));
  }
  const file = path.join(path.dirname(OUT), "logo-debug.png");
  await sharp(dbg, { raw: { width: W, height: H, channels: 3 } }).png().toFile(file);
  console.log(`logo_cutout: wrote ${file}`);
}

// The README banner (D-202), drawn from the same mark and palette as the app icon and
// the installer artwork so all three stay one thing. Two files, because GitHub serves a
// different one to readers in dark and light mode via `<picture>`.
//
// Rasterised to PNG rather than shipped as SVG: GitHub renders a referenced SVG with
// whatever fonts the reader has, and the layout here is tuned to the text widths.
//
// Usage: npm i --no-save sharp && node tools/readme_banner.js
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let sharp;
try {
  sharp = require("sharp");
} catch {
  console.error("readme_banner: needs sharp. Run:\n  npm i --no-save sharp && node tools/readme_banner.js");
  process.exit(2);
}

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "docs", "art");

const ACCENT = "#a3e635";
const ACCENT_DIM = "#6f9e1f";

const THEMES = {
  dark: { bg0: "#0f1216", bg1: "#161b22", tile0: "#1d2530", tile1: "#0f1216", edge: "#2f3947", fg: "#e6e9ee", muted: "#8b949e", rule: "#262d37", ink: ACCENT },
  light: { bg0: "#ffffff", bg1: "#f6f8fa", tile0: "#1d2530", tile1: "#0f1216", edge: "#cfd6de", fg: "#1f2328", muted: "#59636e", rule: "#d9dee5", ink: "#476f10" },
};

const W = 880;
const H = 232;

function svg(t) {
  // The icon's mark, scaled and placed on the left.
  const markSize = 104;
  const mx = 56;
  const my = (H - markSize) / 2;
  const s = markSize / 256;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>
    <linearGradient id="page" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="${t.bg0}" />
      <stop offset="1" stop-color="${t.bg1}" />
    </linearGradient>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="${t.tile0}" />
      <stop offset="1" stop-color="${t.tile1}" />
    </linearGradient>
    <linearGradient id="stroke" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="${ACCENT}" />
      <stop offset="1" stop-color="${ACCENT_DIM}" />
    </linearGradient>
    <linearGradient id="fade" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="${ACCENT}" stop-opacity="0.85" />
      <stop offset="1" stop-color="${ACCENT}" stop-opacity="0" />
    </linearGradient>
  </defs>

  <rect width="${W}" height="${H}" rx="14" fill="url(#page)" />
  <rect width="${W}" height="${H}" rx="14" fill="none" stroke="${t.rule}" />
  <rect x="0" y="0" width="${W}" height="3" fill="url(#fade)" />

  <g transform="translate(${mx} ${my}) scale(${s})">
    <rect x="8" y="8" width="240" height="240" rx="54" fill="url(#tile)" stroke="${t.edge}" stroke-width="4" />
    <circle cx="128" cy="128" r="84" fill="none" stroke="${ACCENT_DIM}" stroke-width="5" opacity="0.32" />
    <path d="M74 74 H182 L74 182 H182" fill="none" stroke="url(#stroke)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
  </g>

  <text x="196" y="88" font-family="Segoe UI, Inter, system-ui, sans-serif" font-size="34" font-weight="700" fill="${t.fg}" letter-spacing="-0.6">DZSA CrayZ Launcher</text>
  <text x="196" y="120" font-family="Segoe UI, Inter, system-ui, sans-serif" font-size="16" font-weight="500" fill="${t.ink}">A fast, honest server browser for DayZ Standalone</text>

  <g font-family="Segoe UI, Inter, system-ui, sans-serif" font-size="13" fill="${t.muted}">
    <text x="196" y="158">Player counts verified against the servers themselves</text>
    <text x="196" y="178">Workshop mods synced and the game started in one click</text>
    <text x="196" y="198">No accounts, no API keys, no telemetry</text>
  </g>

  <g transform="translate(${W - 196} ${H - 34})" font-family="Segoe UI, Inter, system-ui, sans-serif" font-size="12" fill="${t.muted}">
    <rect x="-14" y="-17" width="176" height="25" rx="12.5" fill="none" stroke="${t.rule}" />
    <text x="0" y="0">Windows 10 / 11 · 4.7 MB</text>
  </g>
</svg>`;
}

for (const [name, t] of Object.entries(THEMES)) {
  const file = path.join(OUT, `banner-${name}.png`);
  // 2x, so it stays crisp on the displays most people read GitHub on.
  await sharp(Buffer.from(svg(t)), { density: 144 })
    .resize(W * 2, H * 2, { fit: "fill" })
    .png({ compressionLevel: 9 })
    .toFile(file);
  console.log(`readme_banner: wrote ${file} (${W * 2}x${H * 2})`);
}

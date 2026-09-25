// The README banner (D-202, D-258): the launcher's logo on its own dark surfaces, with
// the three things it does. One picture for GitHub's light and dark modes alike: the
// logo is drawn for a dark ground, and the card's corners are cut away so it sits on
// either page.
//
// Rasterised to PNG rather than shipped as SVG: GitHub renders a referenced SVG with
// whatever fonts the reader has, and the layout here is tuned to the text widths. The
// text is Bebas Neue (tools/fonts, SIL OFL), as in the installer art.
//
// Usage: npm i --no-save sharp && node tools/readme_banner.js   (after tools/logo_cutout.js)
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
const OUT = path.join(ROOT, "docs", "art", "banner.png");
const LOGO = path.join(ROOT, "docs", "art", "logo.png");
const FONT = path.join(ROOT, "tools", "fonts", "BebasNeue-Regular.ttf");

const FG = "#e6e9ee";
const BODY = "#c9d1d9";
const MUTED = "#8b949e";
const ACCENT = "#a3e635";
const GLOW = "#7dff2a";
const EMBER = "#ff7a1a";

// Drawn at 2x, so it stays crisp on the displays most people read GitHub on.
const S = 2;
const W = 880 * S;
const H = 232 * S;
const u = (n) => Math.round(n * S);

async function text(str, px, colour, spacing = 0) {
  const esc = str.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const markup = `<span foreground="${colour}" letter_spacing="${Math.round(spacing * 1024)}">${esc}</span>`;
  const buf = await sharp({ text: { text: markup, font: `Bebas Neue ${px}`, fontfile: FONT, rgba: true, dpi: 72 } }).png().toBuffer();
  const { width, height } = await sharp(buf).metadata();
  return { buf, width, height };
}

const card = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#161b22"/><stop offset="1" stop-color="#0b0e12"/></linearGradient>
    <radialGradient id="g1" cx="0.14" cy="0.5" r="0.32"><stop offset="0" stop-color="${GLOW}" stop-opacity="0.22"/><stop offset="1" stop-color="${GLOW}" stop-opacity="0"/></radialGradient>
    <radialGradient id="g2" cx="0.14" cy="0.42" r="0.2"><stop offset="0" stop-color="${EMBER}" stop-opacity="0.14"/><stop offset="1" stop-color="${EMBER}" stop-opacity="0"/></radialGradient>
    <linearGradient id="fade" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${ACCENT}" stop-opacity="0.9"/><stop offset="1" stop-color="${ACCENT}" stop-opacity="0"/></linearGradient>
    <filter id="grain" x="0" y="0" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.45" numOctaves="2" seed="11"/>
      <feColorMatrix type="saturate" values="0"/>
      <feComponentTransfer><feFuncA type="linear" slope="0.06"/></feComponentTransfer>
    </filter>
    <clipPath id="round"><rect width="${W}" height="${H}" rx="${u(16)}"/></clipPath>
  </defs>
  <g clip-path="url(#round)">
    <rect width="${W}" height="${H}" fill="url(#bg)"/>
    <rect width="${W}" height="${H}" fill="url(#g1)"/>
    <rect width="${W}" height="${H}" fill="url(#g2)"/>
    <rect width="${W}" height="${H}" filter="url(#grain)"/>
    <rect x="0" y="0" width="${W}" height="${u(3)}" fill="url(#fade)"/>
    <rect x="${u(250)}" y="${u(104)}" width="${u(300)}" height="${u(1.5)}" fill="url(#fade)"/>
    ${[0, 1, 2].map((k) => `<rect x="${u(252)}" y="${u(124 + k * 26)}" width="${u(7)}" height="${u(7)}" rx="${u(1.5)}" fill="${ACCENT}"/>`).join("")}
    <rect x="${W - u(186)}" y="${H - u(44)}" width="${u(166)}" height="${u(26)}" rx="${u(13)}" fill="none" stroke="#2f3947" stroke-width="${S}"/>
  </g>
  <rect x="${S / 2}" y="${S / 2}" width="${W - S}" height="${H - S}" rx="${u(16)}" fill="none" stroke="#262d37" stroke-width="${S}"/>
</svg>`;

const logo = await sharp(LOGO).resize(u(210), u(210), { kernel: "lanczos3" }).png().toBuffer();
const layers = [{ input: logo, left: u(22), top: u(11) }];
const title = await text("DZSA CRAYZ LAUNCHER", u(50), FG, u(0.8));
layers.push({ input: title.buf, left: u(248), top: u(26) });
const sub = await text("A FAST, HONEST SERVER BROWSER FOR DAYZ STANDALONE", u(21), ACCENT, u(1.6));
layers.push({ input: sub.buf, left: u(250), top: u(76) });
const lines = ["Player counts verified against the servers themselves", "Workshop mods synced and the game started in one click", "No accounts, no API keys, no telemetry"];
for (const [k, line] of lines.entries()) {
  const t = await text(line.toUpperCase(), u(19), BODY, u(0.7));
  layers.push({ input: t.buf, left: u(268), top: u(119 + k * 26) });
}
const pill = await text("WINDOWS 10 / 11 · FREE", u(15), MUTED, u(1.4));
layers.push({ input: pill.buf, left: W - u(186) + Math.round((u(166) - pill.width) / 2), top: H - u(44) + Math.round((u(26) - pill.height) / 2) + S });

// Quantised: the grain and the logo's detail make a full-colour PNG about 1 MB.
const flat = await sharp(Buffer.from(card)).composite(layers).png().toBuffer();
await sharp(flat).png({ palette: true, quality: 92, effort: 10, dither: 0.6, compressionLevel: 9 }).toFile(OUT);
console.log(`readme_banner: wrote ${path.relative(ROOT, OUT)} (${W}x${H})`);

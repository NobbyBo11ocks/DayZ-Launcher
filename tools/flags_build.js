// Rasterises flag-icons' 4x3 country flags (MIT, docs/08) into one sprite so the
// server table can show a flag without shipping 250 SVGs or web fonts:
//   src/assets/flags.png   one row of 32×24 px flags (shown at 16×12, so crisp at 2×)
//   src/lib/flags.json     the ISO 3166-1 alpha-2 codes in sprite order
// Usage: node tools/flags_build.js        (needs the flag-icons and @resvg/resvg-js dev deps)
// Only two-letter ISO codes are included (no subdivisions like gb-eng); "xk" (Kosovo)
// is kept because DB-IP uses it.
import { createRequire } from "node:module";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { Resvg } = require("@resvg/resvg-js");

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcDir = path.join(root, "node_modules", "flag-icons", "flags", "4x3");
const W = 32;
const H = 24;

const codes = readdirSync(srcDir)
  .filter((f) => /^[a-z]{2}\.svg$/.test(f))
  .map((f) => f.slice(0, 2))
  .sort();

// Every flag SVG is embedded as a nested <svg> at its slot. Ids (clip paths,
// gradients) repeat across files, so they are prefixed per flag.
const parts = codes.map((cc, i) => {
  let svg = readFileSync(path.join(srcDir, `${cc}.svg`), "utf8");
  svg = svg.replace(/<\?xml[^>]*>/, "");
  svg = svg.replace(/\bid="([^"]+)"/g, `id="${cc}-$1"`);
  svg = svg.replace(/url\(#([^)]+)\)/g, `url(#${cc}-$1)`);
  svg = svg.replace(/\b(xlink:)?href="#([^"]+)"/g, `$1href="#${cc}-$2"`);
  // Position the flag: nested svg with explicit size, viewBox scales the content.
  svg = svg.replace(/<svg\b([^>]*)>/, (m, attrs) => {
    const cleaned = attrs.replace(/\s(width|height|x|y)="[^"]*"/g, "");
    return `<svg${cleaned} x="${i * W}" y="0" width="${W}" height="${H}">`;
  });
  return svg;
});

const sheet = `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${codes.length * W}" height="${H}">${parts.join("")}</svg>`;
const png = new Resvg(sheet, { fitTo: { mode: "original" } }).render().asPng();

mkdirSync(path.join(root, "src", "assets"), { recursive: true });
writeFileSync(path.join(root, "src", "assets", "flags.png"), png);
writeFileSync(path.join(root, "src", "lib", "flags.json"), JSON.stringify(codes) + "\n");
console.log(`${codes.length} flags → src/assets/flags.png (${png.length} bytes, ${codes.length * W}×${H})`);

// Generates the app icon source (1024×1024 RGBA PNG) without any image library:
// a dark rounded tile with an amber "Z" built from two bars and a diagonal band.
// Usage: node tools/make_icon.js [out.png]   (default src-tauri/icons/source.png)
// Then:  npx tauri icon src-tauri/icons/source.png
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const SIZE = 1024;
const out = process.argv[2] ?? "src-tauri/icons/source.png";

const BG = [0x16, 0x1b, 0x22, 255];
const EDGE = [0x26, 0x2d, 0x37, 255];
const ACCENT = [0xf0, 0xb4, 0x29, 255];

const px = new Uint8Array(SIZE * SIZE * 4); // transparent by default

function put(x, y, c) {
  const i = (y * SIZE + x) * 4;
  px[i] = c[0];
  px[i + 1] = c[1];
  px[i + 2] = c[2];
  px[i + 3] = c[3];
}

// Rounded square with a subtle edge.
const R = 200;
const inTile = (x, y, inset) => {
  const lo = inset;
  const hi = SIZE - 1 - inset;
  const r = R - inset;
  const cx = x < lo + r ? lo + r : x > hi - r ? hi - r : x;
  const cy = y < lo + r ? lo + r : y > hi - r ? hi - r : y;
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
};
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    if (inTile(x, y, 0)) put(x, y, inTile(x, y, 10) ? BG : EDGE);
  }
}

// "Z": top bar, bottom bar, diagonal band (top-right → bottom-left).
const barH = 84;
const left = 268;
const right = 756;
const top = 292;
const bottom = 732;
const diagW = 96;
for (let y = top; y < bottom; y++) {
  const t = (y - top) / (bottom - top);
  const cx = right - diagW / 2 - t * (right - left - diagW); // band centre at this row
  for (let x = left; x < right; x++) {
    const inTop = y < top + barH;
    const inBottom = y >= bottom - barH;
    const inDiag = Math.abs(x - cx) <= diagW / 2;
    if (inTop || inBottom || inDiag) put(x, y, ACCENT);
  }
}

// --- PNG encoding ---------------------------------------------------------
const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0; // filter: none
  Buffer.from(px.buffer, y * SIZE * 4, SIZE * 4).copy(raw, y * (SIZE * 4 + 1) + 1);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync(out, png);
console.log(`wrote ${out} (${png.length} bytes)`);

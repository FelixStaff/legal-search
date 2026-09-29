// Draws the LegalSearch icon (document + magnifier) procedurally and writes
// PNGs for the extension and an .ico for the installer. No dependencies.
//   node scripts/make-icons.mjs
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const CRC = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
const crc32 = (buf) => {
  let c = -1;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(size, rgba) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

// Signed-distance helpers in a unit square [0,1]^2.
const roundRect = (x, y, cx, cy, hw, hh, r) => {
  const qx = Math.abs(x - cx) - hw + r, qy = Math.abs(y - cy) - hh + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
};
const circle = (x, y, cx, cy, r) => Math.hypot(x - cx, y - cy) - r;
const segment = (x, y, ax, ay, bx, by, r) => {
  const px = x - ax, py = y - ay, dx = bx - ax, dy = by - ay;
  const t = Math.max(0, Math.min(1, (px * dx + py * dy) / (dx * dx + dy * dy)));
  return Math.hypot(px - dx * t, py - dy * t) - r;
};

function draw(size) {
  const out = Buffer.alloc(size * size * 4);
  const SS = 4; // supersampling
  const bg = [29, 78, 216], white = [255, 255, 255], line = [148, 163, 184];
  for (let py = 0; py < size; py++) for (let px = 0; px < size; px++) {
    let acc = [0, 0, 0, 0];
    for (let sy = 0; sy < SS; sy++) for (let sx = 0; sx < SS; sx++) {
      const x = (px + (sx + 0.5) / SS) / size, y = (py + (sy + 0.5) / SS) / size;
      let c = null;
      if (roundRect(x, y, 0.5, 0.5, 0.5, 0.5, 0.2) <= 0) c = bg;
      if (roundRect(x, y, 0.42, 0.45, 0.22, 0.29, 0.04) <= 0) c = white;
      if (size >= 32) for (const ly of [0.3, 0.39, 0.48]) if (segment(x, y, 0.29, ly, 0.52, ly, 0.018) <= 0) c = line;
      const ring = Math.abs(circle(x, y, 0.6, 0.6, 0.15)) - 0.05;
      if (ring <= 0 || segment(x, y, 0.71, 0.71, 0.85, 0.85, 0.06) <= 0) c = [15, 23, 42];
      if (circle(x, y, 0.6, 0.6, 0.1) <= 0) c = [219, 234, 254];
      if (c) { acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 255; }
    }
    const n = SS * SS, a = acc[3] / n, i = (py * size + px) * 4;
    const k = acc[3] ? 255 / acc[3] : 0;
    out[i] = acc[0] * k; out[i + 1] = acc[1] * k; out[i + 2] = acc[2] * k; out[i + 3] = a;
  }
  return png(size, out);
}

const iconDir = join(root, 'extension', 'static', 'icons');
mkdirSync(iconDir, { recursive: true });
for (const s of [16, 32, 48, 128]) writeFileSync(join(iconDir, `icon${s}.png`), draw(s));

// ICO with embedded PNG images (supported since Windows Vista).
const sizes = [16, 32, 48, 256];
const imgs = sizes.map(draw);
const header = Buffer.alloc(6 + 16 * sizes.length);
header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
let off = header.length;
sizes.forEach((s, i) => {
  const e = 6 + 16 * i;
  header[e] = s === 256 ? 0 : s; header[e + 1] = s === 256 ? 0 : s;
  header.writeUInt16LE(1, e + 4); header.writeUInt16LE(32, e + 6);
  header.writeUInt32LE(imgs[i].length, e + 8); header.writeUInt32LE(off, e + 12);
  off += imgs[i].length;
});
writeFileSync(join(root, 'installer', 'legalsearch.ico'), Buffer.concat([header, ...imgs]));
console.log('icons written');

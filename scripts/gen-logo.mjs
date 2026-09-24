// Renders orbit/assets/logo.png (512x512) — a rounded dark tile with a
// gradient orbit ring and a planet, drawn pixel-by-pixel and PNG-encoded with
// node:zlib only (no image libraries).
import zlib from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";

const crcTable = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])), 0);
  return Buffer.concat([len, t, data, crc]);
}

function encodePNG(w, h, rgba) {
  const stride = w * 4;
  const raw = Buffer.alloc((stride + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (stride + 1)] = 0;
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const smooth = (e0, e1, x) => {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};
const lerp = (a, b, t) => a + (b - a) * t;

const W = 512;
const H = 512;
const rgba = Buffer.alloc(W * H * 4);

const bg = [13, 17, 23];
const c1 = [108, 140, 255];
const c2 = [160, 108, 255];
const planetColor = [255, 184, 108];

const ROUND = 112;
const ringC = { x: 256, y: 230, r: 104, w: 26 };
const inner = ringC.r - ringC.w / 2;
const outer = ringC.r + ringC.w / 2;
const planet = { x: 330, y: 156, r: 28 };

function tileCoverage(x, y) {
  const dx = Math.max(Math.abs(x - 256) - (256 - ROUND), 0);
  const dy = Math.max(Math.abs(y - 256) - (256 - ROUND), 0);
  return 1 - smooth(-1, 1, Math.sqrt(dx * dx + dy * dy) - ROUND);
}

for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const px = x + 0.5;
    const py = y + 0.5;
    const cover = tileCoverage(px, py);

    let r = bg[0];
    let g = bg[1];
    let b = bg[2];

    const d = Math.hypot(px - ringC.x, py - ringC.y);
    const ringCov = smooth(inner - 1.5, inner + 1.5, d) * (1 - smooth(outer - 1.5, outer + 1.5, d));
    if (ringCov > 0) {
      const t = Math.min(1, Math.max(0, (px + py) / (W + H)));
      r = lerp(r, lerp(c1[0], c2[0], t), ringCov);
      g = lerp(g, lerp(c1[1], c2[1], t), ringCov);
      b = lerp(b, lerp(c1[2], c2[2], t), ringCov);
    }

    const pd = Math.hypot(px - planet.x, py - planet.y);
    const pCov = 1 - smooth(planet.r - 1.5, planet.r + 1.5, pd);
    if (pCov > 0) {
      r = lerp(r, planetColor[0], pCov);
      g = lerp(g, planetColor[1], pCov);
      b = lerp(b, planetColor[2], pCov);
    }

    const i = (y * W + x) * 4;
    rgba[i] = Math.round(r);
    rgba[i + 1] = Math.round(g);
    rgba[i + 2] = Math.round(b);
    rgba[i + 3] = Math.round(cover * 255);
  }
}

mkdirSync("assets", { recursive: true });
writeFileSync("assets/logo.png", encodePNG(W, H, rgba));
console.log("wrote assets/logo.png");

// Generates orbit/icon-source.png (1024x1024) with a minimal orbit motif.
// No dependencies: uses node:zlib to hand-encode a PNG.
import zlib from "node:zlib";
import { writeFileSync } from "node:fs";

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

function encodePNG(width, height, rgba) {
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const idat = zlib.deflateSync(raw, { level: 9 });
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);
}

function smoothstep(e0, e1, x) {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
}

const W = 1024;
const H = 1024;
const rgba = Buffer.alloc(W * H * 4);
const bg = [15, 17, 21];
const ring = [108, 140, 255];
const planet = [255, 184, 108];

const cx = 512;
const cy = 512;
const inner = 250;
const outer = 330;
const mid = (inner + outer) / 2;
const angle = -Math.PI / 4;
const px = cx + mid * Math.cos(angle);
const py = cy + mid * Math.sin(angle);
const pr = 80;

for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const dx = x - cx;
    const dy = y - cy;
    const d = Math.sqrt(dx * dx + dy * dy);
    let r = bg[0];
    let g = bg[1];
    let b = bg[2];

    const cover = smoothstep(inner - 1.5, inner + 1.5, d) * (1 - smoothstep(outer - 1.5, outer + 1.5, d));
    r += (ring[0] - r) * cover;
    g += (ring[1] - g) * cover;
    b += (ring[2] - b) * cover;

    const pdx = x - px;
    const pdy = y - py;
    const pd = Math.sqrt(pdx * pdx + pdy * pdy);
    const pc = 1 - smoothstep(pr - 1.5, pr + 1.5, pd);
    r += (planet[0] - r) * pc;
    g += (planet[1] - g) * pc;
    b += (planet[2] - b) * pc;

    const i = (y * W + x) * 4;
    rgba[i] = Math.round(r);
    rgba[i + 1] = Math.round(g);
    rgba[i + 2] = Math.round(b);
    rgba[i + 3] = 255;
  }
}

writeFileSync("icon-source.png", encodePNG(W, H, rgba));
console.log("wrote icon-source.png", rgba.length, "bytes rgba");

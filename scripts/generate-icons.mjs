import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { deflateSync } from 'node:zlib';

const root = resolve(import.meta.dirname, '..');
const iconDir = resolve(root, 'src-tauri/icons');
mkdirSync(iconDir, { recursive: true });

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const typeBuffer = Buffer.from(type, 'ascii');
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuffer, data])), 0);
  return Buffer.concat([length, typeBuffer, data, crc]);
}

function png(size) {
  const pixels = Buffer.alloc(size * size * 4);
  const set = (x, y, r, g, b, a = 255) => {
    if (x < 0 || y < 0 || x >= size || y >= size) return;
    const i = (y * size + x) * 4;
    pixels[i] = r; pixels[i + 1] = g; pixels[i + 2] = b; pixels[i + 3] = a;
  };

  const scale = size / 256;
  const insideRoundRect = (x, y, left, top, right, bottom, radius) => {
    const cx = Math.min(Math.max(x, left + radius), right - radius);
    const cy = Math.min(Math.max(y, top + radius), bottom - radius);
    const dx = x - cx; const dy = y - cy;
    return dx * dx + dy * dy <= radius * radius;
  };

  // Dark rounded tile.
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      if (insideRoundRect(x, y, 12 * scale, 12 * scale, 244 * scale, 244 * scale, 54 * scale)) {
        set(x, y, 10, 10, 11, 255);
      }
    }
  }

  // Edge-attached notch body.
  for (let y = Math.floor(50 * scale); y < Math.ceil(206 * scale); y += 1) {
    for (let x = Math.floor(146 * scale); x < size; x += 1) set(x, y, 0, 0, 0, 255);
  }

  // Three usage rings.
  const rings = [
    { cy: 82, color: [0, 255, 136], sweep: 0.72 },
    { cy: 128, color: [242, 255, 0], sweep: 0.48 },
    { cy: 174, color: [255, 63, 0], sweep: 0.88 },
  ];
  for (const ring of rings) {
    const cx = 186 * scale; const cy = ring.cy * scale;
    const radius = 15 * scale; const width = Math.max(1, 4 * scale);
    for (let y = Math.floor(cy - radius - width); y <= Math.ceil(cy + radius + width); y += 1) {
      for (let x = Math.floor(cx - radius - width); x <= Math.ceil(cx + radius + width); x += 1) {
        const dx = x + 0.5 - cx; const dy = y + 0.5 - cy;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (Math.abs(dist - radius) <= width / 2) {
          set(x, y, 48, 48, 48, 255);
          let angle = Math.atan2(dy, dx) + Math.PI / 2;
          if (angle < 0) angle += Math.PI * 2;
          if (angle <= Math.PI * 2 * ring.sweep) set(x, y, ...ring.color, 255);
        }
      }
    }
  }

  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y += 1) {
    const row = y * (size * 4 + 1);
    raw[row] = 0;
    pixels.copy(raw, row + 1, y * size * 4, (y + 1) * size * 4);
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

function ico(pngBuffer, size = 256) {
  const header = Buffer.alloc(22);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  header[6] = size >= 256 ? 0 : size;
  header[7] = size >= 256 ? 0 : size;
  header[8] = 0; header[9] = 0;
  header.writeUInt16LE(1, 10);
  header.writeUInt16LE(32, 12);
  header.writeUInt32LE(pngBuffer.length, 14);
  header.writeUInt32LE(22, 18);
  return Buffer.concat([header, pngBuffer]);
}

const icon32 = png(32);
const icon128 = png(128);
const icon256 = png(256);
writeFileSync(resolve(iconDir, '32x32.png'), icon32);
writeFileSync(resolve(iconDir, '128x128.png'), icon128);
writeFileSync(resolve(iconDir, '128x128@2x.png'), icon256);
writeFileSync(resolve(iconDir, 'icon.ico'), ico(icon256));
console.log(`Generated Tauri icons in ${iconDir}`);

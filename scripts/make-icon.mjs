// 生成应用图标：256x256 的圆角方块 + 白色字母 A。
//
// 手写 PNG 编码而不引 sharp/canvas 这类原生依赖：
// 图标只在开发时生成一次，为它装几十 MB 的包不值得。
// 生成的 png 和 ico 都已提交到仓库，正常开发不需要重跑这个脚本。
//
// 用法：node scripts/make-icon.mjs

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const outDir = join(root, 'src-tauri', 'icons');

const S = 256;
const px = Buffer.alloc(S * S * 4);

/** 写一个像素。坐标超出画布时静默忽略。 */
function setPx(x, y, r, g, b, a) {
  if (x < 0 || y < 0 || x >= S || y >= S) return;
  const i = (y * S + x) * 4;
  px[i] = r;
  px[i + 1] = g;
  px[i + 2] = b;
  px[i + 3] = a;
}

// 圆角矩形背景。圆角半径 52，颜色用品牌绿 #10a37f。
const R = 52;
for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    let inside = true;
    const cx = x < R ? R : x > S - 1 - R ? S - 1 - R : x;
    const cy = y < R ? R : y > S - 1 - R ? S - 1 - R : y;
    if ((x < R || x > S - 1 - R) && (y < R || y > S - 1 - R)) {
      const dx = x - cx;
      const dy = y - cy;
      inside = dx * dx + dy * dy <= R * R;
    }
    if (inside) setPx(x, y, 0x10, 0xa3, 0x7f, 255);
    else setPx(x, y, 0, 0, 0, 0);
  }
}

// 字母 A：两条斜边 + 一条横杠
for (let y = 70; y < 190; y++) {
  const t = (y - 70) / 120;
  const left = Math.round(128 - 58 * t);
  const right = Math.round(128 + 58 * t);
  for (let w = 0; w < 17; w++) {
    setPx(left + w, y, 255, 255, 255, 255);
    setPx(right - w, y, 255, 255, 255, 255);
  }
}
for (let y = 140; y < 157; y++) {
  for (let x = 90; x < 167; x++) setPx(x, y, 255, 255, 255, 255);
}

/** 组装一个 PNG chunk，带 CRC32。 */
const crcTable = (() => {
  const t = [];
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  let crc = 0xffffffff;
  for (const b of td) crc = crcTable[(crc ^ b) & 0xff] ^ (crc >>> 8);
  const cb = Buffer.alloc(4);
  cb.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([len, td, cb]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; // 位深
ihdr[9] = 6; // RGBA

// PNG 每行前面要加一个过滤字节，这里统一用 0（无过滤）
const raw = Buffer.alloc(S * (S * 4 + 1));
for (let y = 0; y < S; y++) {
  raw[y * (S * 4 + 1)] = 0;
  px.copy(raw, y * (S * 4 + 1) + 1, y * S * 4, (y + 1) * S * 4);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]);

// ICO 容器。256 像素在 ICO 头里用 0 表示，单个条目直接内嵌 PNG 数据。
const ico = Buffer.alloc(22);
ico.writeUInt16LE(0, 0);
ico.writeUInt16LE(1, 2);
ico.writeUInt16LE(1, 4);
ico[6] = 0;
ico[7] = 0;
ico[8] = 0;
ico[9] = 0;
ico.writeUInt16LE(1, 10);
ico.writeUInt16LE(32, 12);
ico.writeUInt32LE(png.length, 14);
ico.writeUInt32LE(22, 18);

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, 'icon.png'), png);
writeFileSync(join(outDir, 'icon.ico'), Buffer.concat([ico, png]));

console.log(`已生成 icon.png (${png.length} 字节) 与 icon.ico`);

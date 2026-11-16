// 生成 agentcenter 应用图标：中心管理节点以不规则布局连接五个黑色圆点。
// 图标使用纯 Node.js 绘制并以 4 倍分辨率抗锯齿，不依赖额外图形库。
// 用法：node scripts/make-icon.mjs [warmLight|deepSea|green]

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const outDir = join(root, 'src-tauri', 'icons');
const S = 256;
const SCALE = 4;
const HI = S * SCALE;
const hiPixels = Buffer.alloc(HI * HI * 4);

/**
 * 用途：在高分辨率画布指定像素颜色。
 * 参数：x、y 为像素坐标；color 为 RGBA 四通道颜色。
 * 返回：无；坐标超出画布时不写入。
 * 设计意图：所有图形先以 4 倍尺寸绘制，再平均缩小以平滑边缘。
 */
function setPixel(x, y, color) {
  if (x < 0 || y < 0 || x >= HI || y >= HI) return;
  const i = (y * HI + x) * 4;
  hiPixels[i] = color[0];
  hiPixels[i + 1] = color[1];
  hiPixels[i + 2] = color[2];
  hiPixels[i + 3] = color[3];
}

/**
 * 用途：判断采样点是否落在圆角方形内。
 * 参数：x、y 为高分辨率画布坐标。
 * 返回：点位于图标底板内时为 true，否则为 false。
 * 设计意图：让透明圆角与 Tauri 窗口、桌面图标保持一致。
 */
function insideRoundedSquare(x, y) {
  const radius = 52 * SCALE;
  const cx = Math.max(radius, Math.min(HI - 1 - radius, x));
  const cy = Math.max(radius, Math.min(HI - 1 - radius, y));
  return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2;
}

/**
 * 用途：在底板绘制中心节点与四周节点之间的圆头连线。
 * 参数：起点和终点使用 256 像素设计坐标，width 为线宽，color 为 RGBA。
 * 返回：无；线段覆盖的画布像素会写入颜色。
 * 设计意图：用短而粗的连线表示 agentcenter 汇集并管理多个助手。
 */
function drawLine(x1, y1, x2, y2, width, color) {
  const ax = x1 * SCALE;
  const ay = y1 * SCALE;
  const bx = x2 * SCALE;
  const by = y2 * SCALE;
  const radius = (width * SCALE) / 2;
  const vx = bx - ax;
  const vy = by - ay;
  const lengthSquared = vx * vx + vy * vy;
  const minX = Math.max(0, Math.floor(Math.min(ax, bx) - radius));
  const maxX = Math.min(HI - 1, Math.ceil(Math.max(ax, bx) + radius));
  const minY = Math.max(0, Math.floor(Math.min(ay, by) - radius));
  const maxY = Math.min(HI - 1, Math.ceil(Math.max(ay, by) + radius));

  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      const px = x + 0.5 - ax;
      const py = y + 0.5 - ay;
      const t = Math.max(0, Math.min(1, (px * vx + py * vy) / lengthSquared));
      if ((px - t * vx) ** 2 + (py - t * vy) ** 2 <= radius ** 2) setPixel(x, y, color);
    }
  }
}

/**
 * 用途：在画布指定位置绘制圆形节点。
 * 参数：cx、cy、radius 使用 256 像素设计坐标，color 为 RGBA。
 * 返回：无；圆形覆盖的画布像素会写入颜色。
 * 设计意图：外层圆提供清楚轮廓，内层色点让不同助手易于区分。
 */
function drawCircle(cx, cy, radius, color) {
  const centerX = cx * SCALE;
  const centerY = cy * SCALE;
  const scaledRadius = radius * SCALE;
  const minX = Math.max(0, Math.floor(centerX - scaledRadius));
  const maxX = Math.min(HI - 1, Math.ceil(centerX + scaledRadius));
  const minY = Math.max(0, Math.floor(centerY - scaledRadius));
  const maxY = Math.min(HI - 1, Math.ceil(centerY + scaledRadius));

  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      if ((x + 0.5 - centerX) ** 2 + (y + 0.5 - centerY) ** 2 <= scaledRadius ** 2) {
        setPixel(x, y, color);
      }
    }
  }
}

/**
 * 用途：按 4 倍采样平均值生成 256×256 RGBA 像素。
 * 参数：无；读取高分辨率绘图缓冲区。
 * 返回：可直接写入 PNG 的低分辨率 Buffer。
 * 设计意图：以面积平均缩放，令圆角、连线和节点在小尺寸下保持清晰。
 */
function downsample() {
  const pixels = Buffer.alloc(S * S * 4);
  const samples = SCALE * SCALE;
  for (let y = 0; y < S; y++) {
    for (let x = 0; x < S; x++) {
      const out = (y * S + x) * 4;
      const sums = [0, 0, 0, 0];
      for (let sy = 0; sy < SCALE; sy++) {
        for (let sx = 0; sx < SCALE; sx++) {
          const i = (((y * SCALE + sy) * HI) + x * SCALE + sx) * 4;
          sums[0] += hiPixels[i];
          sums[1] += hiPixels[i + 1];
          sums[2] += hiPixels[i + 2];
          sums[3] += hiPixels[i + 3];
        }
      }
      for (let channel = 0; channel < 4; channel++) {
        pixels[out + channel] = Math.round(sums[channel] / samples);
      }
    }
  }
  return pixels;
}

/**
 * 用途：创建含长度与 CRC32 校验码的 PNG 数据块。
 * 参数：type 为四字符块类型，data 为块内容 Buffer。
 * 返回：可拼接进 PNG 文件的完整数据块 Buffer。
 * 设计意图：不增加 PNG 编码依赖，直接生成 Tauri 可读取的图标文件。
 */
function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const typeAndData = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  let crc = 0xffffffff;
  for (const byte of typeAndData) crc = crcTable[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([len, typeAndData, checksum]);
}

const crcTable = (() => {
  const table = [];
  for (let n = 0; n < 256; n++) {
    let value = n;
    for (let k = 0; k < 8; k++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    table[n] = value >>> 0;
  }
  return table;
})();

// 生成不同底色的预览时只替换配色，节点布局和图形比例保持一致。
const palettes = {
  green: {
    output: 'icon-green-preview.png', background: [0x10, 0xa3, 0x7f, 255],
    links: [225, 255, 246, 255],
    centerOuter: [255, 255, 255, 255], centerDot: [255, 255, 255, 255],
  },
  deepSea: {
    output: 'icon-deep-sea.png', background: [23, 59, 108, 255],
    links: [225, 245, 255, 255],
    centerOuter: [255, 255, 255, 255], centerDot: [255, 255, 255, 255],
  },
  warmLight: {
    output: 'icon.png', background: [244, 241, 234, 255],
    links: [45, 58, 68, 255],
    centerOuter: [45, 58, 68, 255], centerDot: [45, 58, 68, 255],
  },
};
const paletteName = process.argv[2] ?? 'warmLight';
const palette = palettes[paletteName];
if (!palette) throw new Error(`未知配色：${paletteName}。可用值：${Object.keys(palettes).join(', ')}`);
const { output, background, links, centerOuter, centerDot } = palette;
hiPixels.fill(0);
for (let y = 0; y < HI; y++) {
  for (let x = 0; x < HI; x++) {
    if (insideRoundedSquare(x + 0.5, y + 0.5)) setPixel(x, y, background);
  }
}

const hub = { x: 93, y: 123, radius: 29 };
const satellites = [
  { x: 47, y: 62, radius: 8 },
  { x: 207, y: 51, radius: 8 },
  { x: 214, y: 130, radius: 8 },
  { x: 151, y: 205, radius: 8 },
  { x: 47, y: 218, radius: 8 },
];

for (const node of satellites) {
  const dx = node.x - hub.x;
  const dy = node.y - hub.y;
  const length = Math.hypot(dx, dy);
  const ux = dx / length;
  const uy = dy / length;
  // 线段略伸进圆内；后绘制的圆盖住线头，避免抗锯齿后出现空隙。
  drawLine(hub.x + ux * (hub.radius - 1), hub.y + uy * (hub.radius - 1),
    node.x - ux * (node.radius - 1), node.y - uy * (node.radius - 1), 6, links);
}

for (const node of satellites) {
  drawCircle(node.x, node.y, node.radius, [17, 17, 17, 255]);
}

// 偏置的中心环标出统一管理入口，五条连线分别指向大小不同的助手节点。
drawCircle(hub.x, hub.y, hub.radius, centerOuter);
drawCircle(hub.x, hub.y, hub.radius - 8, background);
drawCircle(hub.x, hub.y, 7, centerDot);

const pixels = downsample();
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8;
ihdr[9] = 6;

// PNG 每行以过滤类型 0 开始，像素使用 RGBA 编码。
const raw = Buffer.alloc(S * (S * 4 + 1));
for (let y = 0; y < S; y++) {
  pixels.copy(raw, y * (S * 4 + 1) + 1, y * S * 4, (y + 1) * S * 4);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  pngChunk('IHDR', ihdr),
  pngChunk('IDAT', deflateSync(raw, { level: 9 })),
  pngChunk('IEND', Buffer.alloc(0)),
]);

// ICO 单项容器内嵌同一份 256×256 PNG。
const icoHeader = Buffer.alloc(22);
icoHeader.writeUInt16LE(0, 0);
icoHeader.writeUInt16LE(1, 2);
icoHeader.writeUInt16LE(1, 4);
icoHeader[6] = 0;
icoHeader[7] = 0;
icoHeader.writeUInt16LE(1, 10);
icoHeader.writeUInt16LE(32, 12);
icoHeader.writeUInt32LE(png.length, 14);
icoHeader.writeUInt32LE(22, 18);

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, output), png);
if (paletteName === 'warmLight') {
  writeFileSync(join(outDir, 'icon.ico'), Buffer.concat([icoHeader, png]));
  writeFileSync(join(outDir, 'icon-warm-light.png'), png);
}
console.log(`已生成 ${output} (${png.length} 字节)`);

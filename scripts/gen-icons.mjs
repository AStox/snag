import { deflateSync } from "node:zlib";
import { writeFileSync, copyFileSync } from "node:fs";

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const t = Buffer.from(type);
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const crcBuf = Buffer.concat([t, data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(crcBuf));
  return Buffer.concat([len, t, data, crc]);
}

function encodePNG(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0;
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", idat),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function drawIcon(size, template = false) {
  const rgba = Buffer.alloc(size * size * 4);
  const cx = (size - 1) / 2;
  const cy = (size - 1) / 2;
  const set = (x, y, r, g, b, a) => {
    if (x < 0 || y < 0 || x >= size || y >= size) return;
    const i = (y * size + x) * 4;
    const da = a / 255;
    rgba[i] = Math.round(r * da + rgba[i] * (1 - da));
    rgba[i + 1] = Math.round(g * da + rgba[i + 1] * (1 - da));
    rgba[i + 2] = Math.round(b * da + rgba[i + 2] * (1 - da));
    rgba[i + 3] = Math.min(255, rgba[i + 3] + a);
  };
  const bg = template ? [0, 0, 0, 0] : [28, 27, 24, 255];
  const radius = size * 0.22;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      if (!template) {
        const dx = Math.max(radius - x, 0, x - (size - 1 - radius));
        const dy = Math.max(radius - y, 0, y - (size - 1 - radius));
        const inside = x >= radius && x <= size - 1 - radius || y >= radius && y <= size - 1 - radius || dx * dx + dy * dy <= radius * radius;
        if (inside) set(x, y, bg[0], bg[1], bg[2], 255);
      }
    }
  }
  const ink = template ? [0, 0, 0] : [232, 214, 180];
  const accent = template ? [0, 0, 0] : [201, 163, 106];
  const ring = size * 0.28;
  const thick = Math.max(1.5, size * 0.045);
  const dot = Math.max(1.2, size * 0.07);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = x - cx;
      const dy = y - cy;
      const d = Math.sqrt(dx * dx + dy * dy);
      if (Math.abs(d - ring) < thick) set(x, y, accent[0], accent[1], accent[2], 255);
      if (d < dot) set(x, y, ink[0], ink[1], ink[2], 255);
      const arm = size * 0.16;
      const t = Math.max(1, size * 0.035);
      if (Math.abs(dx) < t && dy < -ring && dy > -ring - arm) set(x, y, ink[0], ink[1], ink[2], 230);
      if (Math.abs(dx) < t && dy > ring && dy < ring + arm) set(x, y, ink[0], ink[1], ink[2], 230);
      if (Math.abs(dy) < t && dx < -ring && dx > -ring - arm) set(x, y, ink[0], ink[1], ink[2], 230);
      if (Math.abs(dy) < t && dx > ring && dx < ring + arm) set(x, y, ink[0], ink[1], ink[2], 230);
    }
  }
  return encodePNG(size, size, rgba);
}

writeFileSync("src-tauri/icons/32x32.png", drawIcon(32));
writeFileSync("src-tauri/icons/128x128.png", drawIcon(128));
writeFileSync("src-tauri/icons/icon.png", drawIcon(512));
writeFileSync("src-tauri/icons/128x128@2x.png", drawIcon(256));
writeFileSync("src-tauri/icons/tray.png", drawIcon(44, true));
copyFileSync("src-tauri/fixtures/slack-thread.png", "public/fixtures/slack-thread.png");
copyFileSync("src-tauri/fixtures/github-pr.png", "public/fixtures/github-pr.png");
console.log("icons + fixtures copied");

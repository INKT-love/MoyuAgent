import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";

const size = 256;
const raw = Buffer.alloc((size * 4 + 1) * size);
for (let y = 0; y < size; y++) {
  for (let x = 0; x < size; x++) {
    const offset = y * (size * 4 + 1) + 1 + x * 4;
    const ink = x > 53 && x < 83 && y > 65 && y < 192 || x > 173 && x < 203 && y > 65 && y < 192 ||
      y > 65 && y < 137 && Math.abs(Math.abs(x - 128) - (137 - y) * .65) < 18;
    raw.set(ink ? [250, 252, 255, 255] : [23, 132, 117, 255], offset);
  }
}
const crc = (buffer) => {
  let result = -1;
  for (const byte of buffer) { result ^= byte; for (let i = 0; i < 8; i++) result = (result >>> 1) ^ (0xedb88320 & -(result & 1)); }
  return (result ^ -1) >>> 0;
};
function chunk(type, data) {
  const name = Buffer.from(type); const result = Buffer.alloc(data.length + 12);
  result.writeUInt32BE(data.length); name.copy(result, 4); data.copy(result, 8);
  result.writeUInt32BE(crc(Buffer.concat([name, data])), data.length + 8); return result;
}
const header = Buffer.alloc(13); header.writeUInt32BE(size); header.writeUInt32BE(size, 4); header[8] = 8; header[9] = 6;
mkdirSync("public", { recursive: true });
writeFileSync("public/moyu.png", Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]));

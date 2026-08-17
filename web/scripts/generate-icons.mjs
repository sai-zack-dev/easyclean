/**
 * Generates the PWA icon set into web/public/.
 *
 * Deliberately dependency-free: Node's zlib is enough to write a PNG, and
 * pulling a 30 MB image toolchain into the install just to draw four sparkles
 * is not a trade worth making. Run it when the mark changes:
 *
 *   node scripts/generate-icons.mjs
 *
 * Mark: true-black tile + a teal four-pointed sparkle (astroid) with a small
 * white companion — the "cleaned up" metaphor, in the OLED palette from ui.md.
 */
import { deflateSync } from 'node:zlib'
import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'public')

const BG = [0, 0, 0] //   --bg          true black
const TEAL = [45, 212, 191] // accent    #2DD4BF
const WHITE = [255, 255, 255]

/** 4× supersampling — cheap antialiasing without a rasterizer. */
const SS = 4

/** Astroid: |x|^(2/3) + |y|^(2/3) <= r^(2/3). A concave 4-point sparkle. */
const inSparkle = (dx, dy, r) =>
  Math.pow(Math.abs(dx), 2 / 3) + Math.pow(Math.abs(dy), 2 / 3) <= Math.pow(r, 2 / 3)

/** Rounded-square coverage test (corner radius as a fraction of the side). */
function inRoundedSquare(x, y, size, radiusFrac) {
  const r = size * radiusFrac
  const cx = Math.min(Math.max(x, r), size - r)
  const cy = Math.min(Math.max(y, r), size - r)
  const dx = x - cx
  const dy = y - cy
  return dx * dx + dy * dy <= r * r
}

/**
 * Draw one icon.
 * @param {number} size   pixel side
 * @param {boolean} maskable  full-bleed square (Android masks it itself) and a
 *                            smaller glyph so nothing lands in the crop zone
 */
function drawIcon(size, maskable) {
  const rgba = Buffer.alloc(size * size * 4)
  // Maskable icons get cropped to as little as 80% — keep the mark well inside.
  const scale = maskable ? 0.60 : 0.70
  const bigR = (size * scale) / 2
  const bigCx = size * 0.46
  const bigCy = size * 0.54
  const smallR = bigR * 0.3
  const smallCx = size * 0.74
  const smallCy = size * 0.26

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let bgHits = 0
      let tealHits = 0
      let whiteHits = 0

      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const px = x + (sx + 0.5) / SS
          const py = y + (sy + 0.5) / SS
          const inTile = maskable || inRoundedSquare(px, py, size, 0.22)
          if (!inTile) continue
          bgHits++
          if (inSparkle(px - smallCx, py - smallCy, smallR)) whiteHits++
          else if (inSparkle(px - bigCx, py - bigCy, bigR)) tealHits++
        }
      }

      const total = SS * SS
      const i = (y * size + x) * 4
      if (bgHits === 0) continue // transparent outside the rounded tile

      // Composite glyph over background, weighted by subsample coverage.
      const glyphHits = tealHits + whiteHits
      const plainBg = bgHits - glyphHits
      const mix = (ch) =>
        (BG[ch] * plainBg + TEAL[ch] * tealHits + WHITE[ch] * whiteHits) / bgHits

      rgba[i] = Math.round(mix(0))
      rgba[i + 1] = Math.round(mix(1))
      rgba[i + 2] = Math.round(mix(2))
      rgba[i + 3] = Math.round((bgHits / total) * 255)
    }
  }
  return rgba
}

// ── Minimal PNG encoder (RGBA8, no interlace) ───────────────────────────────

const CRC_TABLE = (() => {
  const t = new Int32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c
  }
  return t
})()

function crc32(buf) {
  let c = -1
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8)
  return (c ^ -1) >>> 0
}

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

function encodePng(rgba, size) {
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(size, 0)
  ihdr.writeUInt32BE(size, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // colour type: RGBA
  // 10-12: compression / filter / interlace, all 0

  // One filter byte (0 = None) per scanline.
  const raw = Buffer.alloc(size * (size * 4 + 1))
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4)
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

// ── Emit ────────────────────────────────────────────────────────────────────

mkdirSync(OUT, { recursive: true })

const icons = [
  ['icon-192.png', 192, false],
  ['icon-512.png', 512, false],
  ['icon-maskable-512.png', 512, true],
  ['apple-touch-icon.png', 180, true], // iOS masks it itself; no transparency
]

for (const [name, size, maskable] of icons) {
  writeFileSync(join(OUT, name), encodePng(drawIcon(size, maskable), size))
  console.log(`wrote public/${name} (${size}×${size}${maskable ? ', maskable' : ''})`)
}

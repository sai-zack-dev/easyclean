/**
 * Renders the brand mark into `web/public/`: the favicon plus the PWA icon set.
 *
 * The mark itself lives in `src/brand.ts` — this script reads MARK_PATH out of
 * that file so the in-app logo, the tab icon and the installed app icon are all
 * the same outline. Re-run it whenever the mark changes:
 *
 *   node scripts/generate-icons.mjs
 *
 * Deliberately dependency-free: the mark traces to a plain M/L/Z polygon, which
 * a scanline fill and Node's own zlib can turn into a PNG. Pulling a 30 MB
 * image toolchain into the install to draw one glyph is not a trade worth
 * making.
 *
 * Treatment: the true-black tile and white glyph of the OLED palette in ui.md.
 * An icon sits on whatever wallpaper or tab strip the OS gives it, so it brings
 * its own ground rather than inheriting the theme the way the in-app logo does.
 */
import { deflateSync } from 'node:zlib'
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const OUT = join(ROOT, 'public')

const BG = [0, 0, 0] //      --bg    true black
const FG = [255, 255, 255] // --text  white

/** Corner radius of the tile, as a fraction of its side. */
const TILE_RADIUS = 0.22
/** 4× supersampling down the y axis; x coverage is computed exactly. */
const SS = 4

// ── The mark ────────────────────────────────────────────────────────────────

/**
 * Pull MARK_PATH out of src/brand.ts. A build script can't import TypeScript,
 * and the alternative — a second copy of the path here — is exactly the drift
 * this script exists to prevent.
 */
function readMarkPath() {
  const src = readFileSync(join(ROOT, 'src', 'brand.ts'), 'utf8')
  const m = src.match(/export const MARK_PATH\s*=\s*'([^']+)'/)
  if (!m) throw new Error('could not find MARK_PATH in src/brand.ts')
  return m[1]
}

/** Parse the mark's M/L/Z path into closed rings of points. */
function parsePath(d) {
  const rings = []
  let ring = null
  for (const [, cmd, nums] of d.matchAll(/([MLZ])([-\d. ]*)/g)) {
    if (cmd === 'Z') {
      if (ring) rings.push(ring)
      ring = null
      continue
    }
    const [x, y] = nums.trim().split(/\s+/).map(Number)
    if (!Number.isFinite(x) || !Number.isFinite(y)) throw new Error(`bad point in path: "${nums}"`)
    if (cmd === 'M') rings.push((ring = []))
    ring.push([x, y])
  }
  return rings.filter((r) => r.length > 2)
}

const MARK_PATH = readMarkPath()
const MARK_RINGS = parsePath(MARK_PATH)

// ── Rasterising ─────────────────────────────────────────────────────────────

/**
 * Scanline-fill the mark into a coverage map (0..1 per pixel), non-zero winding.
 * Supersampled down y; across x each span contributes its exact overlap with
 * the pixel, which is where most of the smoothness comes from.
 */
function rasterMark(rings, size, scale, offX, offY) {
  const cov = new Float32Array(size * size)
  const edges = []
  for (const ring of rings) {
    for (let i = 0; i < ring.length; i++) {
      const [ax, ay] = ring[i]
      const [bx, by] = ring[(i + 1) % ring.length]
      if (ay === by) continue // horizontal edges never cross a scanline
      edges.push([ax * scale + offX, ay * scale + offY, bx * scale + offX, by * scale + offY])
    }
  }

  const xs = []
  for (let sy = 0; sy < size * SS; sy++) {
    const y = (sy + 0.5) / SS
    xs.length = 0
    for (const [ax, ay, bx, by] of edges) {
      if (ay <= y === by <= y) continue
      xs.push([ax + ((y - ay) * (bx - ax)) / (by - ay), by > ay ? 1 : -1])
    }
    if (!xs.length) continue
    xs.sort((p, q) => p[0] - q[0])

    const row = Math.floor(sy / SS) * size
    let winding = 0
    for (let i = 0; i < xs.length - 1; i++) {
      winding += xs[i][1]
      if (winding === 0) continue
      const xa = Math.max(xs[i][0], 0)
      const xb = Math.min(xs[i + 1][0], size)
      if (xb <= xa) continue
      // Distribute the span across the pixels it touches, by exact overlap.
      for (let px = Math.floor(xa); px < xb; px++) {
        const overlap = Math.min(xb, px + 1) - Math.max(xa, px)
        if (overlap > 0) cov[row + px] += overlap / SS
      }
    }
  }
  return cov
}

/** Coverage of a rounded square, supersampled. */
function rasterTile(size, radiusFrac) {
  const cov = new Float32Array(size * size)
  const r = size * radiusFrac
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) {
      if (radiusFrac === 0) {
        cov[y * size + x] = 1
        continue
      }
      let hits = 0
      for (let sy = 0; sy < SS; sy++)
        for (let sx = 0; sx < SS; sx++) {
          const px = x + (sx + 0.5) / SS
          const py = y + (sy + 0.5) / SS
          const cx = Math.min(Math.max(px, r), size - r)
          const cy = Math.min(Math.max(py, r), size - r)
          const dx = px - cx
          const dy = py - cy
          if (dx * dx + dy * dy <= r * r) hits++
        }
      cov[y * size + x] = hits / (SS * SS)
    }
  return cov
}

/**
 * Draw one icon: white mark on a black tile.
 * @param {number} size        pixel side
 * @param {boolean} fullBleed  square tile, and a smaller glyph — for maskable
 *                             icons (Android crops to as little as 80%) and for
 *                             iOS, which rounds the corners itself.
 */
function drawIcon(size, fullBleed) {
  // Fraction of the tile the mark's height takes up.
  const markScale = fullBleed ? 0.5 : 0.62
  const scale = (size * markScale) / 100
  const off = (size - 100 * scale) / 2

  const tile = rasterTile(size, fullBleed ? 0 : TILE_RADIUS)
  const mark = rasterMark(MARK_RINGS, size, scale, off, off)

  const rgba = Buffer.alloc(size * size * 4)
  for (let i = 0; i < size * size; i++) {
    const a = tile[i]
    if (a === 0) continue // transparent outside the tile
    const g = Math.min(mark[i], 1) * a // the glyph can't spill past the tile
    const p = i * 4
    for (let ch = 0; ch < 3; ch++) rgba[p + ch] = Math.round((BG[ch] * (a - g) + FG[ch] * g) / a)
    rgba[p + 3] = Math.round(a * 255)
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

// The favicon gets the same tile treatment as the PNGs, so a browser that
// prefers the SVG and one that falls back to icon-192 show the same thing.
const markScale = 0.62
const offset = (100 - 100 * markScale) / 2
const favicon =
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">` +
  `<rect width="100" height="100" rx="${100 * TILE_RADIUS}" fill="#000"/>` +
  `<path transform="translate(${offset} ${offset}) scale(${markScale})" d="${MARK_PATH}" fill="#fff"/>` +
  `</svg>\n`
writeFileSync(join(OUT, 'favicon.svg'), favicon)
console.log('wrote public/favicon.svg')

const icons = [
  ['icon-192.png', 192, false],
  ['icon-512.png', 512, false],
  ['icon-maskable-512.png', 512, true],
  ['apple-touch-icon.png', 180, true], // iOS masks it itself; no transparency
]

for (const [name, size, fullBleed] of icons) {
  writeFileSync(join(OUT, name), encodePng(drawIcon(size, fullBleed), size))
  console.log(`wrote public/${name} (${size}×${size}${fullBleed ? ', full-bleed' : ''})`)
}

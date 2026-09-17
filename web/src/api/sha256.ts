/*
 * SHA-256, incrementally.
 *
 * The server verifies every upload against the digest the client declared, so
 * the client has to produce one before it sends anything — and `crypto.subtle`
 * cannot help for the whole job. Its `digest()` is one-shot: it wants the
 * entire file as a single ArrayBuffer, which for a phone video means holding a
 * couple of gigabytes in memory to compute 32 bytes. That is how a tab gets
 * killed halfway through a batch.
 *
 * So: native crypto for anything small enough to hold comfortably, and this for
 * the rest, fed one slice at a time so memory stays flat whatever the file size
 * costs. It is slower than the native path, which is exactly why the native
 * path is tried first.
 */

const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
])

/** Largest file handed to `crypto.subtle` in one piece. */
const NATIVE_LIMIT = 32 * 1024 * 1024

/** Slice size when falling back to the incremental path. */
const SLICE = 4 * 1024 * 1024

function rotr(x: number, n: number): number {
  return (x >>> n) | (x << (32 - n))
}

class Sha256 {
  private h = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ])
  private readonly w = new Uint32Array(64)
  private readonly buf = new Uint8Array(64)
  /** Bytes currently held in `buf`, waiting for a full 64-byte block. */
  private buffered = 0
  private total = 0

  update(data: Uint8Array) {
    this.total += data.length
    let off = 0

    // Top up a partial block left by the previous slice first — file slices
    // are not multiples of 64, so this is the normal case, not an edge one.
    if (this.buffered > 0) {
      const take = Math.min(64 - this.buffered, data.length)
      this.buf.set(data.subarray(0, take), this.buffered)
      this.buffered += take
      off = take
      if (this.buffered === 64) {
        this.block(this.buf, 0)
        this.buffered = 0
      }
    }
    while (off + 64 <= data.length) {
      this.block(data, off)
      off += 64
    }
    if (off < data.length) {
      this.buf.set(data.subarray(off), 0)
      this.buffered = data.length - off
    }
  }

  /** Pad, absorb the length, and render lowercase hex. */
  digest(): string {
    const total = this.total
    // 0x80, then zeros, then an 8-byte big-endian bit count — sized so the
    // whole thing lands on a block boundary.
    const padLen = this.buffered < 56 ? 56 - this.buffered : 120 - this.buffered
    const tail = new Uint8Array(padLen + 8)
    tail[0] = 0x80
    const bits = total * 8
    // `>>> 0` is ToUint32, which is modulo 2^32 — correct for the low word at
    // any size we can reach. The high word divides by 2^32 first, folded into
    // the 2^3 from the bit conversion.
    const hi = Math.floor(total / 0x20000000)
    const lo = bits >>> 0
    const dv = new DataView(tail.buffer)
    dv.setUint32(padLen, hi >>> 0)
    dv.setUint32(padLen + 4, lo)
    this.update(tail)
    this.total = total // `update` counted the padding; it must not

    let out = ''
    for (let i = 0; i < 8; i++) out += (this.h[i] >>> 0).toString(16).padStart(8, '0')
    return out
  }

  private block(p: Uint8Array, o: number) {
    const w = this.w
    for (let i = 0; i < 16; i++) {
      const j = o + i * 4
      w[i] = ((p[j] << 24) | (p[j + 1] << 16) | (p[j + 2] << 8) | p[j + 3]) >>> 0
    }
    for (let i = 16; i < 64; i++) {
      const x = w[i - 15]
      const y = w[i - 2]
      const s0 = rotr(x, 7) ^ rotr(x, 18) ^ (x >>> 3)
      const s1 = rotr(y, 17) ^ rotr(y, 19) ^ (y >>> 10)
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0
    }

    let a = this.h[0]
    let b = this.h[1]
    let c = this.h[2]
    let d = this.h[3]
    let e = this.h[4]
    let f = this.h[5]
    let g = this.h[6]
    let h = this.h[7]

    for (let i = 0; i < 64; i++) {
      const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)
      const ch = (e & f) ^ (~e & g)
      const t1 = (h + S1 + ch + K[i] + w[i]) | 0
      const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)
      const maj = (a & b) ^ (a & c) ^ (b & c)
      const t2 = (S0 + maj) | 0
      h = g
      g = f
      f = e
      e = (d + t1) | 0
      d = c
      c = b
      b = a
      a = (t1 + t2) | 0
    }

    this.h[0] = (this.h[0] + a) | 0
    this.h[1] = (this.h[1] + b) | 0
    this.h[2] = (this.h[2] + c) | 0
    this.h[3] = (this.h[3] + d) | 0
    this.h[4] = (this.h[4] + e) | 0
    this.h[5] = (this.h[5] + f) | 0
    this.h[6] = (this.h[6] + g) | 0
    this.h[7] = (this.h[7] + h) | 0
  }
}

function toHex(buf: ArrayBuffer): string {
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

/**
 * The SHA-256 of a file, as lowercase hex.
 *
 * `onProgress` reports bytes read, for the files slow enough that the UI has to
 * say something — a 2 GB video takes long enough that a silent screen reads as
 * a hang.
 */
export async function hashFile(file: File, onProgress?: (read: number) => void): Promise<string> {
  if (file.size <= NATIVE_LIMIT && globalThis.crypto?.subtle) {
    try {
      const digest = await crypto.subtle.digest('SHA-256', await file.arrayBuffer())
      onProgress?.(file.size)
      return toHex(digest)
    } catch {
      // Fall through. `subtle` is unavailable over plain HTTP on some
      // browsers — which is exactly how this app is served on a LAN — so this
      // is a normal path, not an exceptional one.
    }
  }

  const h = new Sha256()
  let read = 0
  while (read < file.size) {
    const end = Math.min(read + SLICE, file.size)
    const chunk = new Uint8Array(await file.slice(read, end).arrayBuffer())
    h.update(chunk)
    read = end
    onProgress?.(read)
    // Yield between slices so a large file does not freeze the tab.
    await new Promise((r) => setTimeout(r, 0))
  }
  return h.digest()
}

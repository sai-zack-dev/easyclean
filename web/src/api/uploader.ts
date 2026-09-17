/*
 * Driving a batch of files to the server.
 *
 * The shape is: hash everything, ask once which digests the server already
 * holds, then transfer only what is left. That ordering is what makes offering
 * the same camera roll twice nearly free, and it is why hashing happens up
 * front rather than per file as each one's turn arrives.
 *
 * Every failure here is per file unless it is the kind that will obviously
 * repeat. A photo the server cannot read should not stop the other 199; a full
 * quota should stop all of them, because every remaining file is about to be
 * refused for the same reason.
 */

import { api, ApiError } from './client'
import { hashFile } from './sha256'

export type ItemState = 'queued' | 'hashing' | 'skipped' | 'uploading' | 'done' | 'failed'

export interface UploadItem {
  /** Local identity — `File` has no id, and names collide. */
  key: string
  file: File
  state: ItemState
  hash?: string
  /** Bytes confirmed on the server. */
  sent: number
  /** Bytes hashed so far, for the files slow enough to need a bar of their own. */
  hashed: number
  error?: string
  /** What the server actually called it, once filed. */
  savedAs?: string
}

export interface UploadSummary {
  uploaded: number
  skipped: number
  failed: number
  /** Set when the whole batch was stopped rather than merely finishing. */
  stoppedBecause?: string
}

/** Most digests in one `check`; mirrors the server's own cap. */
const CHECK_BATCH = 500

/** Files transferred at once. Two keeps a home link busy without collapsing
 *  onto a phone's limited sockets, and keeps the progress readable. */
const CONCURRENCY = 2

/** Attempts per chunk before the file is given up on. */
const CHUNK_RETRIES = 3

class Stop extends Error {
  readonly why: string

  constructor(why: string) {
    super(why)
    this.why = why
  }
}

export function makeItems(files: File[]): UploadItem[] {
  return files.map((file, i) => ({
    key: `${i}:${file.name}:${file.size}:${file.lastModified}`,
    file,
    state: 'queued',
    sent: 0,
    hashed: 0,
  }))
}

/**
 * Run a batch to completion.
 *
 * `onChange` is called whenever anything moves; the caller re-renders from the
 * array it is handed. `signal` aborts between chunks — mid-chunk cancellation
 * would leave the session's offset describing bytes the client thinks it never
 * sent, and the next call would have to reconcile that for no real gain.
 */
export async function runUpload(
  items: UploadItem[],
  onChange: (items: UploadItem[]) => void,
  signal: AbortSignal,
): Promise<UploadSummary> {
  const touch = () => onChange([...items])

  // ── 1. Hash
  for (const it of items) {
    if (signal.aborted) return tally(items, 'Cancelled')
    it.state = 'hashing'
    touch()
    try {
      it.hash = await hashFile(it.file, (read) => {
        it.hashed = read
        touch()
      })
      it.state = 'queued'
    } catch {
      it.state = 'failed'
      it.error = 'Could not read this file'
    }
    touch()
  }

  // ── 2. Ask what is already held
  const pending = items.filter((i) => i.hash && i.state === 'queued')
  const held = new Set<string>()
  try {
    const unique = [...new Set(pending.map((i) => i.hash!))]
    for (let i = 0; i < unique.length; i += CHECK_BATCH) {
      if (signal.aborted) return tally(items, 'Cancelled')
      const { have } = await api.upload.check(unique.slice(i, i + CHECK_BATCH))
      have.forEach((h) => held.add(h))
    }
  } catch {
    // Not fatal: without this the batch just re-sends things the server
    // already has, which is slow rather than wrong. `finish` still refuses
    // anything corrupt, and duplicate detection still catches the rest.
  }

  const seen = new Set<string>()
  const queue: UploadItem[] = []
  for (const it of pending) {
    // Two copies of one photo in the same selection is the other half of the
    // same problem — the server has not seen either yet, so `check` cannot
    // catch it.
    if (held.has(it.hash!) || seen.has(it.hash!)) {
      it.state = 'skipped'
      continue
    }
    seen.add(it.hash!)
    queue.push(it)
  }
  touch()

  // ── 3. Transfer
  let stopped: string | undefined
  let next = 0
  const worker = async () => {
    for (;;) {
      if (stopped || signal.aborted) return
      const it = queue[next++]
      if (!it) return
      try {
        await sendOne(it, touch, signal)
      } catch (e) {
        if (e instanceof Stop) {
          stopped = e.why
          return
        }
        it.state = 'failed'
        it.error = e instanceof Error ? e.message : 'Upload failed'
        touch()
      }
    }
  }
  await Promise.all(Array.from({ length: CONCURRENCY }, worker))

  // Anything still queued when a batch stops should say so rather than sit
  // looking as though it is about to start.
  if (stopped || signal.aborted) {
    for (const it of items) {
      if (it.state === 'queued' || it.state === 'uploading') {
        it.state = 'failed'
        it.error = stopped ? 'Not sent' : 'Cancelled'
      }
    }
    touch()
  }

  return tally(items, stopped ?? (signal.aborted ? 'Cancelled' : undefined))
}

async function sendOne(it: UploadItem, touch: () => void, signal: AbortSignal) {
  it.state = 'uploading'
  it.sent = 0
  touch()

  let session: { id: string; chunk_size: number }
  try {
    session = await api.upload.init(it.file.name, it.file.size, it.hash!)
  } catch (e) {
    if (e instanceof ApiError) {
      // 413 is a full quota and 507 an out-of-space server: both are about the
      // server, not this file, so every file behind it would fail the same way.
      if (e.status === 413) throw new Stop(e.message)
      if (e.status === 507) throw new Stop('The server is out of disk space')
      // 415 and 400 are about this file alone — a video format we cannot read,
      // a name that could not be made safe. Skip it, keep going.
      if (e.status === 415 || e.status === 400) {
        it.state = 'failed'
        it.error = e.message
        touch()
        return
      }
    }
    throw e
  }

  const { id, chunk_size } = session
  let offset = 0
  try {
    while (offset < it.file.size) {
      if (signal.aborted) throw new Stop('Cancelled')
      const end = Math.min(offset + chunk_size, it.file.size)
      offset = await putChunk(id, offset, it.file.slice(offset, end))
      it.sent = offset
      touch()
    }
    const done = await api.upload.finish(id)
    it.savedAs = done.filename
    it.state = 'done'
    touch()
  } catch (e) {
    // Release the server's scratch file and quota reservation rather than
    // leaving it for the six-hour sweeper — a cancelled batch of 200 photos
    // would otherwise hold its whole size against the user's ceiling.
    await api.upload.abort(id).catch(() => {})
    throw e
  }
}

/**
 * One chunk, with the two recoveries that actually happen on a home network.
 *
 * A 409 means the server's idea of where the file ends differs from ours —
 * usually a request that succeeded but whose reply never arrived. Re-reading
 * the offset and continuing from there is the whole point of tracking it
 * explicitly, so that case costs one extra round trip rather than the file.
 */
async function putChunk(id: string, offset: number, blob: Blob): Promise<number> {
  let attempt = 0
  let at = offset
  let slice = blob
  for (;;) {
    try {
      return await api.upload.chunk(id, at, slice)
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        const real = await api.upload.offset(id)
        if (real >= at + blob.size) return real // it landed after all
        // Re-cut the slice from wherever the server actually is.
        const shift = real - at
        if (shift < 0 || shift > blob.size) throw e
        slice = blob.slice(shift)
        at = real
        continue
      }
      if (e instanceof ApiError && e.status >= 400 && e.status < 500) throw e
      if (++attempt >= CHUNK_RETRIES) throw e
      await new Promise((r) => setTimeout(r, 400 * attempt))
    }
  }
}

function tally(items: UploadItem[], stoppedBecause?: string): UploadSummary {
  return {
    uploaded: items.filter((i) => i.state === 'done').length,
    skipped: items.filter((i) => i.state === 'skipped').length,
    failed: items.filter((i) => i.state === 'failed').length,
    stoppedBecause,
  }
}

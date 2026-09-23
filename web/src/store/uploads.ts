/*
 * The upload batch, lifted out of the screen that starts it.
 *
 * An upload used to live in `UploadScreen`'s own state, which made the screen
 * the upload: leaving it left a transfer running with nothing driving the UI,
 * and the back button had to double as Cancel to stop that being a silent leak.
 * Sending 50 photos over a home link takes minutes, and for those minutes the
 * app was a progress bar you had to sit and watch.
 *
 * Here instead, so a batch outlives the screen. You can go back to Clean, open
 * the Gallery, swipe — the transfer keeps going, and a pill at the top of the
 * app says so and takes you back to it. The tab still has to stay open: there
 * is no Background Fetch on iOS, and a service worker cannot be handed a
 * `File` the user picked in a page that has since gone away.
 */

import { create } from 'zustand'
import { makeItems, runUpload, type UploadItem, type UploadSummary } from '../api/uploader'

/** Above this many files, previews cost more memory than they are worth. */
const PREVIEW_LIMIT = 60

interface UploadState {
  items: UploadItem[]
  /** Object URLs for the picked files, by item key. Empty above the limit. */
  previews: Map<string, string>
  running: boolean
  summary: UploadSummary | null

  /** Replace the selection. Refused mid-batch — the run holds this array. */
  pick: (files: File[]) => void
  /**
   * Send what is picked.
   *
   * `toast` and `onUploaded` are captured here rather than read from a screen,
   * because by the time this resolves the screen that called it may well be
   * gone. Both come from App, which outlives everything.
   */
  start: (toast: (msg: string) => void, onUploaded: () => void) => Promise<void>
  cancel: () => void
  /** Drop the selection and let go of its previews. Refused mid-batch. */
  clear: () => void
}

/**
 * Live run state, deliberately outside the store.
 *
 * Neither belongs in React state — nothing renders them, and writing the
 * controller through `set` would re-render every subscriber for a value only
 * `cancel` ever reads.
 */
let controller: AbortController | null = null
let pendingFrame: number | null = null

function revoke(previews: Map<string, string>) {
  previews.forEach((url) => URL.revokeObjectURL(url))
}

function makePreviews(items: UploadItem[]): Map<string, string> {
  const m = new Map<string, string>()
  if (items.length === 0 || items.length > PREVIEW_LIMIT) return m
  for (const it of items) {
    if (it.file.type.startsWith('image/')) m.set(it.key, URL.createObjectURL(it.file))
  }
  return m
}

export const useUploads = create<UploadState>()((set, get) => ({
  items: [],
  previews: new Map(),
  running: false,
  summary: null,

  pick: (files) => {
    if (get().running) return
    revoke(get().previews)
    const items = makeItems(files)
    // Built once, here, and never rebuilt for the length of the batch. They
    // used to be derived from `items`, which the run replaces several times a
    // second to report progress — so every tick revoked all sixty URLs and
    // minted sixty more, and the browser re-decoded every thumbnail it had
    // just decoded. That is what made the list flicker and go blank under a
    // fast scroll, and it got worse the more photos you picked.
    set({ items, previews: makePreviews(items), summary: null })
  },

  start: async (toast, onUploaded) => {
    const { items, running } = get()
    if (running || items.length === 0) return
    const ac = new AbortController()
    controller = ac
    set({ running: true, summary: null })

    /*
     * Progress, at most once a frame.
     *
     * `runUpload` reports every hashed and every transferred chunk — hundreds
     * of times a second across two workers. Rendering a 60-row list that often
     * is what actually made the upload slow, and on a phone it starved the
     * transfer it was describing. A frame's worth of coalescing costs nothing
     * legible: nobody reads a percentage 300 times a second.
     */
    const push = (next: UploadItem[]) => {
      if (pendingFrame !== null) return
      pendingFrame = requestAnimationFrame(() => {
        pendingFrame = null
        set({ items: [...next] })
      })
    }

    try {
      const result = await runUpload(items, push, ac.signal)
      // The last frame may still be queued, and it carries a state older than
      // this one. Drop it rather than letting it land on top of the result.
      if (pendingFrame !== null) {
        cancelAnimationFrame(pendingFrame)
        pendingFrame = null
      }
      set({ items: [...items], summary: result })

      if (result.uploaded > 0) {
        onUploaded()
        toast(
          `${result.uploaded} photo${result.uploaded === 1 ? '' : 's'} added — analysing in the background`,
        )
      } else if (result.stoppedBecause) {
        toast(result.stoppedBecause)
      } else if (result.skipped > 0) {
        toast('Everything you picked was already on the server')
      }
    } finally {
      set({ running: false })
      controller = null
    }
  },

  cancel: () => controller?.abort(),

  clear: () => {
    if (get().running) return
    revoke(get().previews)
    set({ items: [], previews: new Map(), summary: null })
  },
}))

/** Bytes confirmed / bytes picked, for the screen and the pill both. */
export function uploadProgress(items: UploadItem[]) {
  let total = 0
  let sent = 0
  for (const it of items) {
    total += it.file.size
    sent += it.state === 'done' || it.state === 'skipped' ? it.file.size : it.sent
  }
  return { total, sent, pct: total > 0 ? Math.floor((sent / total) * 100) : 0 }
}

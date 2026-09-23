import { useState, useEffect, useCallback, useRef } from 'react'
import { IC } from '../icons'
import { api, type MediaFile, type Album } from '../api/client'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { ShareSheet } from '../components/ShareSheet'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

/** "Sep 13, 2026 · 6:05 PM" — what replaced the position counter up top. */
function fmtDateTime(secs: number) {
  if (!secs) return ''
  const d = new Date(secs * 1000)
  return (
    d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }) +
    ' · ' +
    d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
  )
}

/** Zoom ceiling. Past ~6x a 1600px preview is just big soft pixels. */
const MAX_SCALE = 6
/** Horizontal travel that commits to the next/previous photo. */
const NAV_THRESHOLD = 70
/** Downward travel that dismisses the viewer. */
const DISMISS_THRESHOLD = 120
/** Downward travel on a drawer that dismisses it. Shorter — it's a small sheet. */
const SHEET_DISMISS = 60
/** Upward travel on the photo that pulls the details drawer up. */
const SWIPE_UP_THRESHOLD = 70

/**
 * The media itself, mounted with `key={file.id}` so paging remounts it — load
 * and error state reset by construction rather than by an effect.
 *
 * A hidden photo takes the long way round. Everything else here is an `<img
 * src>` pointing at the server with the media token in the query string, and
 * the vault deliberately has no such URL: putting its token in a src would
 * write it into the address bar and the history of the one screen whose whole
 * purpose is leaving no trace. So the bytes are fetched with the token in a
 * header and handed to the tag as a blob URL, which is scoped to this document
 * and carries no credential. The thumbnail still comes over a URL — it is a
 * 200px square, and the grid behind this has already paid for it.
 */
function ViewerMedia({ file, hidden, onErr }: { file: MediaFile; hidden: boolean; onErr: () => void }) {
  const [loaded, setLoaded] = useState(false)
  const [err, setErr] = useState(false)
  /** The hidden original, once it has arrived. `null` until then, and always
   *  for an ordinary file, which needs no such trip. */
  const [blobUrl, setBlobUrl] = useState<string | null>(null)

  const fail = () => {
    setErr(true)
    onErr()
  }

  useEffect(() => {
    if (!hidden) return
    let live = true
    let url: string | null = null
    api.vault
      .rawBlob(file.id)
      .then((b) => {
        url = URL.createObjectURL(b)
        // Paged past already: revoke it here rather than leaving the cleanup
        // to find a URL it was never told about.
        if (!live) {
          URL.revokeObjectURL(url)
          url = null
          return
        }
        setBlobUrl(url)
      })
      .catch(() => {
        if (live) fail()
      })
    return () => {
      live = false
      if (url) URL.revokeObjectURL(url)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hidden, file.id])

  const thumb = hidden ? api.vault.thumbUrl(file.id) : api.thumbUrl(file.id)
  const full = hidden ? blobUrl : api.previewUrl(file.id)

  if (err) {
    return (
      <div className="viewer-fallback">
        <IC.imgOff />
        <div>{file.filename}</div>
      </div>
    )
  }

  if (file.media_type !== 'image') {
    // A hidden clip has no URL to stream from, so it waits for the whole file.
    if (hidden && !blobUrl) {
      return <img className="viewer-media in" src={thumb} alt={file.filename} aria-busy="true" />
    }
    return (
      <video
        className="viewer-media in"
        src={hidden ? blobUrl! : api.rawUrl(file.id)}
        // The first frame, extracted by ffmpeg at scan time, standing in until
        // the clip itself has decoded one. Without it the stage was an empty
        // grey rectangle with a play button on it — the video was fine, there
        // was simply nothing to look at until you pressed play, which is no
        // way to decide whether to keep it.
        poster={thumb}
        // Enough to draw the first frame and know the duration, without
        // pulling a 24 MB clip down because you paged past it.
        preload="metadata"
        controls
        playsInline
        onError={fail}
      />
    )
  }

  return (
    <>
      {/* The grid already cached this, so it paints instantly and the stage is
          never empty while the full preview renders server-side. */}
      <img className="viewer-media ph" src={thumb} alt="" aria-hidden="true" />
      {full && (
        <img
          className={`viewer-media full ${loaded ? 'in' : ''}`}
          src={full}
          alt={file.filename}
          draggable={false}
          onLoad={() => setLoaded(true)}
          onError={fail}
        />
      )}
    </>
  )
}

/**
 * Which library the deck is drawn from, and so which bar of actions is honest.
 *
 * `vault` is not a third kind of trash: a hidden file has no albums, no
 * favourite and no share — offering any of them would be a way of mentioning
 * the vault somewhere it should not be mentioned — and its delete is
 * permanent, because the trash needs no PIN and a hidden photo sitting in it
 * would no longer be hidden.
 */
export type ViewerMode = 'gallery' | 'trash' | 'vault'

interface Props {
  files: MediaFile[]
  index: number
  onIndex: (i: number) => void
  onClose: () => void
  /** Which action bar to show. Trash swaps in Put back and Delete forever. */
  mode?: ViewerMode
  /** Called after a file leaves this list, so the caller can drop it. */
  onRemoved?: (id: number) => void
  toast?: (msg: string) => void
}

/** Whether `mode` hides its files — the vault, and only the vault. */
function isHidden(mode: ViewerMode) {
  return mode === 'vault'
}

export function PhotoViewer({
  files,
  index,
  onIndex,
  onClose,
  mode = 'gallery',
  onRemoved,
  toast,
}: Props) {
  const file = files[index]
  const [panel, setPanel] = useState<'none' | 'info' | 'album'>('none')
  const [albums, setAlbums] = useState<Album[]>([])
  /** Albums this photo is already filed under — shown in Details. */
  const [fileAlbums, setFileAlbums] = useState<Album[] | null>(null)
  const [newAlbum, setNewAlbum] = useState('')
  const [busy, setBusy] = useState(false)
  /** Which destructive action is waiting on a yes. */
  const [confirm, setConfirm] = useState<'trash' | 'purge' | null>(null)
  const [sharing, setSharing] = useState(false)
  const [retentionDays, setRetentionDays] = useState(30)
  const [openedAt] = useState(() => Date.now())
  const hidden = isHidden(mode)

  const stageRef = useRef<HTMLDivElement>(null)
  const frameRef = useRef<HTMLDivElement>(null)

  /**
   * Live gesture state.
   *
   * Kept in a ref and written straight to `style.transform`, not to React
   * state: a pinch fires pointermove dozens of times a second, and re-rendering
   * the whole viewer on each one drops frames badly on a phone.
   */
  const view = useRef({ scale: 1, x: 0, y: 0 })
  const g = useRef({
    pointers: new Map<number, { x: number; y: number }>(),
    startDist: 0,
    startScale: 1,
    startX: 0,
    startY: 0,
    originX: 0,
    originY: 0,
    axis: '' as '' | 'x' | 'y',
    /** Raw vertical travel, sign intact. The photo only moves down, so
     *  `view.y` is clamped at 0 and cannot answer "was that a flick up?". */
    dy: 0,
  })

  const apply = useCallback((animate = false) => {
    const el = frameRef.current
    if (!el) return
    const { scale, x, y } = view.current
    el.style.transition = animate ? 'transform .22s cubic-bezier(.25,.46,.45,.94), opacity .22s' : 'none'
    el.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${scale})`
    // Fade the backdrop out as the photo is flung downwards.
    const stage = stageRef.current?.parentElement
    if (stage) {
      const fade = view.current.scale === 1 && y > 0 ? Math.max(1 - y / 400, 0.25) : 1
      stage.style.setProperty('--viewer-dim', String(fade))
    }
  }, [])

  const reset = useCallback((animate = false) => {
    view.current = { scale: 1, x: 0, y: 0 }
    apply(animate)
  }, [apply])

  const go = useCallback(
    (delta: number) => {
      const next = index + delta
      if (next < 0 || next >= files.length) {
        reset(true)
        return
      }
      reset(false)
      onIndex(next)
    },
    [index, files.length, onIndex, reset],
  )

  // Keyboard still works — the arrows are gone from the UI, not the interaction.
  // A confirmation on top owns the keyboard while it's up, or one Escape would
  // dismiss the dialog and the viewer behind it in the same press.
  useEffect(() => {
    if (confirm) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      else if (e.key === 'ArrowLeft') go(-1)
      else if (e.key === 'ArrowRight') go(1)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose, go, confirm])

  /**
   * Lock the page behind the viewer.
   *
   * The scrolling element is `.screen`, not `<body>` — the app is a flex column
   * with its own overflow — so setting `body { overflow: hidden }` did nothing
   * and the grid scrolled away underneath. A class on `<body>` lets one CSS
   * rule freeze whichever `.screen` is mounted, and hides the tab bar.
   */
  useEffect(() => {
    document.body.classList.add('viewer-open')
    return () => document.body.classList.remove('viewer-open')
  }, [])

  useEffect(() => {
    // Nothing in the vault can reach the trash, so its retention is not a
    // number this screen ever says out loud.
    if (hidden) return
    api.settings().then((s) => setRetentionDays(s.trash_retention_days)).catch(() => {})
  }, [hidden])

  // ── Gestures ────────────────────────────────────────────────────────────

  const clampPan = () => {
    const frame = frameRef.current
    if (!frame) return
    const { scale } = view.current
    if (scale <= 1) {
      view.current.x = 0
      view.current.y = 0
      return
    }
    // How far the scaled image may travel before its edge enters the viewport.
    // Measured from the frame's *layout* box rather than a bounding rect: the
    // frame is the element being scaled, so its rect is already multiplied by
    // the very scale this is trying to account for. It is also no longer the
    // same box as the stage — see `--chrome-skew-*`.
    const w = frame.offsetWidth
    const h = frame.offsetHeight
    const maxX = Math.max((w * scale - w) / 2, 0)
    const maxY = Math.max((h * scale - h) / 2, 0)
    view.current.x = Math.min(Math.max(view.current.x, -maxX), maxX)
    view.current.y = Math.min(Math.max(view.current.y, -maxY), maxY)
  }

  const onPointerDown = (e: React.PointerEvent) => {
    const s = g.current
    s.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    s.axis = ''
    s.dy = 0

    // Capture last, and never let it throw the gesture away. `setPointerCapture`
    // raises NotFoundError for a pointer the browser no longer considers active
    // — which, when it ran first, aborted this handler before the pinch state
    // below was set up, leaving a two-finger gesture behaving like a one-finger
    // drag.
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* pointer already gone; the gesture still works without capture */
    }

    if (s.pointers.size === 2) {
      const [a, b] = [...s.pointers.values()]
      s.startDist = Math.hypot(a.x - b.x, a.y - b.y)
      s.startScale = view.current.scale
      s.originX = view.current.x
      s.originY = view.current.y
      s.startX = (a.x + b.x) / 2
      s.startY = (a.y + b.y) / 2
    } else {
      s.startX = e.clientX
      s.startY = e.clientY
      s.originX = view.current.x
      s.originY = view.current.y
    }
  }

  const onPointerMove = (e: React.PointerEvent) => {
    const s = g.current
    if (!s.pointers.has(e.pointerId)) return
    s.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })

    if (s.pointers.size >= 2) {
      const [a, b] = [...s.pointers.values()]
      const dist = Math.hypot(a.x - b.x, a.y - b.y)
      if (s.startDist > 0) {
        view.current.scale = Math.min(Math.max((dist / s.startDist) * s.startScale, 1), MAX_SCALE)
      }
      const midX = (a.x + b.x) / 2
      const midY = (a.y + b.y) / 2
      view.current.x = s.originX + (midX - s.startX)
      view.current.y = s.originY + (midY - s.startY)
      clampPan()
      apply()
      return
    }

    const dx = e.clientX - s.startX
    const dy = e.clientY - s.startY

    if (view.current.scale > 1) {
      // Zoomed in: one finger pans the photo rather than changing it.
      view.current.x = s.originX + dx
      view.current.y = s.originY + dy
      clampPan()
      apply()
      return
    }

    // Not zoomed: the first decisive direction wins, so a sloppy horizontal
    // swipe can't half-dismiss the viewer on the way past.
    if (!s.axis && (Math.abs(dx) > 8 || Math.abs(dy) > 8)) {
      s.axis = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y'
    }
    if (s.axis === 'x') {
      const atEnd = (dx > 0 && index === 0) || (dx < 0 && index === files.length - 1)
      view.current.x = atEnd ? dx * 0.25 : dx // rubber-band at the ends
      view.current.y = 0
    } else if (s.axis === 'y') {
      s.dy = dy
      view.current.y = Math.max(dy, 0) // the photo itself only travels down
      view.current.x = 0
    }
    apply()
  }

  const onPointerUp = (e: React.PointerEvent) => {
    const s = g.current
    s.pointers.delete(e.pointerId)

    if (s.pointers.size >= 1) {
      // A finger lifted mid-pinch; re-seat the remaining one so the photo
      // doesn't jump.
      const [p] = [...s.pointers.values()]
      s.startX = p.x
      s.startY = p.y
      s.originX = view.current.x
      s.originY = view.current.y
      return
    }

    if (view.current.scale > 1) {
      clampPan()
      apply(true)
      return
    }

    const { x, y } = view.current

    // A tap on the photo with no drag behind it dismisses whichever drawer is
    // open — the third way out, alongside the toolbar button and pulling the
    // sheet down. `axis` stays empty until a gesture travels 8px, so this can
    // only fire on a real tap.
    if (s.axis === '' && panel !== 'none') {
      closePanel()
      reset(true)
      return
    }

    if (s.axis === 'x' && Math.abs(x) > NAV_THRESHOLD) {
      go(x < 0 ? 1 : -1)
      return
    }
    // Flick up on the photo to pull the details out — the counterpart to
    // pulling the drawer back down, so the panel has a gesture in both
    // directions and not just a button.
    if (s.axis === 'y' && s.dy < -SWIPE_UP_THRESHOLD && panel === 'none') {
      reset(true)
      openInfo()
      return
    }
    if (s.axis === 'y' && y > DISMISS_THRESHOLD) {
      // Carry the fling through instead of snapping back, then close.
      view.current.y = window.innerHeight
      apply(true)
      setTimeout(onClose, 200)
      return
    }
    reset(true)
  }

  const onDoubleClick = () => {
    view.current = view.current.scale > 1 ? { scale: 1, x: 0, y: 0 } : { scale: 2.5, x: 0, y: 0 }
    apply(true)
  }

  // ── Actions ─────────────────────────────────────────────────────────────

  /** Drawer drag: downward past a threshold dismisses, anything less springs back. */
  const sheetRef = useRef<HTMLDivElement>(null)

  const openAlbums = () => {
    reset(true)
    setPanel('album')
    // Only albums you made yourself. The automatic ones describe what a photo
    // *is* — its year, that it's a video, that you hearted it — so offering
    // them here would invite filing a 2021 photo into 2023, which the server
    // refuses and the next rebuild would undo regardless.
    api.albums()
      .then((all) => setAlbums(all.filter((a) => a.category_type === 'manual')))
      .catch(() => {})
  }

  const openInfo = () => {
    reset(true)
    setPanel('info')
    setFileAlbums(null)
    // A hidden file is out of every album by definition — asking would be a
    // request naming a vault id over the ordinary, PIN-less albums endpoint.
    if (hidden) {
      setFileAlbums([])
      return
    }
    api.fileAlbums(file.id).then(setFileAlbums).catch(() => setFileAlbums([]))
  }

  /**
   * Close whichever drawer is open.
   *
   * Reachable three ways — the photo, the toolbar button that opened it, and a
   * downward swipe on the drawer itself — so the sheet needs no close button of
   * its own, and a tap *inside* it (picking an album, reading a value) never
   * dismisses it by accident.
   */
  const closePanel = useCallback(() => {
    // Drive the exit from the element rather than unmounting straight away: a
    // drawer that vanished on the frame you released it read as a glitch, and
    // this also continues smoothly from wherever a drag left the sheet.
    const el = sheetRef.current
    if (!el) {
      setPanel('none')
      return
    }
    el.style.transition = 'transform 0.2s cubic-bezier(.25,.46,.45,.94)'
    el.style.transform = 'translateY(100%)'
    window.setTimeout(() => setPanel('none'), 190)
  }, [])

  const sheetDrag = useRef({ y: 0, dragging: false })

  const onSheetDown = (e: React.PointerEvent) => {
    // Never start a drag from a control — a slider or a list row needs its own
    // pointer events, and stealing them makes the drawer feel broken.
    if ((e.target as HTMLElement).closest('button, input, a, textarea')) return
    sheetDrag.current = { y: e.clientY, dragging: true }
  }

  const onSheetMove = (e: React.PointerEvent) => {
    const d = sheetDrag.current
    if (!d.dragging || !sheetRef.current) return
    const dy = Math.max(e.clientY - d.y, 0)
    sheetRef.current.style.transition = 'none'
    sheetRef.current.style.transform = `translateY(${dy}px)`
  }

  const onSheetUp = (e: React.PointerEvent) => {
    const d = sheetDrag.current
    if (!d.dragging || !sheetRef.current) return
    d.dragging = false
    const dy = e.clientY - d.y
    sheetRef.current.style.transition = ''
    sheetRef.current.style.transform = ''
    if (dy > SHEET_DISMISS) closePanel()
  }

  /** Drop the current file from the deck and land somewhere sensible. */
  const dropCurrent = (id: number) => {
    onRemoved?.(id)
    if (files.length <= 1) onClose()
    else if (index >= files.length - 1) onIndex(Math.max(index - 1, 0))
    reset(false)
  }

  const del = () => {
    setBusy(true)
    api.trashOne(file.id)
      .then(() => {
        toast?.(`${file.filename} moved to trash`)
        setConfirm(null)
        dropCurrent(file.id)
      })
      .catch(() => toast?.('Could not move it to the trash'))
      .finally(() => setBusy(false))
  }

  const putBack = () => {
    setBusy(true)
    api.trashRestore(file.id)
      .then(() => {
        toast?.(`${file.filename} put back — it's unreviewed again`)
        dropCurrent(file.id)
      })
      .catch(() => toast?.('Could not put it back'))
      .finally(() => setBusy(false))
  }

  const purge = () => {
    setBusy(true)
    api.trashPurge(file.id)
      .then(() => {
        toast?.(`${file.filename} deleted for good`)
        setConfirm(null)
        dropCurrent(file.id)
      })
      .catch(() => toast?.('Could not delete it'))
      .finally(() => setBusy(false))
  }

  /** Out of the vault and back into the library, unreviewed. */
  const unhide = () => {
    setBusy(true)
    api.vault
      .restore([file.id])
      .then(() => {
        toast?.(`${file.filename} is back in your library`)
        dropCurrent(file.id)
      })
      .catch(() => toast?.('Could not put it back'))
      .finally(() => setBusy(false))
  }

  /** Straight off the disk. The vault has no trash to fall back to. */
  const purgeHidden = () => {
    setBusy(true)
    api.vault
      .remove([file.id])
      .then(() => {
        toast?.(`${file.filename} deleted for good`)
        setConfirm(null)
        dropCurrent(file.id)
      })
      .catch(() => toast?.('Could not delete it'))
      .finally(() => setBusy(false))
  }

  const addToAlbum = (album_id?: number, name?: string) => {
    setBusy(true)
    api.albumAdd({ file_id: file.id, album_id, name })
      .then(() => {
        toast?.(`Added to ${name ?? albums.find((a) => a.id === album_id)?.name ?? 'album'}`)
        setPanel('none')
        setNewAlbum('')
        // Details reads this; it is stale the moment a photo is filed.
        setFileAlbums(null)
      })
      .catch(() => toast?.('Could not add it to that album'))
      .finally(() => setBusy(false))
  }

  if (!file) return null

  // `openedAt` rather than a live clock: reading the wall clock during render
  // is impure, and a days-left readout has no reason to tick while you look at
  // one photo.
  const daysLeft =
    file.trashed_at !== null
      ? Math.max(retentionDays - Math.floor((openedAt / 1000 - file.trashed_at) / 86400), 0)
      : null

  return (
    <div className="viewer" role="dialog" aria-modal="true" aria-label={file.filename}>
      <div className="viewer-top">
        <button className="viewer-btn" onClick={onClose} aria-label="Back">
          <IC.chevLeft />
        </button>
        <div className="viewer-when">{fmtDateTime(file.created_at)}</div>
        {/* Favourites are a review signal; they mean nothing for a binned file. */}
        {mode === 'gallery' ? (
          <FavButton key={file.id} file={file} />
        ) : (
          <span className="viewer-btn-spacer" aria-hidden="true" />
        )}
      </div>

      <div className="viewer-stage" ref={stageRef}>
        <div
          className="viewer-frame"
          ref={frameRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onDoubleClick={onDoubleClick}
        >
          <ViewerMedia key={file.id} file={file} hidden={hidden} onErr={() => reset(false)} />
        </div>
      </div>

      {panel === 'info' && (
        <div
          className="viewer-sheet"
          role="group"
          aria-label="Photo details"
          ref={sheetRef}
          onPointerDown={onSheetDown}
          onPointerMove={onSheetMove}
          onPointerUp={onSheetUp}
          onPointerCancel={onSheetUp}
        >
          {/* The grab handle is the close affordance now — pull the sheet down.
              A tap inside must never dismiss, so there is nothing to mis-hit. */}
          <div className="viewer-sheet-grip" aria-hidden="true" />
          <div className="viewer-sheet-head">
            <span>Details</span>
          </div>
          <div className="viewer-sheet-body">
            <dl className="viewer-info">
              <dt>Name</dt><dd>{file.filename}</dd>
              <dt>Taken</dt><dd>{fmtDateTime(file.created_at) || 'unknown'}</dd>
              <dt>Size</dt><dd>{fmtBytes(file.size_bytes)}</dd>
              <dt>Type</dt><dd>{file.media_type}</dd>
              <dt>Folder</dt><dd>{file.rel_path || '—'}</dd>
              {!hidden && (
                <>
                  <dt>Albums</dt>
                  <dd>
                    {fileAlbums === null ? (
                      'Checking…'
                    ) : fileAlbums.length === 0 ? (
                      'Not in any album'
                    ) : (
                      <span className="viewer-album-tags">
                        {fileAlbums.map((a) => (
                          <span key={a.id} className="viewer-album-tag">{a.name}</span>
                        ))}
                      </span>
                    )}
                  </dd>
                </>
              )}
              <dt>Status</dt>
              <dd>
                {hidden
                  ? 'Hidden'
                  : file.status === 'pending'
                    ? 'Not reviewed'
                    : file.status === 'keep'
                      ? 'Cleaned'
                      : 'In trash'}
                {!hidden && daysLeft !== null && ` · ${daysLeft} day${daysLeft === 1 ? '' : 's'} left`}
              </dd>
              {file.blur_score !== null && (
                <>
                  <dt>Sharpness</dt>
                  <dd>{file.blur_score.toFixed(0)}{file.is_blurry && ' · flagged blurry'}</dd>
                </>
              )}
              {file.group_id !== null && (<><dt>Duplicate</dt><dd>in a duplicate group</dd></>)}
            </dl>
          </div>
        </div>
      )}

      {panel === 'album' && (
        <div
          className="viewer-sheet"
          role="group"
          aria-label="Add to album"
          ref={sheetRef}
          onPointerDown={onSheetDown}
          onPointerMove={onSheetMove}
          onPointerUp={onSheetUp}
          onPointerCancel={onSheetUp}
        >
          <div className="viewer-sheet-grip" aria-hidden="true" />
          {/* Title and the new-album field stay put; only the list below them
              scrolls, so the way to create an album never scrolls out of reach
              behind a long list. */}
          <div className="viewer-sheet-head">
            <span>Add to album</span>
          </div>
          <div className="viewer-album-new">
            <input
              value={newAlbum}
              onChange={(e) => setNewAlbum(e.target.value)}
              placeholder="New album name…"
              onKeyDown={(e) => {
                if (e.key === 'Enter' && newAlbum.trim()) addToAlbum(undefined, newAlbum.trim())
              }}
            />
            <button
              className="pill-btn"
              disabled={!newAlbum.trim() || busy}
              onClick={() => addToAlbum(undefined, newAlbum.trim())}
            >
              Create
            </button>
          </div>
          <div className="viewer-album-list viewer-sheet-body">
            {albums.length === 0 ? (
              <div className="viewer-album-empty">
                No albums of your own yet — make one above. Years, Videos and
                Favourites fill themselves.
              </div>
            ) : (
              albums.map((a) => (
                <button key={a.id} className="viewer-album-row" disabled={busy} onClick={() => addToAlbum(a.id)}>
                  <span>{a.name}</span>
                  <span className="viewer-album-count">{a.file_count}</span>
                </button>
              ))
            )}
          </div>
        </div>
      )}

      {/* Reads as the app's tab bar, because it stands in for it while the
          viewer is up — same height, same blur, same four-across rhythm. */}
      <div className="viewer-bar">
        {mode === 'vault' ? (
          <>
            <button className="viewer-act" onClick={unhide} disabled={busy}>
              <IC.restore /><span>Put back</span>
            </button>
            <button
              className={`viewer-act ${panel === 'info' ? 'on' : ''}`}
              onClick={() => (panel === 'info' ? closePanel() : openInfo())}
            >
              <IC.info /><span>Info</span>
            </button>
            <button className="viewer-act danger" onClick={() => setConfirm('purge')} disabled={busy}>
              <IC.trash /><span>Delete forever</span>
            </button>
          </>
        ) : mode === 'gallery' ? (
          <>
            <button className="viewer-act" onClick={() => setSharing(true)} disabled={busy}>
              <IC.share /><span>Share</span>
            </button>
            <button
              className={`viewer-act ${panel === 'info' ? 'on' : ''}`}
              onClick={() => (panel === 'info' ? closePanel() : openInfo())}
            >
              <IC.info /><span>Info</span>
            </button>
            <button
              className={`viewer-act ${panel === 'album' ? 'on' : ''}`}
              onClick={() => (panel === 'album' ? closePanel() : openAlbums())}
              disabled={busy}
            >
              <IC.plus /><span>Album</span>
            </button>
            <button className="viewer-act danger" onClick={() => setConfirm('trash')} disabled={busy}>
              <IC.trash /><span>Delete</span>
            </button>
          </>
        ) : (
          <>
            <button className="viewer-act" onClick={putBack} disabled={busy}>
              <IC.restore /><span>Put back</span>
            </button>
            <button
              className={`viewer-act ${panel === 'info' ? 'on' : ''}`}
              onClick={() => (panel === 'info' ? closePanel() : openInfo())}
            >
              <IC.info /><span>Info</span>
            </button>
            <button className="viewer-act danger" onClick={() => setConfirm('purge')} disabled={busy}>
              <IC.trash /><span>Delete forever</span>
            </button>
          </>
        )}
      </div>

      {sharing && <ShareSheet files={[file]} onClose={() => setSharing(false)} toast={toast} />}

      {/* Binning is recoverable and says so; purging is not, and says that
          instead — the same dialog, two different promises. */}
      {confirm === 'trash' && (
        <ConfirmDialog
          title="Move to trash?"
          subject={file.filename}
          body={`It stays in the trash for ${retentionDays} days — you can put it back from Albums › Trash.`}
          confirmLabel="Move to trash"
          thumbUrl={api.thumbUrl(file.id)}
          busy={busy}
          onConfirm={del}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm === 'purge' && (
        <ConfirmDialog
          title="Delete forever?"
          subject={file.filename}
          body={
            hidden
              ? 'This erases it from disk. Hidden photos have no trash to fall back on — a trash needs no PIN, and one sitting in it would no longer be hidden.'
              : 'This erases it from disk. It cannot be undone.'
          }
          confirmLabel="Delete forever"
          thumbUrl={hidden ? api.vault.thumbUrl(file.id) : api.thumbUrl(file.id)}
          busy={busy}
          onConfirm={hidden ? purgeHidden : purge}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  )
}

/** Favourite toggle. Keyed per photo so its state resets on navigation. */
function FavButton({ file }: { file: MediaFile }) {
  const [fav, setFav] = useState(file.is_favorite)

  const toggle = () => {
    setFav((f) => !f) // optimistic; corrected from the response
    api.favorite(file.id)
      .then((r) => setFav(r.is_favorite))
      .catch(() => setFav(file.is_favorite))
  }

  return (
    <button
      className={`viewer-btn ${fav ? 'on' : ''}`}
      onClick={toggle}
      aria-pressed={fav}
      aria-label={fav ? 'Remove from favourites' : 'Add to favourites'}
    >
      <IC.heart />
    </button>
  )
}

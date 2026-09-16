import { useEffect, useRef, useState } from 'react'
import { useScrollLock } from '../hooks/useScrollLock'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'

/**
 * Can this browser hand files to the operating system's own share sheet?
 *
 * Only in a secure context, which for EasyClean means HTTPS or localhost — over
 * plain HTTP on the LAN `navigator.share` simply isn't there. Checked once at
 * module load because it cannot change while the page is open.
 */
const nav = navigator as Navigator & {
  share?: (d: unknown) => Promise<void>
  canShare?: (d: unknown) => boolean
}
const CAN_SYSTEM_SHARE = Boolean(
  window.isSecureContext && typeof nav.share === 'function' && typeof nav.canShare === 'function',
)

/** iOS is where "open the image and long-press it" is the real save path. */
const IS_IOS =
  /iPad|iPhone|iPod/.test(navigator.userAgent) ||
  (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)

interface Props {
  files: MediaFile[]
  onClose: () => void
  toast?: (msg: string) => void
}

/**
 * What to do with the photos you picked.
 *
 * Deliberately not a copy of the iOS sheet: this can only offer what a web page
 * on plain HTTP is actually allowed to do, and pretending otherwise is how the
 * old Share button ended up dumping people on a "Open in Preview" page with no
 * explanation. Every row here is something that works, and when the system
 * sheet is missing the sheet says why rather than hiding it.
 */
export function ShareSheet({ files, onClose, toast }: Props) {
  const [busy, setBusy] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  useScrollLock()
  const drag = useRef({ y: 0, dragging: false })
  const one = files.length === 1 ? files[0] : null

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  /** Slide out, then unmount — same exit the viewer's drawers use. */
  const dismiss = () => {
    const el = ref.current
    if (!el) return onClose()
    el.style.transition = 'transform 0.2s cubic-bezier(.25,.46,.45,.94)'
    el.style.transform = 'translateY(100%)'
    window.setTimeout(onClose, 190)
  }

  const onDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest('button')) return
    drag.current = { y: e.clientY, dragging: true }
  }
  const onMove = (e: React.PointerEvent) => {
    if (!drag.current.dragging || !ref.current) return
    const dy = Math.max(e.clientY - drag.current.y, 0)
    ref.current.style.transition = 'none'
    ref.current.style.transform = `translateY(${dy}px)`
  }
  const onUp = (e: React.PointerEvent) => {
    if (!drag.current.dragging || !ref.current) return
    drag.current.dragging = false
    const dy = e.clientY - drag.current.y
    ref.current.style.transition = ''
    ref.current.style.transform = ''
    if (dy > 60) dismiss()
  }

  /** Hand the actual files to the OS. Only reachable in a secure context. */
  const systemShare = async () => {
    setBusy(true)
    try {
      const blobs = await Promise.all(
        files.map(async (f) => {
          const b = await (await fetch(api.rawUrl(f.id))).blob()
          return new File([b], f.filename, { type: b.type })
        }),
      )
      if (nav.canShare?.({ files: blobs })) {
        await nav.share!({ files: blobs, title: one?.filename ?? `${files.length} photos` })
        dismiss()
        return
      }
      toast?.('This browser will not share those files')
    } catch {
      /* the user cancelling the OS sheet lands here too — say nothing */
    } finally {
      setBusy(false)
    }
  }

  /**
   * Open the full-size original in its own tab.
   *
   * On a phone this is the route to the camera roll: the image opens on its
   * own and a long press offers Add to Photos. The `download` attribute cannot
   * do that on iOS — it produces the bare "Open in Preview" page instead, which
   * is exactly the dead end this replaces.
   */
  const openOriginal = async () => {
    const f = one ?? files[0]
    setBusy(true)
    try {
      // Fetched and opened as a blob rather than navigating to the API URL.
      // Navigating put the token in the address bar and in history, where it
      // outlives the tab; a blob: URL belongs to this document and carries no
      // credential at all. The bytes are the same either way — this is the
      // same fetch `systemShare` already does.
      const blob = await (await fetch(api.rawUrl(f.id))).blob()
      const url = URL.createObjectURL(blob)
      const w = window.open(url, '_blank', 'noopener')
      if (!w) {
        URL.revokeObjectURL(url)
        toast?.('Your browser blocked the new tab')
        return
      }
      // Long enough for the new tab to have loaded it; the object would
      // otherwise be held for the life of this document.
      window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
      if (IS_IOS) toast?.('Long-press the photo → Add to Photos')
      dismiss()
    } catch {
      toast?.('Could not open that photo')
    } finally {
      setBusy(false)
    }
  }

  /** Straight download. Reliable everywhere except iOS, hence the ordering. */
  const download = () => {
    files.forEach((f, i) => {
      // Staggered: browsers drop a burst of simultaneous downloads.
      window.setTimeout(() => {
        const a = document.createElement('a')
        a.href = api.rawUrl(f.id)
        a.download = f.filename
        document.body.appendChild(a)
        a.click()
        a.remove()
      }, i * 300)
    })
    toast?.(files.length === 1 ? 'Downloading the original…' : `Downloading ${files.length} files…`)
    dismiss()
  }

  return (
    <div className="share-backdrop" role="presentation" onClick={(e) => { if (e.target === e.currentTarget) dismiss() }}>
      <div
        className="share-sheet"
        role="dialog"
        aria-modal="true"
        aria-label="Share"
        ref={ref}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
      >
        <div className="viewer-sheet-grip" aria-hidden="true" />

        <div className="share-head">
          <div className="share-thumbs">
            {files.slice(0, 3).map((f, i) => (
              <img key={f.id} src={api.thumbUrl(f.id)} alt="" style={{ zIndex: 3 - i }} />
            ))}
          </div>
          <div className="share-head-text">
            <div className="share-title">
              {one ? one.filename : `${files.length} photos`}
            </div>
            <div className="share-sub">
              {one ? fmtBytes(one.size_bytes) : 'Selected in the Gallery'}
            </div>
          </div>
        </div>

        <div className="share-actions">
          {CAN_SYSTEM_SHARE && (
            <button className="share-act" onClick={systemShare} disabled={busy}>
              <span className="share-act-icon"><IC.share /></span>
              <span>
                <b>{busy ? 'Preparing…' : 'Share…'}</b>
                <em>Messages, AirDrop, anything on this device</em>
              </span>
            </button>
          )}

          <button className="share-act" onClick={openOriginal} disabled={busy}>
            <span className="share-act-icon"><IC.eye /></span>
            <span>
              <b>{IS_IOS ? 'Save to Photos' : 'Open full size'}</b>
              <em>
                {IS_IOS
                  ? 'Opens the photo — long-press it and choose Add to Photos'
                  : 'Opens the original in a new tab'}
              </em>
            </span>
          </button>

          <button className="share-act" onClick={download} disabled={busy}>
            <span className="share-act-icon"><IC.folder /></span>
            <span>
              <b>{files.length === 1 ? 'Download original' : `Download ${files.length} files`}</b>
              <em>Saves to this device&rsquo;s downloads</em>
            </span>
          </button>
        </div>

        {/* The honest reason a native share row isn't here. Without it, its
            absence looks like a missing feature rather than a constraint. */}
        {!CAN_SYSTEM_SHARE && (
          <div className="share-note">
            Your device&rsquo;s own share sheet needs a secure (HTTPS) connection, and EasyClean is
            served over plain HTTP on your network. Everything above works as-is.
          </div>
        )}

        <button className="share-cancel" onClick={dismiss}>Cancel</button>
      </div>
    </div>
  )
}

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

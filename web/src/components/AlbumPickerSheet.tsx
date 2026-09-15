import { useEffect, useRef, useState } from 'react'
import { api, type Album } from '../api/client'

interface Props {
  /** Shown in the sheet's header. */
  title: string
  busy?: boolean
  /** Called with an existing album, or a name to create a new one. */
  onPick: (body: { album_id?: number; name?: string }) => void
  onClose: () => void
}

/**
 * Pick one of your own albums, or name a new one.
 *
 * Shared by the photo viewer and the Gallery's select mode so "add to album"
 * behaves identically whether you are filing one photo or forty — and so the
 * rule about which albums are eligible lives in one place.
 */
export function AlbumPickerSheet({ title, busy = false, onPick, onClose }: Props) {
  const [albums, setAlbums] = useState<Album[] | null>(null)
  const [name, setName] = useState('')
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef({ y: 0, dragging: false })

  useEffect(() => {
    // Only albums you made. The automatic ones describe what a photo *is* —
    // its year, that it's a video, that you hearted it — so filing into one by
    // hand would state something untrue, and the server refuses it anyway.
    api.albums()
      .then((all) => setAlbums(all.filter((a) => a.category_type === 'manual')))
      .catch(() => setAlbums([]))
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const dismiss = () => {
    const el = ref.current
    if (!el) return onClose()
    el.style.transition = 'transform 0.2s cubic-bezier(.25,.46,.45,.94)'
    el.style.transform = 'translateY(100%)'
    window.setTimeout(onClose, 190)
  }

  const onDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest('button, input')) return
    drag.current = { y: e.clientY, dragging: true }
  }
  const onMove = (e: React.PointerEvent) => {
    if (!drag.current.dragging || !ref.current) return
    ref.current.style.transition = 'none'
    ref.current.style.transform = `translateY(${Math.max(e.clientY - drag.current.y, 0)}px)`
  }
  const onUp = (e: React.PointerEvent) => {
    if (!drag.current.dragging || !ref.current) return
    drag.current.dragging = false
    const dy = e.clientY - drag.current.y
    ref.current.style.transition = ''
    ref.current.style.transform = ''
    if (dy > 60) dismiss()
  }

  return (
    <div
      className="share-backdrop"
      role="presentation"
      onClick={(e) => { if (e.target === e.currentTarget) dismiss() }}
    >
      <div
        className="viewer-sheet share-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        ref={ref}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
      >
        <div className="viewer-sheet-grip" aria-hidden="true" />
        {/* Header and the create field are pinned; only the list scrolls, so
            the way to make a new album can't slide away behind a long one. */}
        <div className="viewer-sheet-head"><span>{title}</span></div>
        <div className="viewer-album-new">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="New album name…"
            onKeyDown={(e) => {
              if (e.key === 'Enter' && name.trim()) onPick({ name: name.trim() })
            }}
          />
          <button
            className="pill-btn"
            disabled={!name.trim() || busy}
            onClick={() => onPick({ name: name.trim() })}
          >
            Create
          </button>
        </div>
        <div className="viewer-album-list viewer-sheet-body">
          {albums === null ? (
            <div className="viewer-album-empty">Loading…</div>
          ) : albums.length === 0 ? (
            <div className="viewer-album-empty">
              No albums of your own yet — make one above. Years, Videos and Favourites fill
              themselves.
            </div>
          ) : (
            albums.map((a) => (
              <button
                key={a.id}
                className="viewer-album-row"
                disabled={busy}
                onClick={() => onPick({ album_id: a.id })}
              >
                <span>{a.name}</span>
                <span className="viewer-album-count">{a.file_count}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  )
}

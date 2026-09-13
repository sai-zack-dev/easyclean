import { useState, useEffect, useCallback } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

function fmtDate(secs: number) {
  if (!secs) return ''
  return new Date(secs * 1000).toLocaleDateString('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

/**
 * The media itself. Split out and mounted with `key={file.id}` so paging to the
 * next photo remounts it — load and error state reset by construction, with no
 * effect reaching in to clear them.
 */
function ViewerMedia({ file }: { file: MediaFile }) {
  const [loaded, setLoaded] = useState(false)
  const [err, setErr] = useState(false)
  const stop = (e: React.MouseEvent) => e.stopPropagation()

  if (err) {
    return (
      <div className="viewer-fallback" onClick={stop}>
        <IC.imgOff />
        <div>{file.filename}</div>
      </div>
    )
  }

  if (file.media_type !== 'image') {
    return (
      <video
        className="viewer-media in"
        src={api.rawUrl(file.id)}
        controls
        playsInline
        onClick={stop}
        onError={() => setErr(true)}
      />
    )
  }

  return (
    <>
      {/* The thumbnail is already cached by the grid, so it paints immediately
          and the stage is never empty while the preview renders server-side. */}
      <img
        className="viewer-media ph"
        src={api.thumbUrl(file.id)}
        alt=""
        aria-hidden="true"
        onClick={stop}
      />
      <img
        className={`viewer-media full ${loaded ? 'in' : ''}`}
        src={api.previewUrl(file.id)}
        alt={file.filename}
        onLoad={() => setLoaded(true)}
        onError={() => setErr(true)}
        onClick={stop}
      />
    </>
  )
}

/** Favourite toggle. Keyed per photo for the same reason as the media above. */
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

interface Props {
  files: MediaFile[]
  index: number
  onIndex: (i: number) => void
  onClose: () => void
}

/**
 * Full-screen single-photo view, shared by the Gallery grid and album detail.
 *
 * Shows the server-rendered preview, not the thumbnail: this is the screen
 * where someone actually looks at a photo, so grid-sized pixels won't do.
 */
export function PhotoViewer({ files, index, onIndex, onClose }: Props) {
  const file = files[index]

  const prev = useCallback(() => {
    if (index > 0) onIndex(index - 1)
  }, [index, onIndex])

  const next = useCallback(() => {
    if (index < files.length - 1) onIndex(index + 1)
  }, [index, files.length, onIndex])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      else if (e.key === 'ArrowLeft') prev()
      else if (e.key === 'ArrowRight') next()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose, prev, next])

  // The grid behind stays mounted, so stop it scrolling under the overlay.
  useEffect(() => {
    const previous = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = previous
    }
  }, [])

  if (!file) return null

  return (
    <div className="viewer" role="dialog" aria-modal="true" aria-label={file.filename}>
      <div className="viewer-top">
        <button className="viewer-btn" onClick={onClose} aria-label="Close">
          <IC.chevLeft />
        </button>
        <div className="viewer-count">
          {index + 1} / {files.length}
        </div>
        <FavButton key={file.id} file={file} />
      </div>

      {/* Clicking the empty surround closes, the way a lightbox should; clicks
          on the photo itself must not, or paging through is a minefield. */}
      <div className="viewer-stage" onClick={onClose}>
        <ViewerMedia key={file.id} file={file} />

        {index > 0 && (
          <button
            className="viewer-nav prev"
            onClick={(e) => { e.stopPropagation(); prev() }}
            aria-label="Previous photo"
          >
            <IC.chevLeft />
          </button>
        )}
        {index < files.length - 1 && (
          <button
            className="viewer-nav next"
            onClick={(e) => { e.stopPropagation(); next() }}
            aria-label="Next photo"
          >
            <IC.chevRight />
          </button>
        )}
      </div>

      <div className="viewer-meta">
        <div className="viewer-name">{file.filename}</div>
        <div className="viewer-sub">
          {fmtDate(file.created_at)} · {fmtBytes(file.size_bytes)}
          {file.is_blurry && ' · blurry'}
          {file.group_id !== null && ' · duplicate'}
        </div>
      </div>
    </div>
  )
}

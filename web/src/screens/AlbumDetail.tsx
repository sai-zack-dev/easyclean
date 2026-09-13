import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type Album, type MediaFile } from '../api/client'
import { PhotoViewer } from './PhotoViewer'
import { Spinner } from '../components/Spinner'

interface Props {
  album: Album
  onClose: () => void
}

/** One page of album contents; more load as the user reaches the bottom. */
const PAGE_SIZE = 60

export function AlbumDetail({ album, onClose }: Props) {
  const [files, setFiles] = useState<MediaFile[]>([])
  const [page, setPage] = useState(1)
  const [loading, setLoading] = useState(true)
  const [done, setDone] = useState(false)
  const [viewing, setViewing] = useState<number | null>(null)

  // `loading` is raised by whoever asks for a page (initial state, or the Load
  // more button), never inside the effect — a synchronous setState in an effect
  // body just schedules a second render for something the caller already knew.
  useEffect(() => {
    let cancelled = false
    api.albumFiles(album.id, page, PAGE_SIZE)
      .then((d) => {
        if (cancelled) return
        // `page` is 1-based server-side; page 1 replaces, later pages append.
        setFiles((prev) => (page === 1 ? d.files : [...prev, ...d.files]))
        if (d.files.length < PAGE_SIZE) setDone(true)
      })
      .catch(() => { if (!cancelled) setDone(true) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [album.id, page])

  return (
    <div className="screen">
      <div className="page-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <button className="back-btn" onClick={onClose} aria-label="Back to albums">
            <IC.chevLeft />
          </button>
          <div>
            <div className="page-title">{album.name}</div>
            <div className="page-subtitle">
              {album.file_count} {album.file_count === 1 ? 'item' : 'items'}
            </div>
          </div>
        </div>
      </div>

      {files.length === 0 && !loading ? (
        <div className="al-empty" style={{ marginTop: 32 }}>
          This album is empty.
        </div>
      ) : (
        <div className="gallery-grid">
          {files.map((f, i) => (
            <button
              key={f.id}
              className="g-cell"
              onClick={() => setViewing(i)}
              aria-label={`Open ${f.filename}`}
            >
              <img
                src={api.thumbUrl(f.id)}
                alt=""
                loading="lazy"
                onError={(e) => {
                  (e.currentTarget.parentNode as HTMLElement).classList.add('g-cell-ph')
                }}
              />
              {f.media_type !== 'image' && <span className="g-cell-vid" aria-hidden="true" />}
            </button>
          ))}
        </div>
      )}

      {loading && (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}>
          <Spinner />
        </div>
      )}
      {!loading && !done && files.length > 0 && (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 16 }}>
          <button
            className="pill-btn"
            onClick={() => { setLoading(true); setPage((p) => p + 1) }}
          >
            Load more
          </button>
        </div>
      )}

      <div className="pb16" />

      {viewing !== null && (
        <PhotoViewer
          files={files}
          index={viewing}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
        />
      )}
    </div>
  )
}

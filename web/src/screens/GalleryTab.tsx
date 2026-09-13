import { useState, useEffect, useMemo } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'
import { PhotoViewer } from './PhotoViewer'

export function GalleryTab() {
  const [files, setFiles] = useState<MediaFile[]>([])
  const [query, setQuery] = useState('')
  const [searching, setSearching] = useState(false)
  const [viewing, setViewing] = useState<number | null>(null)

  useEffect(() => {
    // `library` mode, not the default review deck: the Gallery shows everything
    // you still have — kept photos included — and only hides the trash.
    api.mediaBatch({ mode: 'library', size: 500 })
      .then((d) => setFiles(d.files))
      .catch(() => {})
  }, [])

  // Filename match only — semantic search is a later phase (see CLAUDE.md).
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return files
    return files.filter((f) => f.filename.toLowerCase().includes(q))
  }, [files, query])

  // The viewer pages across the whole visible set, not just one date group, so
  // arrowing right at the end of a day continues into the next one.
  const groups = useMemo(() => {
    const out: { date: string; files: MediaFile[] }[] = []
    shown.forEach((f) => {
      const key = new Date(f.created_at * 1000).toLocaleDateString('en-US', {
        day: '2-digit', month: 'long', year: 'numeric',
      })
      const last = out[out.length - 1]
      if (last && last.date === key) last.files.push(f)
      else out.push({ date: key, files: [f] })
    })
    return out
  }, [shown])

  // Running index into `shown`, so a cell knows its position in the flat list.
  let flat = -1

  return (
    <div className="screen">
      <div className="page-header">
        <div>
          <div className="page-title">Gallery</div>
          <div className="page-subtitle">
            {shown.length} {shown.length === 1 ? 'item' : 'items'}
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          {searching && (
            <input
              className="gallery-search"
              autoFocus
              value={query}
              placeholder="Search filenames…"
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') { setQuery(''); setSearching(false) }
              }}
            />
          )}
          <button
            className="pill-btn"
            style={{ padding: '8px 10px' }}
            aria-label={searching ? 'Close search' : 'Search'}
            onClick={() => {
              if (searching) setQuery('')
              setSearching((s) => !s)
            }}
          >
            <IC.search />
          </button>
        </div>
      </div>

      {groups.length === 0 ? (
        <div style={{ color: 'var(--text3)', textAlign: 'center', padding: '64px 24px', fontSize: 13 }}>
          {query ? `Nothing matching “${query}”.` : 'No photos yet. Use Scan in the Clean tab.'}
        </div>
      ) : (
        groups.map((g) => (
          <div key={g.date}>
            <div className="gallery-date-hdr">{g.date}</div>
            <div className="gallery-grid">
              {g.files.map((f) => {
                flat += 1
                const at = flat
                return (
                  <button
                    key={f.id}
                    className="g-cell"
                    onClick={() => setViewing(at)}
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
                )
              })}
            </div>
          </div>
        ))
      )}
      <div className="pb16" />

      {viewing !== null && (
        <PhotoViewer
          files={shown}
          index={viewing}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
        />
      )}
    </div>
  )
}

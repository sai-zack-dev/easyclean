import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'

export function GalleryTab() {
  const [files, setFiles] = useState<MediaFile[]>([])

  useEffect(() => {
    api.mediaBatch({ size: 150 })
      .then((d) => setFiles(d.files))
      .catch(() => {})
  }, [])

  const groups: Record<string, MediaFile[]> = {}
  files.forEach((f) => {
    const key = new Date(f.created_at * 1000).toLocaleDateString('en-US', {
      day: '2-digit', month: 'long', year: 'numeric',
    })
    if (!groups[key]) groups[key] = []
    groups[key].push(f)
  })

  return (
    <div className="screen">
      <div className="page-header">
        <div>
          <div className="page-title">Gallery</div>
          <div className="page-subtitle">
            {new Date().toLocaleDateString('en-US', { day: '2-digit', month: 'long', year: 'numeric' })}
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button className="pill-btn" style={{ padding: '8px 10px' }}>
            <IC.search />
          </button>
          <button className="pill-btn">Select</button>
        </div>
      </div>

      {Object.keys(groups).length === 0 ? (
        <div style={{ color: 'var(--text3)', textAlign: 'center', padding: '64px 24px', fontSize: 13 }}>
          No photos yet. Use Scan in Clean tab.
        </div>
      ) : (
        Object.entries(groups).map(([date, gf]) => (
          <div key={date}>
            <div className="gallery-date-hdr">{date}</div>
            <div className="gallery-grid">
              {gf.slice(0, 12).map((f) => (
                <div key={f.id} className="g-cell">
                  <img
                    src={api.thumbUrl(f.id)}
                    alt=""
                    onError={(e) => {
                      (e.currentTarget.parentNode as HTMLElement).className = 'g-cell g-cell-ph'
                    }}
                  />
                </div>
              ))}
            </div>
          </div>
        ))
      )}
      <div className="pb16" />
    </div>
  )
}

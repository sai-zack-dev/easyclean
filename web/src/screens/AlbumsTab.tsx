import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type Album } from '../api/client'

function AlCard({ album, cls }: { album: Album; cls?: string }) {
  return (
    <div className={`al-card ${cls ?? ''}`}>
      {album.cover_file_id !== null && (
        <img className="al-cover" src={api.thumbUrl(album.cover_file_id)} alt="" loading="lazy" />
      )}
      <div className="al-label-bar">
        {album.name}
        <span className="al-count">{album.file_count}</span>
      </div>
    </div>
  )
}

function Section({ label, albums, cls }: { label: string; albums: Album[]; cls: string }) {
  return (
    <>
      <div style={{ padding: '0 16px', marginBottom: 8 }}>
        <div className="section-label">{label}</div>
      </div>
      {albums.length === 0 ? (
        <div className="al-empty">Nothing here yet</div>
      ) : (
        <div className={cls === 'al-lg' ? 'al-2col-wrap' : 'albums-scroll'} style={{ marginBottom: 20 }}>
          {cls === 'al-lg' ? (
            <div className="al-2col">
              {albums.map((a) => (
                <AlCard key={a.id} album={a} cls={cls} />
              ))}
            </div>
          ) : (
            albums.map((a) => <AlCard key={a.id} album={a} cls={cls} />)
          )}
        </div>
      )}
    </>
  )
}

export function AlbumsTab({ onOpenTrash }: { onOpenTrash: () => void }) {
  const [albums, setAlbums] = useState<Album[]>([])
  const [loaded, setLoaded] = useState(false)
  const [trashCount, setTrashCount] = useState(0)

  const load = () => {
    api.albums()
      .then(setAlbums)
      .catch(() => {})
      .finally(() => setLoaded(true))
    api.trash().then((f) => setTrashCount(f.length)).catch(() => {})
  }

  useEffect(() => { load() }, [])

  const refresh = () => {
    api.albumsRefresh().then(load).catch(() => {})
  }

  // The server's category_type vocabulary: month · year · videos ·
  // screenshots · favorites · place (see core/src/categorize.rs).
  const collections = albums.filter((a) => ['favorites', 'place'].includes(a.category_type))
  const suggested = albums.filter((a) => ['videos', 'screenshots'].includes(a.category_type))
  const memories = albums.filter((a) => ['month', 'year'].includes(a.category_type))

  return (
    <div className="screen">
      <div className="page-header">
        <div className="page-title">Albums</div>
        <button className="pill-btn" onClick={refresh} title="Rebuild albums">
          <IC.plus />
        </button>
      </div>

      {loaded && albums.length === 0 ? (
        <div className="al-empty" style={{ marginTop: 32 }}>
          No albums yet — keep some photos and finish a session to build them.
        </div>
      ) : (
        <>
          <Section label="Collections" albums={collections} cls="al-lg" />
          <Section label="Suggested Collections" albums={suggested} cls="al-sm" />
          <Section label="Memories" albums={memories} cls="al-sm" />
        </>
      )}

      <div style={{ padding: '0 16px', marginBottom: 20 }}>
        <button className="trash-entry" onClick={onOpenTrash}>
          <span className="trash-entry-icon"><IC.trash /></span>
          <span className="trash-entry-text">
            <span className="trash-entry-title">Trash</span>
            <span className="trash-entry-sub">
              {trashCount === 0
                ? 'Empty'
                : `${trashCount} file${trashCount === 1 ? '' : 's'} · restorable for 30 days`}
            </span>
          </span>
          <IC.chevRight />
        </button>
      </div>
      <div className="pb16" />
    </div>
  )
}

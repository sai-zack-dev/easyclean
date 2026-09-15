import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type Album } from '../api/client'
import { AlbumDetail } from './AlbumDetail'
import { useStickyHeader } from '../hooks/useStickyHeader'

function AlCard({ album, cls, onOpen }: { album: Album; cls?: string; onOpen: () => void }) {
  return (
    <button className={`al-card ${cls ?? ''}`} onClick={onOpen} aria-label={`Open ${album.name}`}>
      {album.cover_file_id !== null && (
        <img className="al-cover" src={api.thumbUrl(album.cover_file_id)} alt="" loading="lazy" />
      )}
      <div className="al-label-bar">
        {album.name}
        <span className="al-count">{album.file_count}</span>
      </div>
    </button>
  )
}

function Section({
  label,
  albums,
  cls,
  hint,
  icon,
  onOpen,
}: {
  label: string
  albums: Album[]
  cls: string
  /** What lands in this section, for when it's empty. */
  hint: string
  icon: keyof typeof IC
  onOpen: (a: Album) => void
}) {
  const Glyph = IC[icon]
  return (
    <>
      <div style={{ padding: '0 16px', marginBottom: 8 }}>
        <div className="section-label">{label}</div>
      </div>
      {albums.length === 0 ? (
        // "Nothing here yet" told you nothing you could act on, three times over.
        // These sections are built by the server, so the useful thing to say is
        // what puts something in them.
        <div className="al-hint">
          <span className="al-hint-icon"><Glyph /></span>
          <span>{hint}</span>
        </div>
      ) : (
        <div className={cls === 'al-lg' ? 'al-2col-wrap' : 'albums-scroll'} style={{ marginBottom: 20 }}>
          {cls === 'al-lg' ? (
            <div className="al-2col">
              {albums.map((a) => (
                <AlCard key={a.id} album={a} cls={cls} onOpen={() => onOpen(a)} />
              ))}
            </div>
          ) : (
            albums.map((a) => <AlCard key={a.id} album={a} cls={cls} onOpen={() => onOpen(a)} />)
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
  const [open, setOpen] = useState<Album | null>(null)
  const { ref: screenRef, onScroll } = useStickyHeader()

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

  // Counts and covers can change while the detail view is open (a swipe session
  // finishing behind it), so reload the list on the way back out.
  if (open) {
    return <AlbumDetail album={open} onClose={() => { setOpen(null); load() }} />
  }

  return (
    <div className="screen" ref={screenRef} onScroll={onScroll}>
      <div className="page-header">
        <div style={{ minWidth: 0 }}>
          <div className="page-title">Albums</div>
          {/* Same shape as Gallery's "1045 items" — a count of what's on this
              page, not a label for the first section below it. */}
          <div className="page-subtitle">
            {albums.length} collection{albums.length === 1 ? '' : 's'}
          </div>
        </div>
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
          {/* Each hint names the one action that fills that section. All three
              only appear after a session is finished — that's when the server
              files kept photos into albums. */}
          <Section
            label="Collections"
            albums={collections}
            cls="al-lg"
            icon="heart"
            hint="Tap the heart on a photo while reviewing, and your favourites collect here."
            onOpen={setOpen}
          />
          <Section
            label="Suggested Collections"
            albums={suggested}
            cls="al-sm"
            icon="album"
            hint="Videos and screenshots you keep are grouped here for you."
            onOpen={setOpen}
          />
          <Section
            label="Memories"
            albums={memories}
            cls="al-sm"
            icon="calendar"
            hint="Photos you keep are filed by month and year once you finish a session."
            onOpen={setOpen}
          />
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

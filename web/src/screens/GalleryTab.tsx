import { useState, useEffect, useMemo, useRef, useCallback } from 'react'
import { IC } from '../icons'
import { api, type MediaFile, type SearchResult, type SearchHit } from '../api/client'
import { PhotoViewer } from './PhotoViewer'
import { Spinner } from '../components/Spinner'
import { ShareSheet } from '../components/ShareSheet'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { AlbumPickerSheet } from '../components/AlbumPickerSheet'
import { SearchOverlay } from '../components/SearchOverlay'
import { useStickyHeader } from '../hooks/useStickyHeader'

/**
 * The three Gallery scopes, mapped to the server's batch modes.
 *
 * "Original" is the app's own word for un-reviewed — a photo still sitting in
 * `source/` exactly as the camera left it. "Cleaned" is one you kept, filed
 * into `clean/<album>/`. "All" is both, and never the trash.
 */
const TABS = [
  { id: 'library', label: 'All' },
  { id: 'original', label: 'Original' },
  { id: 'cleaned', label: 'Cleaned' },
] as const

type TabId = (typeof TABS)[number]['id']
type TypeFilter = '' | 'image' | 'video'

/** Rows per request. Enough to fill a screen, small enough to stay snappy. */
const PAGE_SIZE = 120
/** Wait for a pause in typing before asking the server again. */

/** A labelled grid of search hits. */
function HitGrid({
  files,
  onOpen,
}: {
  files: SearchHit[]
  onOpen: (i: number) => void
}) {
  return (
    <div className="gallery-grid">
      {files.map((f, i) => (
        <button key={f.id} className="g-cell" onClick={() => onOpen(i)} aria-label={`Open ${f.filename}`}>
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
  )
}

/**
 * Search output: exact matches, then photos that merely look like the query.
 *
 * They stay in separate groups rather than one blended ranking because they
 * answer different questions — a name match is a fact, a visual match is a
 * guess, and mixing them would hide which is which.
 */
function SearchResults({
  results,
  query,
  loading,
  onOpen,
}: {
  results: SearchResult | null
  query: string
  loading: boolean
  onOpen: (list: SearchHit[], index: number) => void
}) {
  if (loading && !results) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
        <Spinner />
      </div>
    )
  }
  if (!results) return null

  const { name_matches: named, semantic_matches: alike, semantic_available, semantic_confident } = results
  if (named.length === 0 && alike.length === 0) {
    return (
      <div style={{ color: 'var(--text3)', textAlign: 'center', padding: '56px 24px', fontSize: 13, lineHeight: 1.6 }}>
        Nothing matching “{query}”.
        {!semantic_available && (
          <>
            <br />
            Searching by what’s <em>in</em> a photo needs an index — build it in Setting.
          </>
        )}
      </div>
    )
  }

  return (
    <>
      {named.length > 0 && (
        <>
          <div className="gallery-date-hdr">
            Name matches<span className="hit-count">{named.length}</span>
          </div>
          <HitGrid files={named} onOpen={(i) => onOpen(named, i)} />
        </>
      )}
      {alike.length > 0 && (
        <>
          <div className="gallery-date-hdr">
            {semantic_confident ? `Looks like “${query}”` : 'Closest guesses'}
            <span className="hit-count">{alike.length}</span>
          </div>
          {!semantic_confident && (
            <div className="hit-note">Nothing clearly matches — these are just the nearest.</div>
          )}
          <HitGrid files={alike} onOpen={(i) => onOpen(alike, i)} />
        </>
      )}
      {!semantic_available && named.length > 0 && (
        <div className="gal-end">
          Only filenames were searched — build the photo index in Setting to search by content.
        </div>
      )}
    </>
  )
}

interface Props {
  toast: (msg: string) => void
}

export function GalleryTab({ toast }: Props) {
  const [tab, setTab] = useState<TabId>('library')
  const [type, setType] = useState<TypeFilter>('')
  const [files, setFiles] = useState<MediaFile[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [loading, setLoading] = useState(true)
  const [done, setDone] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [viewing, setViewing] = useState<number | null>(null)
  /** Ids picked in select mode. Null means select mode is off entirely. */
  const [picked, setPicked] = useState<Set<number> | null>(null)
  const [sharing, setSharing] = useState(false)
  const [confirmDel, setConfirmDel] = useState(false)
  const [albumPick, setAlbumPick] = useState(false)
  const [bulkBusy, setBulkBusy] = useState(false)

  /**
   * The query the server has been asked for.
   *
   * One string, not two: the old inline box updated on every keystroke and
   * needed a debounced `applied` alongside it. The overlay submits on Enter or
   * Search, so there is no half-typed state to hold.
   */
  const [applied, setApplied] = useState('')
  /** Merged name + semantic results; null while browsing. */
  const [results, setResults] = useState<SearchResult | null>(null)
  /** Which list the viewer pages through — a search group, or the browse grid. */
  const [resultList, setResultList] = useState<MediaFile[] | null>(null)

  const { ref: screenRef, onScroll } = useStickyHeader()
  const sentinel = useRef<HTMLDivElement>(null)

  /** How many rows the search actually returned, and whether more matched. */
  const resultCount =
    (results?.name_matches.length ?? 0) + (results?.semantic_matches.length ?? 0)
  const resultTruncated =
    results != null && (results.semantic_total ?? 0) > results.semantic_matches.length

  /** Start a search, or clear one by passing an empty string. */
  const runSearch = useCallback(
    (text: string) => {
      setApplied(text)
      setResults(null)
      setFiles([])
      setPage(1)
      setDone(false)
      setLoading(true)
      screenRef.current?.scrollTo({ top: 0 })
    },
    [screenRef],
  )

  // A response landing after the filters moved on would splice the wrong rows
  // in, so each request remembers what it asked for.
  const reqKey = useRef('')

  useEffect(() => {
    const key = `${tab}|${type}|${applied}`
    reqKey.current = key
    let cancelled = false

    // Searching and browsing are different shapes: browsing pages forever,
    // a search returns one merged, ranked set.
    if (applied) {
      api.search({ q: applied, mode: tab, type: type || undefined })
        .then((r) => {
          if (cancelled || reqKey.current !== key) return
          setResults(r)
          setDone(true)
        })
        .catch(() => { if (!cancelled) toast('Search failed') })
        .finally(() => { if (!cancelled) setLoading(false) })
      return () => { cancelled = true }
    }

    api.mediaBatch({ mode: tab, size: PAGE_SIZE, page, type: type || undefined })
      .then((d) => {
        if (cancelled || reqKey.current !== key) return
        setTotal(d.total)
        setFiles((prev) => (page === 1 ? d.files : [...prev, ...d.files]))
        if (d.files.length < PAGE_SIZE) setDone(true)
      })
      .catch(() => { if (!cancelled) toast('Could not load the gallery') })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [tab, type, applied, page, toast])

  // Pull the next page when the sentinel scrolls into view. This is what lets
  // the Gallery hold a 100K-photo library instead of the first 500.
  useEffect(() => {
    const el = sentinel.current
    if (!el || done || loading || applied) return
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          setLoading(true)
          setPage((p) => p + 1)
        }
      },
      { root: screenRef.current, rootMargin: '600px' },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [done, loading, files.length, applied, screenRef])

  /** Apply a filter change and start the list over from page 1. */
  const restart = (fn: () => void) => {
    fn()
    setResults(null)
    setFiles([])
    setPage(1)
    setDone(false)
    setLoading(true)
    screenRef.current?.scrollTo({ top: 0 })
  }

  const groups = useMemo(() => {
    const out: { date: string; files: MediaFile[] }[] = []
    files.forEach((f) => {
      const key = new Date(f.created_at * 1000).toLocaleDateString('en-US', {
        day: '2-digit', month: 'long', year: 'numeric',
      })
      const last = out[out.length - 1]
      if (last && last.date === key) last.files.push(f)
      else out.push({ date: key, files: [f] })
    })
    return out
  }, [files])

  const dropFile = (id: number) => {
    setFiles((fs) => fs.filter((f) => f.id !== id))
    setTotal((t) => Math.max(t - 1, 0))
  }

  // ── Select mode ─────────────────────────────────────────────────────────

  const selecting = picked !== null

  // The select bar stands *in place of* the tab bar, so the tab bar has to go:
  // both are translucent and bottom-anchored, and left up it showed through as
  // ghost text behind the counts.
  useEffect(() => {
    document.body.classList.toggle('select-mode', selecting)
    return () => document.body.classList.remove('select-mode')
  }, [selecting])
  const chosen = useMemo(
    () => (picked ? files.filter((f) => picked.has(f.id)) : []),
    [picked, files],
  )

  const togglePick = (id: number) => {
    setPicked((p) => {
      const next = new Set(p ?? [])
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  /** Leave select mode, dropping the selection with it. */
  const endSelect = () => {
    setPicked(null)
    setSharing(false)
    setAlbumPick(false)
    setConfirmDel(false)
  }

  /**
   * Bin everything picked.
   *
   * One request per file rather than a bulk endpoint: `/trash` already exists
   * and is the same operation the viewer uses, so this keeps one code path for
   * "a photo went to the trash" instead of two that could drift apart.
   */
  const trashPicked = () => {
    setBulkBusy(true)
    const ids = chosen.map((f) => f.id)
    Promise.allSettled(ids.map((id) => api.trashOne(id)))
      .then((rs) => {
        const ok = ids.filter((_, i) => rs[i].status === 'fulfilled')
        ok.forEach(dropFile)
        toast(
          ok.length === ids.length
            ? `${ok.length} moved to trash`
            : `${ok.length} of ${ids.length} moved to trash`,
        )
        endSelect()
      })
      .finally(() => setBulkBusy(false))
  }

  const addPickedToAlbum = (body: { album_id?: number; name?: string }) => {
    setBulkBusy(true)
    const ids = chosen.map((f) => f.id)
    Promise.allSettled(ids.map((id) => api.albumAdd({ file_id: id, ...body })))
      .then((rs) => {
        const ok = rs.filter((r) => r.status === 'fulfilled').length
        toast(ok === ids.length ? `${ok} added` : `${ok} of ${ids.length} added`)
        endSelect()
      })
      .finally(() => setBulkBusy(false))
  }

  let flat = -1

  const emptyMessage = applied
    ? `Nothing matching “${applied}”.`
    : type === 'video'
      ? 'No videos here.'
      : tab === 'cleaned'
        ? 'Nothing cleaned yet — keep some photos and finish a session.'
        : tab === 'original'
          ? 'Nothing left to review. All caught up.'
          : 'No photos yet. Use Scan in the Clean tab.'

  return (
    <div className="screen" ref={screenRef} onScroll={onScroll}>
      {/* Header and tabs pin as one block. Sticking them separately meant the
          tabs needed to know the header's height, and the guess they used was
          shorter than the real thing — so they slid up underneath it. */}
      <div className="page-sticky">
        <div className="page-header">
          <div style={{ minWidth: 0 }}>
            <div className="page-title">Gallery</div>
            {/* While a search is on, the count moves to the row below so it can
                sit opposite the query chip. */}
            {!applied && (
              <div className="page-subtitle">
                {total} item{total === 1 ? '' : 's'}
              </div>
            )}
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <button
              className={`pill-btn ${applied ? 'on' : ''}`}
              style={{ padding: '8px 10px' }}
              aria-label="Search"
              onClick={() => setSearchOpen(true)}
            >
              <IC.search />
            </button>
            {/* Select becomes Cancel while picking, so the way in and the way
                out are the same control rather than two places to look. */}
            <button
              className={`pill-btn ${selecting ? 'on' : ''}`}
              onClick={() => (selecting ? endSelect() : setPicked(new Set()))}
            >
              {selecting ? 'Cancel' : 'Select'}
            </button>
          </div>
        </div>

        {/* What you searched for, and how much it found. The chip is the only
            way out of a search, so it carries the clear button rather than
            hiding it behind reopening the overlay. */}
        {applied && (
          <div className="gal-query">
            <span className="gal-query-count">
              {resultCount} item{resultCount === 1 ? '' : 's'}
              {resultTruncated && <em> of {results?.semantic_total}</em>}
            </span>
            <button className="gal-chip" onClick={() => runSearch('')} title="Clear search">
              <IC.search />
              <span>{applied}</span>
              <IC.close />
            </button>
          </div>
        )}

        <div className="gal-tabs" role="tablist">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`gal-tab ${tab === t.id ? 'active' : ''}`}
              onClick={() => tab !== t.id && restart(() => setTab(t.id))}
            >
              {t.label}
            </button>
          ))}

          <span className="gal-tab-sep" aria-hidden="true" />

          {/* Media type is a separate axis from the review scope, so these toggle
              rather than joining the tab group: tapping the active one clears it. */}
          {([['image', 'Photos'], ['video', 'Videos']] as const).map(([id, label]) => (
            <button
              key={id}
              aria-pressed={type === id}
              className={`gal-tab ${type === id ? 'active' : ''}`}
              onClick={() => restart(() => setType((cur) => (cur === id ? '' : id)))}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {applied ? (
        <SearchResults
          results={results}
          query={applied}
          loading={loading}
          onOpen={(list, i) => { setResultList(list); setViewing(i) }}
        />
      ) : groups.length === 0 && !loading ? (
        <div style={{ color: 'var(--text3)', textAlign: 'center', padding: '64px 24px', fontSize: 13 }}>
          {emptyMessage}
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
                    className={`g-cell ${selecting ? 'g-pick' : ''} ${picked?.has(f.id) ? 'on' : ''}`}
                    onClick={() => {
                      if (selecting) togglePick(f.id)
                      else { setResultList(null); setViewing(at) }
                    }}
                    aria-pressed={selecting ? picked?.has(f.id) : undefined}
                    aria-label={selecting ? `Select ${f.filename}` : `Open ${f.filename}`}
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
                    {selecting && (
                      <span className="g-tick" aria-hidden="true">
                        {picked?.has(f.id) && <IC.check />}
                      </span>
                    )}
                  </button>
                )
              })}
            </div>
          </div>
        ))
      )}

      {/* Infinite-scroll trigger; also the "that's everything" marker. */}
      <div ref={sentinel} />
      {!applied && loading && (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}>
          <Spinner />
        </div>
      )}
      {!applied && done && files.length > 0 && (
        <div className="gal-end">
          {files.length === total ? `All ${total} shown` : `${files.length} of ${total} shown`}
        </div>
      )}

      <div className="pb16" />

      {viewing !== null && (
        <PhotoViewer
          files={resultList ?? files}
          index={viewing}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
          onRemoved={dropFile}
          toast={toast}
        />
      )}

      {/* Stands in for the tab bar while picking — same height and rhythm, so
          it reads as the app changing mode rather than a bar appearing over it.
          The count sits in the middle where the running totals do elsewhere. */}
      {selecting && (
        <div className="select-bar">
          <button
            className="select-act"
            disabled={chosen.length === 0 || bulkBusy}
            onClick={() => setSharing(true)}
          >
            <IC.share /><span>Share</span>
          </button>
          <div className="select-count">
            <b>{chosen.length}</b>
            <span>selected</span>
          </div>
          <button
            className="select-act"
            disabled={chosen.length === 0 || bulkBusy}
            onClick={() => setAlbumPick(true)}
          >
            <IC.plus /><span>Album</span>
          </button>
          <button
            className="select-act danger"
            disabled={chosen.length === 0 || bulkBusy}
            onClick={() => setConfirmDel(true)}
          >
            <IC.trash /><span>Delete</span>
          </button>
        </div>
      )}

      {sharing && chosen.length > 0 && (
        <ShareSheet files={chosen} onClose={() => setSharing(false)} toast={toast} />
      )}

      {searchOpen && (
        <SearchOverlay
          initial={applied}
          onSearch={(q) => {
            setSearchOpen(false)
            runSearch(q)
          }}
          onClose={() => setSearchOpen(false)}
        />
      )}

      {albumPick && (
        <AlbumPickerSheet
          title={`Add ${chosen.length} to album`}
          busy={bulkBusy}
          onPick={addPickedToAlbum}
          onClose={() => setAlbumPick(false)}
        />
      )}

      {confirmDel && (
        <ConfirmDialog
          title={chosen.length === 1 ? 'Move to trash?' : `Move ${chosen.length} to trash?`}
          subject={chosen.length === 1 ? chosen[0].filename : undefined}
          body="They stay in the trash for 30 days — you can put them back from Albums › Trash."
          confirmLabel="Move to trash"
          busy={bulkBusy}
          onConfirm={trashPicked}
          onCancel={() => setConfirmDel(false)}
        />
      )}
    </div>
  )
}

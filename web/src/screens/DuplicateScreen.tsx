import { useState, useEffect, useMemo } from 'react'
import { IC } from '../icons'
import { api, type MediaFile, type DuplicateGroup } from '../api/client'
import { Spinner } from '../components/Spinner'

interface Props {
  onClose: () => void
  toast: (msg: string) => void
}

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
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

/** How many decided groups can be taken back, matching the swipe deck's depth. */
const UNDO_DEPTH = 10

interface HistoryEntry {
  /** Where the group sat, so undo puts it back in place rather than at the end. */
  at: number
  group: DuplicateGroup
  kept: number[]
  deleted: number[]
}

/**
 * Which copy in a group is the one worth keeping.
 *
 * Largest file first — for the same picture, more bytes means less
 * re-compression, which is the difference between the original and the copy
 * that came back through a chat app. Sharper wins a tie, then the older one,
 * since the first save is usually the original. It is only a suggestion; the
 * user can select anything.
 */
function suggestBest(files: MediaFile[]): number | null {
  if (files.length === 0) return null
  const best = [...files].sort((a, b) => {
    if (b.size_bytes !== a.size_bytes) return b.size_bytes - a.size_bytes
    const as = a.blur_score ?? 0
    const bs = b.blur_score ?? 0
    if (bs !== as) return bs - as
    return a.created_at - b.created_at
  })[0]
  return best.id
}

/**
 * Side-by-side duplicate review.
 *
 * A one-card-at-a-time deck is the wrong shape for duplicates: it asks "keep
 * this?" about a photo whose only problem is that another copy exists, and
 * never shows you the other copy. Here a whole cluster is on screen at once,
 * you tap the copies you mean, and one button settles the group — selected go
 * one way, the rest go the other. That covers every case in a single gesture:
 * select both and Keep, select both and Delete, or select one of each.
 */
export function DuplicateScreen({ onClose, toast }: Props) {
  const [groups, setGroups] = useState<DuplicateGroup[]>([])
  const [idx, setIdx] = useState(0)
  /**
   * The user's own choice for one group, if they've made one.
   *
   * Keyed by group id rather than reset on navigation: the selection is
   * *derived* from the current group below, so moving between groups needs no
   * effect to keep it in sync — and a group you come back to via Undo still
   * remembers what you had picked.
   */
  const [picked, setPicked] = useState<{ gid: string; ids: Set<number> } | null>(null)
  const [hist, setHist] = useState<HistoryEntry[]>([])
  const [kept, setKept] = useState(0)
  const [removed, setRemoved] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [finishing, setFinishing] = useState(false)
  /** Tapped-open copy, shown full-bleed so the two can be compared properly. */
  const [zoom, setZoom] = useState<MediaFile | null>(null)

  useEffect(() => {
    let cancelled = false
    api.duplicateGroups()
      .then((d) => {
        if (cancelled) return
        setGroups(d.groups)
        setLoading(false)
      })
      .catch(() => {
        if (!cancelled) {
          toast('Could not load duplicates')
          setLoading(false)
        }
      })
    return () => { cancelled = true }
  }, [toast])

  const group = groups[idx]
  const suggested = useMemo(() => (group ? suggestBest(group.files) : null), [group])

  // Untouched, a group arrives with the best copy already selected — so the
  // common case, "keep the good one and bin the rest", is a single tap.
  const sel = useMemo(() => {
    if (!group) return new Set<number>()
    if (picked && picked.gid === group.group_id) return picked.ids
    return new Set<number>(suggested === null ? [] : [suggested])
  }, [group, picked, suggested])

  const toggle = (id: number) => {
    if (!group) return
    const next = new Set(sel)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    setPicked({ gid: group.group_id, ids: next })
  }

  /**
   * Settle the group. `action` says what the *selection* means; everything
   * unselected gets the opposite, because a copy left pending would bring the
   * same group straight back on the next pass.
   */
  const resolve = (action: 'keep' | 'delete') => {
    if (!group || busy) return
    const chosen = group.files.filter((f) => sel.has(f.id)).map((f) => f.id)
    const rest = group.files.filter((f) => !sel.has(f.id)).map((f) => f.id)
    const keep = action === 'keep' ? chosen : rest
    const del = action === 'keep' ? rest : chosen

    setBusy(true)
    api.resolveDuplicates({ keep, delete: del })
      .then(() => {
        const rest2 = groups.filter((_, i) => i !== idx)
        setHist((h) => [...h, { at: idx, group, kept: keep, deleted: del }].slice(-UNDO_DEPTH))
        setKept((k) => k + keep.length)
        setRemoved((r) => r + del.length)
        setGroups(rest2)
        // Removing a group slides the next one into this slot, so the cursor
        // stays put — except at the end of the list, where it has to step back.
        setIdx(Math.min(idx, Math.max(rest2.length - 1, 0)))
      })
      .catch(() => toast('Could not settle that group'))
      .finally(() => setBusy(false))
  }

  /** Leave a group undecided and come back to it later. */
  const skip = () => {
    if (!group || groups.length < 2) return
    setGroups([...groups.filter((_, i) => i !== idx), group])
    // Sending the last group to the back would land it right back under the
    // cursor, so that case wraps to the front instead.
    setIdx(idx >= groups.length - 1 ? 0 : idx)
  }

  const undo = () => {
    const last = hist[hist.length - 1]
    if (!last || busy) return
    setBusy(true)
    api.resolveDuplicates({ undo: [...last.kept, ...last.deleted] })
      .then(() => {
        setHist((h) => h.slice(0, -1))
        setKept((k) => Math.max(0, k - last.kept.length))
        setRemoved((r) => Math.max(0, r - last.deleted.length))
        setGroups((gs) => {
          const next = [...gs]
          next.splice(Math.min(last.at, next.length), 0, last.group)
          return next
        })
        setIdx(Math.min(last.at, groups.length))
      })
      .catch(() => toast('Undo failed — check the server'))
      .finally(() => setBusy(false))
  }

  const finish = async () => {
    setFinishing(true)
    try {
      const r = await api.sessionFinish()
      toast(`${r.moved} moved to clean · ${r.trashed} to trash`)
      onClose()
    } catch {
      toast('Could not finish the session')
    } finally {
      setFinishing(false)
    }
  }

  const reviewed = hist.length
  const prog = reviewed + groups.length > 0 ? `${reviewed}/${reviewed + groups.length} groups` : ''
  const selCount = sel.size
  const restCount = group ? group.files.length - selCount : 0

  return (
    <div className="swipe-screen">
      <div className="swipe-header">
        <button className="back-btn" onClick={onClose} aria-label="Back">
          <IC.chevLeft />
        </button>
        <div className="swipe-title-wrap">
          <div className="swipe-title">Duplicates</div>
          <div className="swipe-sub">{prog}</div>
        </div>
        <button className="undo-btn" onClick={undo} disabled={!hist.length || busy}>
          UNDO <IC.undo />
        </button>
      </div>

      {loading ? (
        <div className="dup-arena">
          <Spinner />
        </div>
      ) : !group ? (
        <div className="dup-arena">
          <div className="done-screen">
            <svg
              viewBox="0 0 24 24"
              className="done-tick"
              style={{ width: 56, height: 56, fill: 'none', strokeWidth: 1.2, strokeLinecap: 'round', strokeLinejoin: 'round' }}
            >
              <polyline points="20 6 9 17 4 12" />
            </svg>
            <div className="done-title">{reviewed === 0 ? 'No duplicates' : 'All done!'}</div>
            <div className="done-sub">
              {reviewed === 0
                ? 'Nothing looks like a copy of anything else.'
                : `${kept} kept · ${removed} deleted`}
            </div>
            {kept + removed > 0 && (
              <>
                <button className="done-btn" onClick={finish} disabled={finishing}>
                  {finishing ? <Spinner /> : <IC.check />}
                  {finishing ? 'Finishing…' : 'Finish session'}
                </button>
                <div className="done-hint">
                  Moves kept photos into albums and deleted ones to the trash.
                </div>
              </>
            )}
          </div>
        </div>
      ) : (
        <>
          <div className="dup-hint">
            {group.files.length} copies look the same — tap the ones you mean, then choose.
          </div>

          <div className={`dup-arena dup-n-${Math.min(group.files.length, 4)}`}>
            {group.files.map((f) => {
              const on = sel.has(f.id)
              return (
                <div key={f.id} className={`dup-card ${on ? 'on' : ''}`}>
                  <button
                    className="dup-card-media"
                    onClick={() => toggle(f.id)}
                    aria-pressed={on}
                    aria-label={`${on ? 'Deselect' : 'Select'} ${f.filename}`}
                  >
                    <img src={api.previewUrl(f.id)} alt="" draggable={false} />
                    <span className="dup-tick" aria-hidden="true">
                      {on && <IC.check />}
                    </span>
                    {f.id === suggested && <span className="dup-best">BEST</span>}
                  </button>
                  <div className="dup-meta">
                    <div className="dup-name">{f.filename}</div>
                    <div className="dup-sub">
                      {fmtDate(f.created_at)} · {fmtBytes(f.size_bytes)}
                    </div>
                  </div>
                  <button
                    className="dup-zoom"
                    onClick={() => setZoom(f)}
                    aria-label={`View ${f.filename} full size`}
                  >
                    <IC.eye />
                  </button>
                </div>
              )
            })}
          </div>

          <div className="dup-actions">
            <button
              className="dup-btn del"
              onClick={() => resolve('delete')}
              disabled={busy || selCount === 0}
            >
              <IC.trash />
              <span>
                DELETE {selCount}
                <em>keep the other {restCount}</em>
              </span>
            </button>
            <button
              className="dup-skip"
              onClick={skip}
              disabled={busy || groups.length < 2}
              aria-label="Decide later"
            >
              SKIP
            </button>
            <button
              className="dup-btn kp"
              onClick={() => resolve('keep')}
              disabled={busy || selCount === 0}
            >
              <IC.check />
              <span>
                KEEP {selCount}
                <em>bin the other {restCount}</em>
              </span>
            </button>
          </div>
        </>
      )}

      {zoom && (
        <div className="dup-zoom-view" role="dialog" aria-modal="true" onClick={() => setZoom(null)}>
          <img src={api.previewUrl(zoom.id)} alt={zoom.filename} />
          <div className="dup-zoom-meta">
            {zoom.filename} · {fmtBytes(zoom.size_bytes)}
            {zoom.blur_score !== null && ` · sharpness ${zoom.blur_score.toFixed(0)}`}
          </div>
        </div>
      )}
    </div>
  )
}

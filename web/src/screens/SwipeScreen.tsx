import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'
import { SwipeCard } from './SwipeCard'
import { Spinner } from '../components/Spinner'

interface SwipeHistoryEntry {
  file: MediaFile
  action: 'keep' | 'delete'
  idx: number
}

interface Props {
  mode: string
  month?: string
  label: string
  onClose: () => void
  toast: (msg: string) => void
}

/** Matches the "last ~10 swipes" undo depth in the product spec. */
const UNDO_DEPTH = 10

export function SwipeScreen({ mode, month, label, onClose, toast }: Props) {
  const [files, setFiles] = useState<MediaFile[]>([])
  const [idx, setIdx] = useState(0)
  const [dels, setDels] = useState(0)
  const [kps, setKps] = useState(0)
  const [hist, setHist] = useState<SwipeHistoryEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [finishing, setFinishing] = useState(false)

  // `cancelled` guards against a slow first batch landing after the user has
  // already switched mode — the stale response would overwrite the new deck.
  useEffect(() => {
    let cancelled = false
    api.mediaBatch({ mode, month, size: 200 })
      .then((d) => {
        if (cancelled) return
        setFiles(d.files)
        setIdx(0)
        setLoading(false)
      })
      .catch(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [mode, month])

  const cur = files[idx]
  const total = files.length
  const prog = total > 0 ? `${idx}/${total} · ${((idx / total) * 100).toFixed(0)}%` : ''

  const act = (action: 'keep' | 'delete') => {
    if (!cur) return
    api.swipe(cur.id, action).catch(() => toast('Swipe failed — check the server'))
    setHist((h) => [...h, { file: cur, action, idx }].slice(-UNDO_DEPTH))
    if (action === 'delete') setDels((d) => d + 1)
    else setKps((k) => k + 1)
    setIdx((i) => i + 1)
  }

  const undo = () => {
    if (!hist.length) return
    const last = hist[hist.length - 1]
    // Revert on the server too, or the file stays keep/trashed and still
    // moves at session finish.
    api.unswipe(last.file.id).catch(() => toast('Undo failed — check the server'))
    setHist((h) => h.slice(0, -1))
    if (last.action === 'delete') setDels((d) => Math.max(0, d - 1))
    else setKps((k) => Math.max(0, k - 1))
    setIdx(last.idx)
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

  const badge = (f: MediaFile): 'blur' | 'dup' | null => {
    if (mode === 'blurs' || f.is_blurry) return 'blur'
    if (mode === 'duplicates' || f.group_id !== null) return 'dup'
    return null
  }

  const numFmt = (n: number) => String(n).padStart(3, '0')

  return (
    <div className="swipe-screen">
      <div className="swipe-header">
        <button className="back-btn" onClick={onClose}>
          <IC.chevLeft />
        </button>
        <div className="swipe-title-wrap">
          <div className="swipe-title">{label}</div>
          <div className="swipe-sub">{prog}</div>
        </div>
        <button className="undo-btn" onClick={undo} disabled={!hist.length}>
          UNDO <IC.undo />
        </button>
      </div>

      <div className="swipe-arena">
        {loading ? (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%' }}>
            <Spinner />
          </div>
        ) : cur ? (
          <SwipeCard
            key={cur.id}
            file={cur}
            badge={badge(cur)}
            onKeep={() => act('keep')}
            onDelete={() => act('delete')}
          />
        ) : (
          <div className="done-screen">
            <svg
              viewBox="0 0 24 24"
              className="done-tick"
              style={{ width: 56, height: 56, fill: 'none', strokeWidth: 1.2, strokeLinecap: 'round', strokeLinejoin: 'round' }}
            >
              <polyline points="20 6 9 17 4 12" />
            </svg>
            <div className="done-title">{total === 0 ? 'Nothing here' : 'All done!'}</div>
            <div className="done-sub">{kps} kept · {dels} deleted</div>
            {kps + dels > 0 && (
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
        )}
      </div>

      <div className="swipe-actions">
        <button className="action-btn del-btn" onClick={() => act('delete')}>
          <IC.chevLeft /> DELETE
        </button>
        <span style={{ color: 'var(--delete)', fontSize: 13, fontWeight: 700, alignSelf: 'center' }}>
          {numFmt(dels)}
        </span>
        <span style={{ color: 'var(--keep)', fontSize: 13, fontWeight: 700, alignSelf: 'center' }}>
          {numFmt(kps)}
        </span>
        <button className="action-btn kp-btn" onClick={() => act('keep')}>
          KEEP <IC.chevRight />
        </button>
      </div>
    </div>
  )
}

import { useCallback, useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'
import { Spinner } from '../components/Spinner'
import { PhotoViewer } from './PhotoViewer'

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

/**
 * Days before retention purges this file for good.
 *
 * `trashed_at` is when it was binned; anything without one (deleted but never
 * finished, so still sitting in `source/`) hasn't started its clock.
 */
function daysLeft(f: MediaFile, retention: number, now: number): number | null {
  if (f.trashed_at === null) return null
  const elapsed = Math.floor(now / 1000 - f.trashed_at) / 86400
  return Math.max(retention - Math.floor(elapsed), 0)
}

export function TrashScreen({ onClose, toast }: Props) {
  const [files, setFiles] = useState<MediaFile[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<number | null>(null)
  const [confirmAll, setConfirmAll] = useState(false)
  const [retention, setRetention] = useState(30)
  const [viewing, setViewing] = useState<number | null>(null)
  // Pinned at mount: reading the clock during render is impure, and the
  // countdown has no reason to tick while the screen is open.
  const [now] = useState(() => Date.now())

  const load = useCallback(() => {
    // The server orders by when each file was binned, so the newest deletion is
    // already first — which is the order you want when you've just deleted
    // something by mistake.
    api.trash()
      .then(setFiles)
      .catch(() => toast('Could not load the trash'))
      .finally(() => setLoading(false))
  }, [toast])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    api.settings().then((s) => setRetention(s.trash_retention_days)).catch(() => {})
  }, [])

  const restore = (f: MediaFile) => {
    setBusy(f.id)
    api.trashRestore(f.id)
      .then(() => {
        setFiles((fs) => fs.filter((x) => x.id !== f.id))
        toast(`${f.filename} put back — it's unreviewed again`)
      })
      .catch(() => toast('Could not put it back'))
      .finally(() => setBusy(null))
  }

  const purge = (f: MediaFile) => {
    setBusy(f.id)
    api.trashPurge(f.id)
      .then(() => {
        setFiles((fs) => fs.filter((x) => x.id !== f.id))
        toast(`${f.filename} deleted for good`)
      })
      .catch(() => toast('Delete failed'))
      .finally(() => setBusy(null))
  }

  const purgeAll = () => {
    const n = files.length
    api.trashPurgeAll()
      .then(() => {
        setFiles([])
        setConfirmAll(false)
        toast(`${n} file${n === 1 ? '' : 's'} deleted for good`)
      })
      .catch(() => toast('Could not empty the trash'))
  }

  const totalSize = files.reduce((sum, f) => sum + f.size_bytes, 0)

  return (
    <div className="swipe-screen">
      <div className="swipe-header">
        <button className="back-btn" onClick={onClose} aria-label="Back">
          <IC.chevLeft />
        </button>
        <div className="swipe-title-wrap">
          <div className="swipe-title">Trash</div>
          <div className="swipe-sub">
            {loading ? '' : `${files.length} file${files.length === 1 ? '' : 's'} · ${fmtBytes(totalSize)}`}
          </div>
        </div>
        {files.length > 0 ? (
          <button
            className={`undo-btn ${confirmAll ? 'danger' : ''}`}
            onClick={() => (confirmAll ? purgeAll() : setConfirmAll(true))}
          >
            {confirmAll ? 'SURE?' : 'EMPTY'}
          </button>
        ) : (
          <span className="header-spacer" aria-hidden="true" />
        )}
      </div>

      <div className="screen">
        {loading ? (
          <div className="trash-empty"><Spinner /></div>
        ) : files.length === 0 ? (
          <div className="trash-empty">
            <IC.trash />
            <div className="trash-empty-title">Trash is empty</div>
            <div className="trash-empty-sub">
              Deleted photos land here and stay for {retention} days.
            </div>
          </div>
        ) : (
          <>
            <div className="trash-note">
              Newest deletion first. Put back returns a photo to Original, unreviewed.
            </div>
            <div className="trash-list">
              {files.map((f, i) => {
                const left = daysLeft(f, retention, now)
                return (
                  <div className="trash-row" key={f.id}>
                    <button
                      className="trash-thumb-btn"
                      onClick={() => setViewing(i)}
                      aria-label={`Open ${f.filename}`}
                    >
                      <img className="trash-thumb" src={api.thumbUrl(f.id)} alt="" loading="lazy" />
                    </button>
                    <button className="trash-meta" onClick={() => setViewing(i)}>
                      <div className="trash-name">{f.filename}</div>
                      <div className="trash-sub">
                        {fmtBytes(f.size_bytes)}
                        {left !== null && (
                          <>
                            {' · '}
                            <span className={left <= 3 ? 'trash-soon' : undefined}>
                              {left === 0 ? 'purges today' : `${left} day${left === 1 ? '' : 's'} left`}
                            </span>
                          </>
                        )}
                      </div>
                    </button>
                    <div className="trash-acts">
                      <button
                        className="trash-act"
                        onClick={() => restore(f)}
                        disabled={busy === f.id}
                        aria-label={`Put ${f.filename} back`}
                      >
                        <IC.restore />
                      </button>
                      <button
                        className="trash-act del"
                        onClick={() => purge(f)}
                        disabled={busy === f.id}
                        aria-label={`Delete ${f.filename} permanently`}
                      >
                        <IC.trash />
                      </button>
                    </div>
                  </div>
                )
              })}
            </div>
          </>
        )}
        <div className="pb16" />
      </div>

      {viewing !== null && (
        <PhotoViewer
          files={files}
          index={viewing}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
          mode="trash"
          onRemoved={(id) => setFiles((fs) => fs.filter((x) => x.id !== id))}
          toast={toast}
        />
      )}
    </div>
  )
}

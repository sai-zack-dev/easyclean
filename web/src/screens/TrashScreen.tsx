import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'
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

export function TrashScreen({ onClose, toast }: Props) {
  const [files, setFiles] = useState<MediaFile[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<number | null>(null)
  const [confirmAll, setConfirmAll] = useState(false)

  const load = () => {
    api.trash()
      .then(setFiles)
      .catch(() => toast('Could not load the trash'))
      .finally(() => setLoading(false))
  }

  useEffect(() => { load() }, [])

  const restore = (f: MediaFile) => {
    setBusy(f.id)
    api.trashRestore(f.id)
      .then(() => {
        setFiles((fs) => fs.filter((x) => x.id !== f.id))
        toast(`${f.filename} restored`)
      })
      .catch(() => toast('Restore failed'))
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
          /* Keeps the title centred between the back button and this slot. */
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
              Deleted photos land here when you finish a session, and stay for 30 days.
            </div>
          </div>
        ) : (
          <>
            <div className="trash-note">
              Kept for 30 days, then deleted automatically.
            </div>
            <div className="trash-list">
              {files.map((f) => (
                <div className="trash-row" key={f.id}>
                  <img className="trash-thumb" src={api.thumbUrl(f.id)} alt="" loading="lazy" />
                  <div className="trash-meta">
                    <div className="trash-name">{f.filename}</div>
                    <div className="trash-sub">{fmtBytes(f.size_bytes)}</div>
                  </div>
                  <div className="trash-acts">
                    <button
                      className="trash-act"
                      onClick={() => restore(f)}
                      disabled={busy === f.id}
                      aria-label={`Restore ${f.filename}`}
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
              ))}
            </div>
          </>
        )}
        <div className="pb16" />
      </div>
    </div>
  )
}

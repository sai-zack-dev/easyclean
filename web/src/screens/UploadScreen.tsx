import { useCallback, useEffect, useRef, useState, type DragEvent } from 'react'
import { IC } from '../icons'
import { api } from '../api/client'
import { type UploadItem } from '../api/uploader'
import { useUploads, uploadProgress } from '../store/uploads'
import { Spinner } from '../components/Spinner'

interface Props {
  onClose: () => void
  toast: (msg: string) => void
  /** Something arrived, so the screens behind this one are now stale. */
  onUploaded: () => void
}

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

function stateLabel(it: UploadItem): string {
  switch (it.state) {
    case 'queued':
      return 'Waiting'
    case 'hashing':
      return it.file.size > 0
        ? `Checking… ${Math.floor((it.hashed / it.file.size) * 100)}%`
        : 'Checking…'
    case 'skipped':
      return 'Already on the server'
    case 'uploading':
      return `${Math.floor((it.sent / Math.max(it.file.size, 1)) * 100)}%`
    case 'done':
      return it.savedAs && it.savedAs !== it.file.name ? `Added as ${it.savedAs}` : 'Added'
    case 'failed':
      return it.error ?? 'Failed'
  }
}

/**
 * The batch's own screen.
 *
 * It no longer *owns* the upload — `useUploads` does, so closing this is
 * leaving the room, not pulling the plug. That is why the back button here
 * closes even mid-transfer and Cancel is its own button: the two were the same
 * control only because there was nowhere for a running batch to live.
 */
export function UploadScreen({ onClose, toast, onUploaded }: Props) {
  const { items, previews, running, summary, pick, start, cancel, clear } = useUploads()
  const [quota, setQuota] = useState<{ used: number; limit: number | null } | null>(null)
  const [dragging, setDragging] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const loadQuota = useCallback(() => {
    api.upload
      .quota()
      .then((q) => setQuota({ used: q.used, limit: q.limit }))
      .catch(() => {})
  }, [])

  useEffect(() => {
    loadQuota()
  }, [loadQuota])

  // Refetched when a batch settles rather than inside `start`, so the figure is
  // right however you got back here — including opening the screen again after
  // the upload finished while you were off swiping.
  useEffect(() => {
    if (!running) loadQuota()
  }, [running, loadQuota])

  const choose = (list: FileList | null) => {
    if (!list || list.length === 0) return
    pick(Array.from(list))
  }

  const onDrop = (e: DragEvent) => {
    e.preventDefault()
    setDragging(false)
    if (!running) choose(e.dataTransfer.files)
  }

  const { total: totalBytes, pct } = uploadProgress(items)
  const remaining = quota?.limit != null ? Math.max(quota.limit - quota.used, 0) : null
  const overQuota = remaining !== null && totalBytes > remaining

  return (
    <div className="swipe-screen">
      <div className="swipe-header">
        <button className="back-btn" onClick={onClose} aria-label="Back">
          <IC.chevLeft />
        </button>
        <div className="swipe-title-wrap">
          <div className="swipe-title">Add photos</div>
          <div className="swipe-sub">
            {quota
              ? quota.limit == null
                ? `${fmtBytes(quota.used)} stored`
                : `${fmtBytes(quota.used)} of ${fmtBytes(quota.limit)} used`
              : ''}
          </div>
        </div>
        {items.length > 0 && !running ? (
          <button className="undo-btn" onClick={clear}>
            CLEAR
          </button>
        ) : (
          <span className="header-spacer" aria-hidden="true" />
        )}
      </div>

      <div className="screen">
        {quota?.limit != null && (
          <div className="up-quota">
            <div className="idx-bar">
              <div
                className="idx-fill"
                style={{ width: `${Math.min((quota.used / quota.limit) * 100, 100)}%` }}
              />
            </div>
            <div className="up-quota-note">{fmtBytes(remaining ?? 0)} free</div>
          </div>
        )}

        {items.length === 0 ? (
          <div
            className={`up-drop ${dragging ? 'over' : ''}`}
            onDragOver={(e) => {
              e.preventDefault()
              setDragging(true)
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={onDrop}
          >
            <IC.upload />
            <div className="up-drop-title">Send photos to your server</div>
            <div className="up-drop-sub">
              They land in Original, ready to swipe. Blurry shots and duplicates are found
              automatically once they arrive.
            </div>
            <button className="pill-btn" onClick={() => inputRef.current?.click()}>
              <IC.plus />
              Choose photos
            </button>
            <div className="up-drop-hint">or drop them here</div>
          </div>
        ) : (
          <>
            <div className="up-summary">
              <div className="up-summary-head">
                <span>
                  {items.length} file{items.length === 1 ? '' : 's'} · {fmtBytes(totalBytes)}
                </span>
                {running && <span className="up-pct">{pct}%</span>}
              </div>
              {running && (
                <div className="idx-bar">
                  <div className="idx-fill" style={{ width: `${pct}%` }} />
                </div>
              )}
              {running && (
                <div className="up-quota-note">
                  Keep the app open and this carries on while you use the rest of it.
                </div>
              )}
              {overQuota && !running && (
                <div className="up-warn">
                  That is more than the {fmtBytes(remaining ?? 0)} you have left. Empty the trash or
                  pick fewer files.
                </div>
              )}
              {summary && !running && (
                <div className="up-result">
                  {summary.uploaded} added
                  {summary.skipped > 0 && ` · ${summary.skipped} already there`}
                  {summary.failed > 0 && ` · ${summary.failed} failed`}
                  {summary.stoppedBecause && (
                    <div className="up-warn">{summary.stoppedBecause}</div>
                  )}
                </div>
              )}
            </div>

            <div className="up-list">
              {items.map((it) => {
                const preview = previews.get(it.key)
                return (
                  <div className={`up-row ${it.state}`} key={it.key}>
                    <div className="up-thumb">
                      {preview ? (
                        // `lazy` and `async` together are what keep a long list
                        // scrollable: a row that has never been on screen costs
                        // nothing, and one that comes into view decodes off the
                        // main thread instead of stalling the scroll on a 10 MB
                        // photo.
                        <img src={preview} alt="" loading="lazy" decoding="async" />
                      ) : (
                        <IC.imgOff />
                      )}
                    </div>
                    <div className="up-meta">
                      <div className="up-name">{it.file.name}</div>
                      <div className="up-sub">
                        {fmtBytes(it.file.size)} · {stateLabel(it)}
                      </div>
                      {it.state === 'uploading' && (
                        <div className="idx-bar up-row-bar">
                          <div
                            className="idx-fill"
                            style={{
                              width: `${Math.floor((it.sent / Math.max(it.file.size, 1)) * 100)}%`,
                            }}
                          />
                        </div>
                      )}
                    </div>
                    <div className="up-state" aria-hidden="true">
                      {it.state === 'done' && <IC.check />}
                      {it.state === 'skipped' && <IC.copy />}
                      {it.state === 'failed' && <IC.close />}
                      {(it.state === 'hashing' || it.state === 'uploading') && <Spinner />}
                    </div>
                  </div>
                )
              })}
            </div>
          </>
        )}

        <input
          ref={inputRef}
          type="file"
          multiple
          accept="image/*,video/*"
          hidden
          onChange={(e) => {
            choose(e.target.files)
            // Cleared so picking the same files again still fires a change.
            e.target.value = ''
          }}
        />
      </div>

      {items.length > 0 && (
        <div className="up-actions">
          {running ? (
            <>
              <button className="up-btn ghost" onClick={cancel}>
                Cancel
              </button>
              {/* Leaving is the ordinary thing to do now, so it gets the
                  primary slot and Cancel stops being the only way out. */}
              <button className="up-btn primary" onClick={onClose}>
                Keep uploading
              </button>
            </>
          ) : (
            <>
              <button className="up-btn ghost" onClick={() => inputRef.current?.click()}>
                Choose other files
              </button>
              {/* Once a batch has run, the primary action is leaving — pressing
                  Upload again would re-send files the server would only skip. */}
              <button
                className="up-btn primary"
                onClick={summary ? onClose : () => void start(toast, onUploaded)}
              >
                {summary ? 'Done' : `Upload ${items.length} file${items.length === 1 ? '' : 's'}`}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  )
}

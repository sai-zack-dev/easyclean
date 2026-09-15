import { useEffect, useRef } from 'react'

interface Props {
  /** Short and specific — "Delete forever?", not "Are you sure?". */
  title: string
  /** What will actually happen, in one sentence. */
  body: string
  /** The verb on the confirming button: "Delete", "Empty trash". */
  confirmLabel: string
  /** Red styling for anything that destroys data. */
  danger?: boolean
  /** Thumbnail of the file in question, so you can see what you're binning. */
  thumbUrl?: string
  busy?: boolean
  onConfirm: () => void
  onCancel: () => void
}

/**
 * A blocking yes/no for anything that loses data.
 *
 * Cancel is the default focus, not Confirm: this dialog exists because a
 * mis-tap on Delete used to be irreversible, and a confirm button under the
 * finger that just tapped Delete would put the second tap in the same place as
 * the first.
 */
export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  danger = true,
  thumbUrl,
  busy = false,
  onConfirm,
  onCancel,
}: Props) {
  const cancelRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    cancelRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
      // Enter confirms only from the focused button, which is Cancel — so the
      // keyboard's easy path is the safe one too.
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])

  return (
    <div
      className="confirm-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel()
      }}
    >
      <div className="confirm-box" role="alertdialog" aria-modal="true" aria-label={title}>
        {thumbUrl && <img className="confirm-thumb" src={thumbUrl} alt="" />}
        <div className="confirm-title">{title}</div>
        <div className="confirm-body">{body}</div>
        <div className="confirm-actions">
          <button ref={cancelRef} className="confirm-btn" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button
            className={`confirm-btn ${danger ? 'danger' : 'primary'}`}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? 'Working…' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}

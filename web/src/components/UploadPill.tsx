import { IC } from '../icons'
import { useUploads, uploadProgress } from '../store/uploads'

interface Props {
  /** Reopen the batch's own screen. */
  onOpen: () => void
}

/**
 * The running batch, while you are somewhere else.
 *
 * An upload that survives leaving its screen has to say so, or it is
 * indistinguishable from one that quietly died — this is the thread back to it.
 * It sits above the tab bar rather than under the header, because it belongs
 * to the app rather than to whichever tab happens to be on screen.
 */
export function UploadPill({ onOpen }: Props) {
  const items = useUploads((s) => s.items)
  const running = useUploads((s) => s.running)
  if (!running) return null

  const { pct } = uploadProgress(items)
  const settled = items.filter((i) => i.state === 'done' || i.state === 'skipped').length

  return (
    <button className="up-pill" onClick={onOpen} aria-live="polite">
      <span className="up-pill-bar" style={{ width: `${pct}%` }} aria-hidden="true" />
      <IC.upload />
      <span className="up-pill-text">
        Uploading {settled} of {items.length}
      </span>
      <span className="up-pill-pct">{pct}%</span>
    </button>
  )
}

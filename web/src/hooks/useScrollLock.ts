import { useEffect } from 'react'

/**
 * Freeze the page behind a modal.
 *
 * `body { overflow: hidden }` does nothing here: the app is a flex column and
 * `.screen` is the element that scrolls, not the document. So this sets a body
 * class and the stylesheet stops `.screen` scrolling — the same approach the
 * photo viewer already uses.
 *
 * Counted rather than toggled, because modals stack: the Add-to-album sheet can
 * open over the vault grid, and the first one to close must not unlock the page
 * while the second is still up.
 */
let depth = 0

export function useScrollLock(active = true) {
  useEffect(() => {
    if (!active) return
    depth += 1
    document.body.classList.add('modal-open')
    return () => {
      depth -= 1
      if (depth <= 0) {
        depth = 0
        document.body.classList.remove('modal-open')
      }
    }
  }, [active])
}

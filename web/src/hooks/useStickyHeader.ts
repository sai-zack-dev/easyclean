import { useRef, useCallback } from 'react'

/**
 * Wire up a scrolling `.screen` so its sticky `.page-header` grows a hairline
 * once anything has scrolled beneath it.
 *
 * Toggled as a class rather than React state on purpose: scroll fires on every
 * frame, and re-rendering a photo grid that often is how you drop frames.
 *
 *   const { ref, onScroll } = useStickyHeader()
 *   <div className="screen" ref={ref} onScroll={onScroll}>
 */
export function useStickyHeader<T extends HTMLElement = HTMLDivElement>() {
  const ref = useRef<T>(null)

  const onScroll = useCallback(() => {
    const el = ref.current
    if (el) el.classList.toggle('scrolled', el.scrollTop > 4)
  }, [])

  return { ref, onScroll }
}

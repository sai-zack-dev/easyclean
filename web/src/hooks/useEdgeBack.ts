import { useCallback, useRef } from 'react'

/** How far in from the left edge a back gesture may start. */
const EDGE = 28
/** Rightward travel that commits to going back. */
const COMMIT = 80
/** Travel before the gesture claims an axis, so a diagonal scroll isn't a back. */
const SLOP = 10

/**
 * The platform back gesture — drag in from the left edge to leave a sub-screen.
 *
 * Every screen this is used on is reached from somewhere and has a back button
 * in its header, but a button is a target you have to aim at with the hand
 * already holding the phone. The gesture is the same action without the aim,
 * and it is the one a phone user reaches for first.
 *
 * The screen follows the finger and springs back if the drag is abandoned, so
 * it says what it will do before you commit. Only a drag that *starts* within
 * `EDGE` of the left edge qualifies; anything further in is an ordinary scroll
 * or a tap on a photo.
 *
 * Deliberately ref-free — the element comes from `currentTarget` — because the
 * screens that want this also want `useStickyHeader`, and two hooks handing
 * back two refs for one node is a merge every caller would have to write.
 *
 *   <div className="screen" {...useEdgeBack(onClose)}>
 */
export function useEdgeBack<T extends HTMLElement = HTMLDivElement>(
  onBack: () => void,
  enabled = true,
) {
  // A ref, not state: this is written on every pointermove, and re-rendering a
  // grid of photos at that rate is how you drop frames.
  const g = useRef({
    node: null as T | null,
    active: false,
    x: 0,
    y: 0,
    axis: '' as '' | 'x' | 'y',
  })

  const slide = useCallback((dx: number, animate: boolean) => {
    const node = g.current.node
    if (!node) return
    node.style.transition = animate ? 'transform .2s cubic-bezier(.25,.46,.45,.94)' : 'none'
    node.style.transform = dx ? `translate3d(${dx}px, 0, 0)` : ''
  }, [])

  const onPointerDown = useCallback(
    (e: React.PointerEvent<T>) => {
      // Touch only. On a desktop the left edge of the pane is somewhere you
      // drag to select, not a way out of the screen.
      if (!enabled || e.pointerType !== 'touch') return
      const node = e.currentTarget
      if (e.clientX - node.getBoundingClientRect().left > EDGE) return
      g.current = { node, active: true, x: e.clientX, y: e.clientY, axis: '' }
      try {
        node.setPointerCapture(e.pointerId)
      } catch {
        /* the gesture still tracks without capture; it just ends at the edge */
      }
    },
    [enabled],
  )

  const onPointerMove = useCallback(
    (e: React.PointerEvent<T>) => {
      const s = g.current
      if (!s.active) return
      const dx = e.clientX - s.x
      const dy = e.clientY - s.y
      if (!s.axis && (Math.abs(dx) > SLOP || Math.abs(dy) > SLOP)) {
        s.axis = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y'
        // Scrolling won the axis: let go entirely rather than fight the
        // scroller for the rest of the drag.
        if (s.axis === 'y') s.active = false
      }
      if (s.axis === 'x') slide(Math.max(dx, 0), false)
    },
    [slide],
  )

  const onPointerUp = useCallback(
    (e: React.PointerEvent<T>) => {
      const s = g.current
      if (!s.active) return
      s.active = false
      const dx = e.clientX - s.x
      if (s.axis === 'x' && dx > COMMIT) {
        // Carry the throw off-screen rather than snapping back, then leave.
        // The node unmounts with the screen, so nothing has to undo this.
        slide(window.innerWidth, true)
        window.setTimeout(onBack, 170)
        return
      }
      slide(0, true)
    },
    [slide, onBack],
  )

  return { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp }
}

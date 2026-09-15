interface Props {
  val: number
  total: number
  size?: number
}

/**
 * Ring showing how much of a month has been reviewed.
 *
 * Colours come from CSS tokens, never literal `rgba(255,255,255,…)`: a white
 * ring is invisible on the light theme's white cards.
 */
export function ProgressRing({ val, total, size = 28 }: Props) {
  const pct = total > 0 ? Math.min(val / total, 1) : 0
  const r = size / 2 - 3
  const c = 2 * Math.PI * r

  // Finished: filled with the accent, matching the streak rings. A grey outline
  // with a coloured tick read as "not quite done" next to them.
  if (pct >= 1) {
    return (
      <div
        style={{
          width: size,
          height: size,
          borderRadius: '50%',
          background: 'var(--accent)',
          border: '2px solid var(--accent)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <svg
          viewBox="0 0 24 24"
          width={12}
          height={12}
          style={{
            stroke: 'var(--accent-on)',
            fill: 'none',
            strokeWidth: 3,
            strokeLinecap: 'round',
            strokeLinejoin: 'round',
          }}
        >
          <polyline points="20 6 9 17 4 12" />
        </svg>
      </div>
    )
  }

  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="prog-ring">
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--ring)" strokeWidth={2.5} />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={2.5}
        strokeDasharray={`${c * pct} ${c}`}
        strokeLinecap="round"
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
    </svg>
  )
}

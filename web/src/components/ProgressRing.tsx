interface Props {
  val: number
  total: number
  size?: number
}

export function ProgressRing({ val, total, size = 28 }: Props) {
  const pct = total > 0 ? Math.min(val / total, 1) : 0
  const r = size / 2 - 3
  const c = 2 * Math.PI * r

  if (pct >= 1) {
    return (
      <div
        style={{
          width: size,
          height: size,
          borderRadius: '50%',
          border: '2px solid rgba(255,255,255,.35)',
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
            stroke: 'rgba(255,255,255,.8)',
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
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="rgba(255,255,255,.1)" strokeWidth={2.5} />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="rgba(255,255,255,.65)"
        strokeWidth={2.5}
        strokeDasharray={`${c * pct} ${c}`}
        strokeLinecap="round"
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
    </svg>
  )
}

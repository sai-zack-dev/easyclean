import { useState, useEffect, useRef } from 'react'
import { IC } from '../icons'
import { api, type StatsSummary, type StorageDay } from '../api/client'
import { useSession, type Theme } from '../store/session'
import { ACCENTS } from '../theme'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

interface Props {
  toast: (msg: string) => void
}

/**
 * Duplicate-sensitivity steps, as pHash Hamming distance (how many of the 64
 * hash bits two photos may differ in and still count as the same shot).
 *
 * Higher distance = looser match = more photos flagged, so the slider reads
 * left-to-right as "flag fewer" → "flag more". 16 is the practical ceiling
 * before genuinely different photos start grouping together.
 */
const SENSITIVITY = [
  { label: 'Low', distance: 4, hint: 'Near-identical copies only' },
  { label: 'Medium', distance: 10, hint: 'Same shot, minor edits or re-saves' },
  { label: 'High', distance: 16, hint: 'Similar-looking shots — more false matches' },
]

/** Nearest slider step for a stored distance (which may come from env config). */
function stepFor(distance: number) {
  let best = 0
  SENSITIVITY.forEach((s, i) => {
    if (Math.abs(s.distance - distance) < Math.abs(SENSITIVITY[best].distance - distance)) best = i
  })
  return best
}

/**
 * A settings row. Declared at module scope, not inside `SettingsTab`: a
 * component created during render is a brand-new type on every render, so React
 * unmounts and remounts it (losing any state inside) each time.
 */
function SRow({
  icon,
  label,
  sub,
  right,
  onClick,
}: {
  icon: keyof typeof IC
  label: string
  sub?: string
  right?: React.ReactNode
  onClick?: () => void
}) {
  const El = IC[icon] ?? IC.settings
  return (
    <div className="srow" onClick={onClick}>
      <div className="srow-left">
        <div className="srow-icon"><El /></div>
        <div>
          <div className="srow-label">{label}</div>
          {sub && <div className="srow-sub">{sub}</div>}
        </div>
      </div>
      <div className="srow-right">
        {right}
        <IC.chevRight />
      </div>
    </div>
  )
}

/** Bar chart of bytes binned per day. Real data, zero-filled by the server. */
function StorageChart({ days }: { days: StorageDay[] }) {
  const max = Math.max(...days.map((d) => d.bytes), 1)
  const fmtDay = (secs: number) =>
    new Date(secs * 1000).toLocaleDateString('en-US', { month: 'short', day: '2-digit' })

  // Five evenly-spaced ticks, taken from the actual series rather than
  // hardcoded — the axis has to agree with the bars above it.
  const tickAt = [0, 0.25, 0.5, 0.75, 1].map((f) => Math.round(f * (days.length - 1)))

  return (
    <>
      <div className="bar-row">
        {days.map((d) => (
          <div
            key={d.day}
            className={`b ${d.bytes > 0 ? 'hi' : ''}`}
            style={{ height: `${Math.max((d.bytes / max) * 100, 2)}%` }}
            title={`${fmtDay(d.day)} — ${fmtBytes(d.bytes)} (${d.files} file${d.files === 1 ? '' : 's'})`}
          />
        ))}
      </div>
      <div className="chart-dates">
        {tickAt.map((i) => (
          <span key={i} className="chart-date">{days[i] ? fmtDay(days[i].day) : ''}</span>
        ))}
      </div>
    </>
  )
}

export function SettingsTab({ toast }: Props) {
  const { user, theme, setTheme, accentIndex, setAccentIndex, logout } = useSession()
  const [stats, setStats] = useState<StatsSummary | null>(null)
  const [storage, setStorage] = useState<StorageDay[] | null>(null)
  const [sens, setSens] = useState<number | null>(null)
  const [savingSens, setSavingSens] = useState(false)

  useEffect(() => {
    api.statsSummary().then(setStats).catch(() => {})
    api.statsStorage(30).then((s) => setStorage(s.days)).catch(() => {})
    api.settings()
      .then((s) => setSens(stepFor(s.duplicate_distance)))
      .catch(() => {})
  }, [])

  const sensTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Saving re-clusters every hashed photo server-side, so debounce rather than
  // firing per step the slider passes through. Debounced off `onChange`, not
  // `onPointerUp`: a drag released outside the control never sends a pointerup
  // here, and the setting would silently not save.
  const onSens = (step: number) => {
    setSens(step)
    if (sensTimer.current) clearTimeout(sensTimer.current)
    sensTimer.current = setTimeout(() => commitSens(step), 350)
  }

  useEffect(() => () => {
    if (sensTimer.current) clearTimeout(sensTimer.current)
  }, [])

  const commitSens = (step: number) => {
    setSavingSens(true)
    api.saveSettings({ duplicate_distance: SENSITIVITY[step].distance })
      .then((r) => {
        toast(
          r.duplicate_groups === null
            ? 'Sensitivity saved'
            : `${SENSITIVITY[step].label} — ${r.duplicate_groups} duplicate group${r.duplicate_groups === 1 ? '' : 's'}`,
        )
      })
      .catch(() => toast('Could not save sensitivity'))
      .finally(() => setSavingSens(false))
  }

  const storedBytes = storage?.reduce((a, d) => a + d.bytes, 0) ?? 0
  const storedFiles = storage?.reduce((a, d) => a + d.files, 0) ?? 0

  return (
    <div className="screen">
      <div className="page-header">
        <div>
          <div className="page-title">Setting</div>
          <div className="page-subtitle">{user?.role === 'admin' ? 'Admin' : 'User'}</div>
        </div>
        <div className="user-pill">
          <div className="user-pill-av"><IC.user /></div>
          <div className="user-pill-name">{user?.username ?? 'User'}</div>
        </div>
      </div>

      {/* Storage chart — real figures from /api/stats/storage */}
      <div className="stor-card">
        <div className="stor-header">
          <div>
            <div className="stor-lbl">Storage Saved</div>
            <div className="stor-days">Last 30 days</div>
          </div>
          <div style={{ textAlign: 'right' }}>
            <div className="stor-amount">{fmtBytes(storedBytes)}</div>
            <div className="stor-change">
              {storedFiles} file{storedFiles === 1 ? '' : 's'} binned
            </div>
          </div>
        </div>
        {storage === null ? (
          <div className="stor-placeholder">Loading…</div>
        ) : storedBytes === 0 ? (
          <div className="stor-placeholder">
            Nothing binned in the last 30 days. Delete some photos and this fills in.
          </div>
        ) : (
          <StorageChart days={storage} />
        )}
      </div>

      {/* Library — the other real numbers we already have */}
      <div className="settings-section">
        <div className="st-label">Library</div>
        <div className="settings-group">
          <SRow
            icon="photo"
            label="Indexed media"
            sub={`${stats?.pending ?? 0} still to review`}
            right={String(stats?.total ?? 0)}
          />
          <SRow
            icon="folder"
            label="Library size"
            sub={`${fmtBytes(stats?.trashed_size ?? 0)} currently in trash`}
            right={fmtBytes(stats?.total_size ?? 0)}
          />
          <SRow
            icon="blur"
            label="Flagged blurry"
            sub="Laplacian variance below the threshold"
            right={String(stats?.blurry ?? 0)}
          />
        </div>
      </div>

      {/* App Settings */}
      <div className="settings-section">
        <div className="st-label">App Settings</div>
        <div className="settings-group">
          <div className="srow srow-stack">
            <div className="srow-left">
              <div className="srow-icon"><IC.sliders /></div>
              <div>
                <div className="srow-label">Duplicate Sensitivity</div>
                <div className="srow-sub">
                  {sens === null ? 'Loading…' : SENSITIVITY[sens].hint}
                </div>
              </div>
            </div>
            <div className="sens-row">
              <input
                type="range"
                min={0}
                max={SENSITIVITY.length - 1}
                step={1}
                value={sens ?? 1}
                disabled={sens === null}
                onChange={(e) => onSens(Number(e.target.value))}
                aria-label="Duplicate sensitivity"
              />
              <span className="sens-val">
                {sens === null ? '—' : savingSens ? 'Saving…' : SENSITIVITY[sens].label}
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* Appearance */}
      <div className="settings-section">
        <div className="st-label">Appearance</div>
        <div className="settings-group">
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.palette /></div>
              <div className="srow-label">Theme</div>
            </div>
            <div className="theme-opts">
              {(['dark', 'light', 'system'] as Theme[]).map((t) => (
                <div
                  key={t}
                  className={`th-opt ${theme === t ? 'active' : ''}`}
                  onClick={() => setTheme(t)}
                >
                  {t.charAt(0).toUpperCase() + t.slice(1)}
                </div>
              ))}
            </div>
          </div>
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.drop /></div>
              <div>
                <div className="srow-label">Accent Color</div>
                <div className="srow-sub">{ACCENTS[accentIndex]?.name ?? 'Default'}</div>
              </div>
            </div>
            <div className="accent-row">
              {ACCENTS.map((a, i) => (
                <button
                  key={a.name}
                  className={`ac-dot ${accentIndex === i ? 'active' : ''} ${a.color === null ? 'ac-auto' : ''}`}
                  style={a.color ? { background: a.color } : undefined}
                  onClick={() => setAccentIndex(i)}
                  aria-label={a.name}
                  aria-pressed={accentIndex === i}
                  title={a.name}
                />
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Language */}
      <div className="settings-section">
        <div className="st-label">Language</div>
        <div className="settings-group">
          <SRow icon="globe" label="Language" right="English" />
        </div>
      </div>

      {/* Logout */}
      <div className="settings-section">
        <div className="settings-group">
          <button className="srow srow-danger" onClick={logout}>
            <div className="srow-left">
              <div className="srow-icon"><IC.logout /></div>
              <div className="srow-label">Log out</div>
            </div>
          </button>
        </div>
      </div>
      <div className="pb16" />
    </div>
  )
}

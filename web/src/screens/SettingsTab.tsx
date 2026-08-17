import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type StatsSummary } from '../api/client'
import { useSession, type Theme } from '../store/session'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

interface Props {
  toast: (msg: string) => void
}

const ACCENTS = ['#ffffff', '#00C7BE', '#30D158', '#FFD60A', '#FF9F0A']
const BARS = [0.3, 0.5, 0.8, 1.2, 1.8, 2.1, 1.5, 2.4, 2.8, 2.2, 1.9, 2.6, 2.3, 1.7, 1.4, 1.6, 2.0, 2.5, 2.9, 2.7, 2.1, 1.8, 2.4, 2.8, 3.0, 2.3, 1.9, 2.2]
const BAR_MAX = Math.max(...BARS)

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

export function SettingsTab({ toast }: Props) {
  const { backendUrl, setBackendUrl, user, theme, setTheme, accentIndex, setAccentIndex, logout } = useSession()
  const [url, setUrl] = useState(backendUrl)
  const [zoom, setZoom] = useState(true)
  const [stats, setStats] = useState<StatsSummary | null>(null)

  useEffect(() => {
    api.statsSummary().then(setStats).catch(() => {})
  }, [])

  const saveUrl = () => {
    const t = url.trim().replace(/\/$/, '')
    setBackendUrl(t)
    toast('Server URL saved!')
  }

  return (
    <div className="screen">
      <div className="page-header">
        <div>
          <div className="page-title">Setting</div>
          <div className="page-subtitle">‹ Admin User ›</div>
        </div>
        <div className="user-pill">
          <div className="user-pill-av"><IC.user /></div>
          <div className="user-pill-name">{user?.username ?? 'User'}</div>
        </div>
      </div>

      {/* Storage chart */}
      <div className="stor-card">
        <div className="stor-header">
          <div>
            <div className="stor-lbl">Storage Saved</div>
            <div className="stor-days">Last 30 days</div>
          </div>
          <div>
            <div className="stor-amount">{fmtBytes(stats?.trashed_size ?? 0)}</div>
            <div className="stor-change">+12% vs last 30 days</div>
          </div>
        </div>
        <div className="bar-row">
          {BARS.map((h, i) => (
            <div key={i} className={`b ${i >= 20 ? 'hi' : ''}`} style={{ height: `${(h / BAR_MAX) * 100}%` }} />
          ))}
        </div>
        <div className="chart-dates">
          {['May 04', 'May 11', 'May 18', 'May 25', 'Jun 01'].map((d) => (
            <span key={d} className="chart-date">{d}</span>
          ))}
        </div>
      </div>

      {/* Connection */}
      <div className="settings-section">
        <div className="st-label">Connection & Storage</div>
        <div className="settings-group">
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.server /></div>
              <div className="srow-label">Server Status</div>
            </div>
            <div className="srow-right">
              <span className="online-dot" />
              Online
              <IC.chevRight />
            </div>
          </div>
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.link /></div>
              <div className="srow-label">Server URL</div>
            </div>
            <div className="srow-right" style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {/* Empty backendUrl means same-origin — show the host actually
                  in use, not a misleading "localhost". */}
              {backendUrl || window.location.origin}
              <IC.chevRight />
            </div>
          </div>
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.folder /></div>
              <div className="srow-label">Folder Paths</div>
            </div>
            <div className="srow-right">›</div>
          </div>
        </div>
      </div>

      {/* Backend URL input */}
      <div className="settings-section">
        <div className="st-label">Backend URL</div>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            style={{
              flex: 1,
              background: 'var(--bg3)',
              border: '1px solid var(--border2)',
              borderRadius: 'var(--r)',
              padding: '12px 14px',
              color: 'var(--text)',
              fontFamily: 'var(--font)',
              fontSize: 14,
              outline: 'none',
            }}
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="leave empty to use this server"
          />
          <button className="pill-btn" onClick={saveUrl}>Save</button>
        </div>
      </div>

      {/* App Settings */}
      <div className="settings-section">
        <div className="st-label">App Settings</div>
        <div className="settings-group">
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.sliders /></div>
              <div>
                <div className="srow-label">Duplicate Sensitivity</div>
                <div className="srow-sub">Medium</div>
              </div>
            </div>
            <div className="srow-right" style={{ minWidth: 120 }}>
              <input type="range" min={1} max={3} defaultValue={2} style={{ flex: 1 }} />
            </div>
          </div>
          <div className="srow">
            <div className="srow-left">
              <div className="srow-icon"><IC.sync /></div>
              <div>
                <div className="srow-label">Zoom Sync</div>
                <div className="srow-sub">Sync zoom across devices</div>
              </div>
            </div>
            <div
              className="tog"
              onClick={() => setZoom((z) => !z)}
              style={{ background: zoom ? 'var(--keep)' : 'var(--surface)' }}
            >
              <div className="tog-knob" style={{ transform: zoom ? 'translateX(20px)' : '' }} />
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
              <div className="srow-label">Accent Color</div>
            </div>
            <div className="accent-row">
              {ACCENTS.map((c, i) => (
                <div
                  key={i}
                  className={`ac-dot ${accentIndex === i ? 'active' : ''}`}
                  style={{ background: c, border: c === '#ffffff' ? '2px solid rgba(255,255,255,.3)' : 'none' }}
                  onClick={() => setAccentIndex(i)}
                />
              ))}
              <div className="ac-dot ac-add"><IC.plus /></div>
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
          <div className="srow" onClick={logout}>
            <div className="srow-left">
              <div className="srow-icon"><IC.logout /></div>
              <div className="srow-label" style={{ color: 'var(--delete)' }}>Log out</div>
            </div>
          </div>
        </div>
      </div>
      <div className="pb16" />
    </div>
  )
}

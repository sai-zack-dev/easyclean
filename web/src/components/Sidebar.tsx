import { IC } from '../icons'
import type { TabId } from '../App'

const TABS: { id: TabId; label: string; icon: keyof typeof IC }[] = [
  { id: 'clean', label: 'Clean', icon: 'broom' },
  { id: 'gallery', label: 'Gallery', icon: 'photo' },
  { id: 'albums', label: 'Albums', icon: 'album' },
  { id: 'setting', label: 'Setting', icon: 'settings' },
]

interface Props {
  tab: TabId
  setTab: (t: TabId) => void
  username?: string
}

export function Sidebar({ tab, setTab, username }: Props) {
  const initials = username
    ? username.slice(0, 2).toUpperCase()
    : 'EC'

  return (
    <nav className="sidebar">
      <div className="sidebar-brand">
        <div className="sidebar-brand-icon">
          <IC.broom />
        </div>
        <div>
          <div className="sidebar-brand-name">EasyClean</div>
          <div className="sidebar-brand-ver">MVP</div>
        </div>
      </div>

      {TABS.map((t) => {
        const El = IC[t.icon]
        return (
          <div
            key={t.id}
            className={`nav-item ${tab === t.id ? 'active' : ''}`}
            onClick={() => setTab(t.id)}
          >
            <El />
            {t.label}
          </div>
        )
      })}

      <div className="sidebar-spacer" />

      <div className="sidebar-user">
        <div className="sidebar-avatar">{initials}</div>
        <div>
          <div className="sidebar-username">{username ?? 'User'}</div>
          <div className="sidebar-role">Admin</div>
        </div>
      </div>
    </nav>
  )
}

export function TabBar({ tab, setTab }: { tab: TabId; setTab: (t: TabId) => void }) {
  return (
    <div className="tabbar">
      {TABS.map((t) => {
        const El = IC[t.icon]
        return (
          <button
            key={t.id}
            className={`tab-btn ${tab === t.id ? 'active' : ''}`}
            onClick={() => setTab(t.id)}
          >
            <El />
            <span className="tab-label">{t.label}</span>
          </button>
        )
      })}
    </div>
  )
}

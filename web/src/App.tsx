import { useState, useRef, useEffect } from 'react'
import { useSession } from './store/session'
import { applyAccent } from './theme'
import { Sidebar, TabBar } from './components/Sidebar'
import { Toast } from './components/Toast'
import { LoginScreen } from './screens/LoginScreen'
import { ServerSetup } from './screens/ServerSetup'
import { CleanTab } from './screens/CleanTab'
import { SwipeScreen } from './screens/SwipeScreen'
import { GalleryTab } from './screens/GalleryTab'
import { AlbumsTab } from './screens/AlbumsTab'
import { SettingsTab } from './screens/SettingsTab'
import { TrashScreen } from './screens/TrashScreen'

export type TabId = 'clean' | 'gallery' | 'albums' | 'setting'

interface SwipeTarget {
  mode: string
  month?: string
  label: string
}

export function App() {
  const { token, user, theme, accentIndex, backendUrl } = useSession()
  const [tab, setTab] = useState<TabId>('clean')
  const [swipe, setSwipe] = useState<SwipeTarget | null>(null)
  const [trashOpen, setTrashOpen] = useState(false)
  const [toastMsg, setToastMsg] = useState('')
  const [serverReady, setServerReady] = useState(import.meta.env.DEV)
  const tmr = useRef<ReturnType<typeof setTimeout> | null>(null)

  const toast = (msg: string) => {
    setToastMsg(msg)
    if (tmr.current) clearTimeout(tmr.current)
    tmr.current = setTimeout(() => setToastMsg(''), 2500)
  }

  useEffect(() => {
    const root = document.documentElement
    if (theme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches
      root.setAttribute('data-theme', prefersDark ? 'dark' : 'light')
    } else {
      root.setAttribute('data-theme', theme)
    }
  }, [theme])

  // Re-applied on theme change too: "Default" clears the inline override so the
  // theme's own --accent takes over, and that has to happen after the swap.
  useEffect(() => {
    applyAccent(accentIndex)
  }, [accentIndex, theme])

  useEffect(() => {
    if (import.meta.env.DEV || serverReady) return
    fetch(`${backendUrl}/api/health`, { signal: AbortSignal.timeout(3000) })
      .then((r) => { if (r.ok) setServerReady(true) })
      .catch(() => {})
  }, [backendUrl, serverReady])

  if (!serverReady) {
    return <ServerSetup onDone={() => setServerReady(true)} />
  }

  if (!token || !user) {
    return <LoginScreen onLogin={() => {}} />
  }

  return (
    <div id="app">
      {toastMsg && <Toast msg={toastMsg} />}
      {swipe ? (
        <SwipeScreen
          mode={swipe.mode}
          month={swipe.month}
          label={swipe.label}
          onClose={() => setSwipe(null)}
          toast={toast}
        />
      ) : trashOpen ? (
        <TrashScreen onClose={() => setTrashOpen(false)} toast={toast} />
      ) : (
        <>
          <Sidebar tab={tab} setTab={setTab} username={user.username} />
          <div className="main">
            {tab === 'clean' && (
              <CleanTab onSwipe={(t) => setSwipe(t)} toast={toast} />
            )}
            {tab === 'gallery' && <GalleryTab />}
            {tab === 'albums' && <AlbumsTab onOpenTrash={() => setTrashOpen(true)} />}
            {tab === 'setting' && <SettingsTab toast={toast} />}
            <TabBar tab={tab} setTab={setTab} />
          </div>
        </>
      )}
    </div>
  )
}

export default App

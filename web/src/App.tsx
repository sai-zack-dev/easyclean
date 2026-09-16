import { useState, useRef, useEffect, useCallback } from 'react'
import { useSession } from './store/session'
import { applyAccent } from './theme'
import { onUpdateAvailable, applyUpdate } from './pwa'
import { Sidebar, TabBar } from './components/Sidebar'
import { Toast } from './components/Toast'
import { LoginScreen } from './screens/LoginScreen'
import { ServerSetup } from './screens/ServerSetup'
import { CleanTab } from './screens/CleanTab'
import { SwipeScreen } from './screens/SwipeScreen'
import { DuplicateScreen } from './screens/DuplicateScreen'
import { GalleryTab } from './screens/GalleryTab'
import { AlbumsTab } from './screens/AlbumsTab'
import { SettingsTab } from './screens/SettingsTab'
import { TrashScreen } from './screens/TrashScreen'
import { VaultScreen } from './screens/VaultScreen'

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
  const [dupOpen, setDupOpen] = useState(false)
  const [trashOpen, setTrashOpen] = useState(false)
  const [vaultOpen, setVaultOpen] = useState(false)
  const [toastMsg, setToastMsg] = useState('')
  /** A newer build is installed and waiting for a reload. */
  const [updateReady, setUpdateReady] = useState(false)
  const [serverReady, setServerReady] = useState(import.meta.env.DEV)
  const tmr = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Stable identity on purpose: screens list `toast` in effect dependencies, so
  // a fresh function each render re-ran those effects every time a toast came
  // and went — which in the duplicate compare screen meant refetching the
  // groups and throwing away the review in progress.
  const toast = useCallback((msg: string) => {
    setToastMsg(msg)
    if (tmr.current) clearTimeout(tmr.current)
    tmr.current = setTimeout(() => setToastMsg(''), 2500)
  }, [])

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

  // Surfaced as a banner rather than only in Setting: an installed PWA has no
  // address bar and no reload, so without this the app can sit on an old build
  // indefinitely without ever saying so.
  useEffect(() => onUpdateAvailable(setUpdateReady), [])

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
      {updateReady && (
        <div className="update-banner" role="status">
          <span>A new version of EasyClean is ready.</span>
          <button onClick={applyUpdate}>Update</button>
          <button className="ghost" onClick={() => setUpdateReady(false)} aria-label="Dismiss">
            Later
          </button>
        </div>
      )}
      {swipe ? (
        <SwipeScreen
          mode={swipe.mode}
          month={swipe.month}
          label={swipe.label}
          onClose={() => setSwipe(null)}
          toast={toast}
        />
      ) : dupOpen ? (
        // Duplicates get their own screen, not the swipe deck: the decision is
        // between copies, so they have to be on screen together.
        <DuplicateScreen onClose={() => setDupOpen(false)} toast={toast} />
      ) : trashOpen ? (
        <TrashScreen onClose={() => setTrashOpen(false)} toast={toast} />
      ) : vaultOpen ? (
        // Replaces the whole shell, tab bar included: the vault is not a tab,
        // and leaving one visible would be a way back out that skips the lock.
        <VaultScreen onClose={() => setVaultOpen(false)} toast={toast} />
      ) : (
        <>
          <Sidebar tab={tab} setTab={setTab} username={user.username} />
          <div className="main">
            {tab === 'clean' && (
              <CleanTab
                onSwipe={(t) => setSwipe(t)}
                onDuplicates={() => setDupOpen(true)}
                toast={toast}
              />
            )}
            {tab === 'gallery' && <GalleryTab toast={toast} />}
            {tab === 'albums' && (
              <AlbumsTab onOpenTrash={() => setTrashOpen(true)} toast={toast} />
            )}
            {tab === 'setting' && (
              <SettingsTab toast={toast} onSecret={() => setVaultOpen(true)} />
            )}
            <TabBar tab={tab} setTab={setTab} />
          </div>
        </>
      )}
    </div>
  )
}

export default App

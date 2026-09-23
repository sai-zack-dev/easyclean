import { useState, useRef, useEffect, useCallback } from 'react'
import { useSession } from './store/session'
import { ensureMediaToken, clearMediaToken } from './api/client'
import { applyAccent } from './theme'
import { onUpdateAvailable, applyUpdate } from './pwa'
import { Sidebar, TabBar } from './components/Sidebar'
import { Toast } from './components/Toast'
import { UploadPill } from './components/UploadPill'
import { LoginScreen } from './screens/LoginScreen'
import { ServerSetup } from './screens/ServerSetup'
import { CleanTab } from './screens/CleanTab'
import { SwipeScreen } from './screens/SwipeScreen'
import { DuplicateScreen } from './screens/DuplicateScreen'
import { GalleryTab } from './screens/GalleryTab'
import { AlbumsTab } from './screens/AlbumsTab'
import { SettingsTab } from './screens/SettingsTab'
import { TrashScreen } from './screens/TrashScreen'
import { UploadScreen } from './screens/UploadScreen'
import { VaultScreen } from './screens/VaultScreen'
import { useUploads } from './store/uploads'

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
  const [uploadOpen, setUploadOpen] = useState(false)
  const [vaultOpen, setVaultOpen] = useState(false)
  /**
   * Bumped when an upload lands, so the tab underneath refetches on return.
   *
   * A counter rather than a boolean: two batches in a row have to be two
   * distinct changes, or the second one leaves the Clean tab showing counts
   * from before it.
   */
  const [libraryVersion, setLibraryVersion] = useState(0)
  const [toastMsg, setToastMsg] = useState('')
  /** A newer build is installed and waiting for a reload. */
  const [updateReady, setUpdateReady] = useState(false)
  const [serverReady, setServerReady] = useState(import.meta.env.DEV)
  /** The session token we last minted a media token for. */
  const [mediaFor, setMediaFor] = useState<string | null>(null)
  const tmr = useRef<ReturnType<typeof setTimeout> | null>(null)
  /** A batch is in flight somewhere — it no longer needs its screen to run. */
  const uploading = useUploads((s) => s.running)

  /**
   * Leave the Add screen, and put the finished batch away with it.
   *
   * Only when nothing is running: a live batch keeps its list, because that
   * list *is* the batch and the pill leads straight back to it. A settled one
   * has already said what happened, in a toast, so holding on to it would mean
   * opening "Add photos" next time onto last week's receipt — and holding its
   * previews, which are object URLs over the whole selection.
   */
  const closeUpload = useCallback(() => {
    setUploadOpen(false)
    const s = useUploads.getState()
    if (!s.running) s.clear()
  }, [])

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

  // The one thing a background upload still cannot survive is the page going
  // away — there is no Background Fetch on iOS, and a service worker cannot be
  // handed files picked in a document that no longer exists. So say so, on the
  // platforms that will ask.
  useEffect(() => {
    if (!uploading) return
    const warn = (e: BeforeUnloadEvent) => e.preventDefault()
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [uploading])

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

  // Mint the read-only token that image and stream URLs carry. Runs on login
  // and on every launch with a stored session, because it is deliberately not
  // persisted — a cheap request instead of a third credential on disk.
  //
  // Readiness is derived from *which* session we minted for rather than kept
  // as its own flag, so logging out closes the gate by itself: `token` changes
  // and `mediaFor` no longer matches, with no second state update to sequence.
  useEffect(() => {
    if (!token) {
      // Goes with the session that minted it, including when a 401 logs us out
      // from under the UI.
      clearMediaToken()
      return
    }
    let live = true
    let retry: ReturnType<typeof setTimeout> | undefined
    const attempt = () => {
      void ensureMediaToken().then((ok) => {
        if (!live) return
        // Keep trying rather than settling into a permanently blank screen:
        // the usual reason to land here is the server still coming back up.
        if (ok) setMediaFor(token)
        else retry = setTimeout(attempt, 2000)
      })
    }
    attempt()
    return () => {
      live = false
      if (retry) clearTimeout(retry)
    }
  }, [token])

  if (!serverReady) {
    return <ServerSetup onDone={() => setServerReady(true)} />
  }

  if (!token || !user) {
    return <LoginScreen onLogin={() => {}} />
  }

  if (mediaFor !== token) {
    // Every image URL carries the media token, so the first paint has to wait
    // for it — rendering a grid of URLs with an empty token would be a screen
    // of broken thumbnails that only a reload fixes. It is one request.
    return <div id="app" aria-busy="true" />
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
      {/* Not inside the tab shell: a batch keeps running through the swipe
          deck, the duplicate compare and the trash too. Only its own screen
          and the vault suppress it — one already shows the batch in full, and
          the other is a screen nothing may point back out of. */}
      {uploading && !uploadOpen && !vaultOpen && (
        <UploadPill onOpen={() => setUploadOpen(true)} />
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
      ) : uploadOpen ? (
        <UploadScreen
          onClose={closeUpload}
          toast={toast}
          onUploaded={() => setLibraryVersion((v) => v + 1)}
        />
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
                onUpload={() => setUploadOpen(true)}
                refreshKey={libraryVersion}
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

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { dropCachedThumbs } from '../pwa'
import { SAME_ORIGIN, normalizeBackendUrl } from '../backendUrl'

export type Theme = 'dark' | 'light' | 'system'

export interface SessionUser {
  username: string
  role?: string
}

/*
 * `SAME_ORIGIN` (the empty string) means "same origin as this page", i.e.
 * relative `/api/...` calls — the normal case, since the Rust server serves
 * this bundle. It lives in ../backendUrl alongside the rule for what may be
 * stored in its place.
 */

interface SessionState {
  token: string | null
  backendUrl: string
  user: SessionUser | null
  theme: Theme
  accentIndex: number
  setToken: (token: string | null) => void
  setBackendUrl: (url: string) => void
  setUser: (user: SessionUser | null) => void
  setTheme: (theme: Theme) => void
  setAccentIndex: (idx: number) => void
  logout: () => void
}

export const useSession = create<SessionState>()(
  persist(
    (set) => ({
      token: null,
      backendUrl: SAME_ORIGIN,
      user: null,
      theme: 'dark',
      accentIndex: 0,
      setToken: (token) => set({ token }),
      setBackendUrl: (backendUrl) => set({ backendUrl }),
      setUser: (user) => set({ user }),
      setTheme: (theme) => set({ theme }),
      setAccentIndex: (accentIndex) => set({ accentIndex }),
      // Cached thumbnails go with the session that fetched them. They are
      // keyed on the path with the token stripped, so leaving them behind
      // means the next person to open the app on this device still sees the
      // gallery — logged out, but looking at the photos.
      //
      // Here rather than on the Settings button because logging out also
      // happens on its own, whenever a request comes back 401.
      logout: () => {
        dropCachedThumbs()
        set({ token: null, user: null })
      },
    }),
    {
      name: 'easyclean-session',
      version: 2,
      migrate: (persisted, version) => {
        const state = persisted as SessionState

        // v0 persisted the hardcoded localhost default into every browser that
        // ever opened the app. Clear exactly that value so those clients fall
        // back to same-origin; a deliberately chosen URL is left alone.
        if (version === 0 && state?.backendUrl === 'http://localhost:8080') {
          return { ...state, backendUrl: SAME_ORIGIN }
        }
        return state
      },
      // Re-checked on every rehydrate, not only on a version bump. The address
      // stored here is where the passcode goes, and until v2 anything that
      // parsed as a string could be written to it — so a value already sitting
      // in a browser has never been validated by the rule that now guards the
      // input. Falling back to same-origin is the safe answer: it is where the
      // app was actually loaded from.
      onRehydrateStorage: () => (state) => {
        if (!state || state.backendUrl === SAME_ORIGIN) return
        const checked = normalizeBackendUrl(state.backendUrl)
        if ('error' in checked) state.backendUrl = SAME_ORIGIN
        else state.backendUrl = checked.url
      },
    }
  )
)

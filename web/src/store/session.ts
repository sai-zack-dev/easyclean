import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type Theme = 'dark' | 'light' | 'system'

export interface SessionUser {
  username: string
  role?: string
}

/**
 * Empty string = "same origin as this page", i.e. relative `/api/...` calls.
 *
 * That is the normal case: the Rust server serves this bundle, so whatever host
 * the browser loaded it from is also the API host. A hardcoded default like
 * `http://localhost:8080` breaks the moment the app is opened from a phone —
 * localhost is then the *phone*, not the server — and it needs a CORS opening
 * the server doesn't grant by default. Set a value only to point the UI at a
 * different backend (split dev setup).
 */
const SAME_ORIGIN = ''

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
      logout: () => set({ token: null, user: null }),
    }),
    {
      name: 'easyclean-session',
      version: 1,
      // v0 persisted the hardcoded localhost default into every browser that
      // ever opened the app. Clear exactly that value so those clients fall
      // back to same-origin; a deliberately chosen URL is left alone.
      migrate: (persisted, version) => {
        const state = persisted as SessionState
        if (version === 0 && state?.backendUrl === 'http://localhost:8080') {
          return { ...state, backendUrl: SAME_ORIGIN }
        }
        return state
      },
    }
  )
)

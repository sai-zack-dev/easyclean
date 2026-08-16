import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type Theme = 'dark' | 'light' | 'system'

export interface SessionUser {
  username: string
  role?: string
}

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
      backendUrl: 'http://localhost:8080',
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
    { name: 'easyclean-session' }
  )
)

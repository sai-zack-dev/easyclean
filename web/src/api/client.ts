import { useSession } from '../store/session'

export interface MediaFile {
  id: number
  filename: string
  path: string
  rel_path: string
  size_bytes: number
  media_type: string
  /** Unix seconds. */
  created_at: number
  is_blurry: boolean
  is_favorite: boolean
  blur_score: number | null
  group_id: number | null
  status: 'pending' | 'keep' | 'trashed'
  thumb_url: string
  raw_url: string
}

export interface Album {
  id: number
  name: string
  category_type: string
  file_count: number
  cover_file_id: number | null
  created_at: number
}

export interface StatsSummary {
  total: number
  pending: number
  keep: number
  trashed: number
  blurry: number
  total_size: number
  trashed_size: number
}

export interface StreakStats {
  current_streak: number
  total_swipes: number
  /** Swipe counts for the last 7 days; index 0 = 7 days ago, 6 = today. */
  last_7_days: number[]
}

export interface TimelineMonth {
  month: string
  sort_key: string
  total: number
  reviewed: number
}

export interface BatchResult {
  files: MediaFile[]
  total: number
}

export interface ScanStatus {
  status: string
  scanned: number
  total: number
  message?: string
}

function getBase() {
  return useSession.getState().backendUrl
}

function authQuery() {
  return encodeURIComponent(useSession.getState().token ?? '')
}

function getHeaders(): Record<string, string> {
  const token = useSession.getState().token
  const h: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) h['Authorization'] = `Bearer ${token}`
  return h
}

/**
 * A failed API call. Carries the HTTP status so a caller can choose wording
 * that fits its screen — the same 401 means "wrong passcode" on the login
 * form and "session expired" everywhere else.
 */
export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

/** Errors render as `{ "error": "..." }`; unwrap that so raw JSON never
 *  reaches the UI. Falls back to the body, then the status text. */
async function errorMessage(res: Response): Promise<string> {
  const raw = await res.text().catch(() => '')
  if (!raw) return res.statusText
  try {
    const parsed = JSON.parse(raw) as { error?: string }
    return parsed.error || raw
  } catch {
    return raw
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const base = getBase()
  const res = await fetch(`${base}${path}`, {
    method,
    headers: getHeaders(),
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (res.status === 401) {
    useSession.getState().logout()
    throw new ApiError(401, await errorMessage(res))
  }
  if (!res.ok) {
    throw new ApiError(res.status, await errorMessage(res))
  }
  return res.json() as Promise<T>
}

export const api = {
  get: <T>(path: string) => request<T>('GET', path),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
  delete: <T>(path: string) => request<T>('DELETE', path),

  login: (username: string, passcode: string) =>
    request<{ access_token: string; token_type: string; user: { username: string; role: string } }>(
      'POST',
      '/api/auth/login',
      { username, passcode },
    ),

  me: () => request<{ username: string; role: string }>('GET', '/api/auth/me'),

  scan: () => request<{ job_id: string }>('POST', '/api/scan'),

  mediaBatch: (params: { mode?: string; month?: string; size?: number }) => {
    const q = new URLSearchParams()
    if (params.mode) q.set('mode', params.mode)
    if (params.month) q.set('month', params.month)
    if (params.size) q.set('size', String(params.size))
    return request<BatchResult>('GET', `/api/media/batch?${q}`)
  },

  swipe: (file_id: number, action: 'keep' | 'delete') =>
    request<{ id: number; status: string }>('POST', '/api/media/swipe', { file_id, action }),

  /** Put a swiped file back in the deck and drop it from the streak log. */
  unswipe: (file_id: number) =>
    request<{ id: number; status: string }>('POST', '/api/media/swipe', {
      file_id,
      action: 'undo',
    }),

  sessionFinish: () =>
    request<{ moved: number; trashed: number; categorized: number }>(
      'POST',
      '/api/media/session/finish',
    ),

  // `<img>` can't set an Authorization header, so these carry the JWT in the
  // query string — the server's AuthUser extractor accepts either.
  thumbUrl: (id: number) => `${getBase()}/api/media/${id}/thumb?token=${authQuery()}`,
  rawUrl: (id: number) => `${getBase()}/api/media/${id}/raw?token=${authQuery()}`,

  favorite: (id: number) =>
    request<{ id: number; is_favorite: boolean }>('POST', `/api/media/${id}/favorite`),

  albums: () => request<{ albums: Album[] }>('GET', '/api/albums').then((r) => r.albums ?? []),
  albumFiles: (id: number, page = 0, size = 50) =>
    request<BatchResult>('GET', `/api/albums/${id}/files?page=${page}&size=${size}`),
  albumsRefresh: () => request<void>('POST', '/api/albums/refresh'),

  statsSummary: () => request<StatsSummary>('GET', '/api/stats/summary'),
  statsTimeline: () =>
    request<{ months: TimelineMonth[] }>('GET', '/api/stats/timeline').then((r) => r.months ?? []),
  statsStreak: () => request<StreakStats>('GET', '/api/stats/streak'),

  trash: () => request<{ files: MediaFile[] }>('GET', '/api/trash').then((r) => r.files ?? []),
  trashRestore: (id: number) => request<void>('POST', `/api/trash/restore/${id}`),
  trashPurge: (id: number) => request<void>('DELETE', `/api/trash/purge/${id}`),
  trashPurgeAll: () => request<void>('DELETE', '/api/trash/purge-all'),

  scanStatusStream: () => {
    const base = getBase()
    const token = useSession.getState().token
    return new EventSource(`${base}/api/scan/status?token=${token ?? ''}`)
  },
}

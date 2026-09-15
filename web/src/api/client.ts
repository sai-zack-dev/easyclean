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
  /** Duplicate-cluster id — a hash string, shared by every copy in the group. */
  group_id: string | null
  status: 'pending' | 'keep' | 'trashed'
  /** Unix seconds the file was binned; null unless `status` is `trashed`. */
  trashed_at: number | null
  thumb_url: string
  preview_url: string
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

/** One bar of the Settings "Storage Saved" chart. */
export interface StorageDay {
  /** Unix seconds at the start of the day. */
  day: number
  bytes: number
  files: number
}

export interface StorageSeries {
  days: StorageDay[]
  total_bytes: number
  total_files: number
}

/** One photo from a search, with its similarity score when semantic. */
export interface SearchHit extends MediaFile {
  /** Cosine similarity, present only on semantic matches. */
  score?: number
}

export interface SearchResult {
  /** Filename / album-path matches — exact, and always available. */
  name_matches: SearchHit[]
  /** Ranked by what the photo looks like. Empty until an index exists. */
  semantic_matches: SearchHit[]
  /** False when no index has been built, so the UI can offer to build one. */
  semantic_available: boolean
  /** False when nothing stood out — results are guesses, and labelled as such. */
  semantic_confident: boolean
}

export interface SearchStatus {
  model_ready: boolean
  download_bytes: number
  indexed: number
  indexable: number
  running: boolean
  /** "download" | "embed" | "" */
  phase: string
  done: number
  total: number
  error: string | null
}

export interface Settings {
  /** pHash Hamming distance: higher catches more (looser) duplicates. */
  duplicate_distance: number
  duplicate_distance_max: number
  /** Days a file survives in the trash — drives the countdown on that screen. */
  trash_retention_days: number
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

/** One cluster of near-identical photos, reviewed together rather than one by one. */
export interface DuplicateGroup {
  group_id: string
  files: MediaFile[]
}

export interface DuplicateGroups {
  groups: DuplicateGroup[]
  total_groups: number
  total_files: number
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

  /** Liveness plus the build the server is running — Setting compares it with ours. */
  health: () => request<{ status: string; version?: string }>('GET', '/api/health'),

  scan: () => request<{ job_id: string }>('POST', '/api/scan'),

  /**
   * `mode`: timeline | onthisday | recents | blurs | duplicates (review decks,
   * pending only) · library | original | cleaned (Gallery tabs). `page` is
   * 1-based.
   *
   * `q` and `type` are applied by the database across the whole library, not
   * by the browser across the pages it happens to have loaded.
   */
  mediaBatch: (params: {
    mode?: string
    month?: string
    size?: number
    page?: number
    q?: string
    type?: 'image' | 'video'
  }) => {
    const p = new URLSearchParams()
    if (params.mode) p.set('mode', params.mode)
    if (params.month) p.set('month', params.month)
    if (params.size) p.set('size', String(params.size))
    if (params.page) p.set('page', String(params.page))
    if (params.q?.trim()) p.set('q', params.q.trim())
    if (params.type) p.set('type', params.type)
    return request<BatchResult>('GET', `/api/media/batch?${p}`)
  },

  /** Whole duplicate clusters, for the side-by-side compare screen. */
  duplicateGroups: () => request<DuplicateGroups>('GET', '/api/media/duplicates'),

  /**
   * Settle one cluster in a single call: everything selected goes one way and
   * the rest the other, so a group is never left half-decided. Pass `undo`
   * alone to put a whole group back.
   */
  resolveDuplicates: (body: { keep?: number[]; delete?: number[]; undo?: number[] }) =>
    request<{ kept?: number; deleted?: number; restored?: number }>(
      'POST',
      '/api/media/duplicates/resolve',
      body,
    ),

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
  /** Full-screen-quality render — use wherever the user actually judges a photo. */
  previewUrl: (id: number) => `${getBase()}/api/media/${id}/preview?token=${authQuery()}`,
  rawUrl: (id: number) => `${getBase()}/api/media/${id}/raw?token=${authQuery()}`,

  /** Bin a file immediately (Gallery / viewer), rather than marking a swipe. */
  trashOne: (id: number) =>
    request<{ id: number; status: string; trashed_at: number }>('POST', `/api/media/${id}/trash`),

  favorite: (id: number) =>
    request<{ id: number; is_favorite: boolean }>('POST', `/api/media/${id}/favorite`),

  albums: () => request<{ albums: Album[] }>('GET', '/api/albums').then((r) => r.albums ?? []),
  /** Which albums one file is filed under — the viewer's Details panel. */
  fileAlbums: (id: number) =>
    request<{ albums: Album[] }>('GET', `/api/media/${id}/albums`).then((r) => r.albums ?? []),
  /** `page` is 1-based, matching the server's offset maths. */
  albumFiles: (id: number, page = 1, size = 50) =>
    request<BatchResult>('GET', `/api/albums/${id}/files?page=${page}&size=${size}`),
  albumsRefresh: () => request<void>('POST', '/api/albums/refresh'),
  /** File a photo under an existing album, or a new one by name. */
  albumAdd: (body: { file_id: number; album_id?: number; name?: string }) =>
    request<{ album_id: number; file_id: number }>('POST', '/api/albums/add', body),

  statsSummary: () => request<StatsSummary>('GET', '/api/stats/summary'),
  statsTimeline: () =>
    request<{ months: TimelineMonth[] }>('GET', '/api/stats/timeline').then((r) => r.months ?? []),
  statsStreak: () => request<StreakStats>('GET', '/api/stats/streak'),
  statsStorage: (days = 30) =>
    request<StorageSeries>('GET', `/api/stats/storage?days=${days}`),

  /** Merged search: exact name matches, then photos that look like the query. */
  search: (params: { q: string; mode?: string; type?: 'image' | 'video' }) => {
    const p = new URLSearchParams({ q: params.q })
    if (params.mode) p.set('mode', params.mode)
    if (params.type) p.set('type', params.type)
    return request<SearchResult>('GET', `/api/search?${p}`)
  },
  searchStatus: () => request<SearchStatus>('GET', '/api/search/status'),
  searchIndex: () => request<{ started: boolean; reason?: string }>('POST', '/api/search/index'),

  settings: () => request<Settings>('GET', '/api/settings'),
  saveSettings: (body: { duplicate_distance?: number }) =>
    request<{ duplicate_distance: number; duplicate_groups: number | null }>(
      'POST',
      '/api/settings',
      body,
    ),

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

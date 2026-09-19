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

/**
 * A folder inside the vault. Not an `Album`: those are listed without a PIN,
 * and a folder's name is as private as what is in it.
 */
export interface VaultFolder {
  id: number
  name: string
  created_at: number
  file_count: number
  cover_file_id: number | null
}

/** Which part of the vault a listing covers: everything, no folder, or one. */
export type VaultScope = 'all' | 'unfiled' | number

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
  /**
   * How many photos cleared the relevance filter, before the server's cap.
   * Larger than `semantic_matches.length` only when a query matched more than
   * one page's worth, which is how the header can say "300 of 1,240" instead
   * of implying the cap is the whole answer.
   */
  semantic_total?: number
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

/**
 * The read-only token that image and stream URLs carry.
 *
 * Held here rather than in the session store on purpose. It is short-lived and
 * rotates, and every gallery cell calls `thumbUrl()` during render — putting it
 * in the store would re-render the whole grid each time it refreshed, for a
 * value no component actually displays. Same reasoning as `vaultToken` below.
 *
 * Not persisted either: it is cheap to re-mint from the login on the next
 * launch, and one fewer credential written to disk.
 */
let mediaToken: string | null = null
let refreshing: Promise<boolean> | null = null
let refreshTimer: ReturnType<typeof setTimeout> | null = null

/**
 * Make sure a media token is in hand, minting one if not.
 *
 * Concurrent callers share the one in-flight request — on a cold start the
 * gallery, the stats row and the scan stream all want it at once, and three
 * tokens would be two too many.
 */
export function ensureMediaToken(): Promise<boolean> {
  if (mediaToken) return Promise.resolve(true)
  if (refreshing) return refreshing
  refreshing = request<{ media_token: string; expires_in: number }>(
    'POST',
    '/api/auth/media-token',
  )
    .then((r) => {
      mediaToken = r.media_token
      // Re-minted at four fifths of its life, so a URL built a moment before
      // the swap is still valid when the browser actually fetches it.
      if (refreshTimer) clearTimeout(refreshTimer)
      refreshTimer = setTimeout(
        () => {
          mediaToken = null
          void ensureMediaToken()
        },
        Math.max(30, r.expires_in * 0.8) * 1000,
      )
      return true
    })
    .catch(() => {
      // Leave it null and say so: better that the caller retries than that
      // every URL on the page is built around a token we know is dead.
      mediaToken = null
      return false
    })
    .finally(() => {
      refreshing = null
    })
  return refreshing
}

/** Drop the media token — on logout, with the session it belonged to. */
export function clearMediaToken() {
  mediaToken = null
  if (refreshTimer) clearTimeout(refreshTimer)
  refreshTimer = null
}

function mediaQuery() {
  return encodeURIComponent(mediaToken ?? '')
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

/**
 * The live vault token.
 *
 * Module state, never localStorage. A token written to disk would outlive the
 * app being closed, which is the one thing a 15-minute unlock exists to
 * prevent — losing it on reload is the feature, not a bug.
 */
let vaultToken: string | null = null

export function setVaultToken(t: string | null) {
  vaultToken = t
}

export function hasVaultToken() {
  return vaultToken !== null
}

/**
 * A vault call.
 *
 * Deliberately not `request`: that reads every 401 as an expired session and
 * logs you out, and a mistyped PIN answers 401. Being thrown back to the login
 * screen for a typo would be absurd.
 */
async function vaultRequest<T>(method: string, path: string, body?: unknown): Promise<T> {
  const h = getHeaders()
  if (vaultToken) h['X-Vault-Token'] = vaultToken
  const res = await fetch(`${getBase()}${path}`, {
    method,
    headers: h,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) throw new ApiError(res.status, await errorMessage(res))
  return res.json() as Promise<T>
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

  /**
   * The hidden vault. Reached by tapping the Settings profile chip three
   * times; nothing else in the UI refers to it.
   *
   * Image URLs carry the token in the query string for the same reason the
   * ordinary ones do — `<img src>` cannot set a header.
   */
  vault: {
    status: () => vaultRequest<{ configured: boolean }>('GET', '/api/vault/status'),
    setPin: (pin: string, current_pin?: string) =>
      vaultRequest<{ configured: boolean }>('POST', '/api/vault/pin', { pin, current_pin }),
    unlock: (pin: string) =>
      vaultRequest<{ vault_token: string; expires_in: number }>('POST', '/api/vault/unlock', {
        pin,
      }),
    files: (scope: VaultScope = 'all') =>
      vaultRequest<{ files: MediaFile[]; total: number }>(
        'GET',
        scope === 'all' ? '/api/vault/files' : `/api/vault/files?folder=${scope}`,
      ),
    /** Hide `ids`, straight into `folder_id` when one is on screen. */
    add: (ids: number[], folder_id?: number) =>
      vaultRequest<{ hidden: number }>('POST', '/api/vault/add', { ids, folder_id }),
    folders: () =>
      vaultRequest<{ folders: VaultFolder[]; unfiled: number; total: number }>(
        'GET',
        '/api/vault/folders',
      ),
    createFolder: (name: string) =>
      vaultRequest<{ id: number; name: string }>('POST', '/api/vault/folders', { name }),
    renameFolder: (id: number, name: string) =>
      vaultRequest<{ id: number; name: string }>('PATCH', `/api/vault/folders/${id}`, { name }),
    /** The folder only — its photos stay hidden, in Unfiled. */
    deleteFolder: (id: number) =>
      vaultRequest<{ deleted: number; unfiled: number }>('DELETE', `/api/vault/folders/${id}`),
    /** File hidden photos into a folder; `null` means Unfiled. */
    move: (ids: number[], folder_id: number | null) =>
      vaultRequest<{ moved: number }>('POST', '/api/vault/move', { ids, folder_id }),
    restore: (ids: number[]) =>
      vaultRequest<{ restored: number }>('POST', '/api/vault/restore', { ids }),
    /** Permanent — hidden files cannot go to the trash, which needs no PIN. */
    remove: (ids: number[]) =>
      vaultRequest<{ purged: number }>('POST', '/api/vault/delete', { ids }),
    thumbUrl: (id: number) =>
      `${getBase()}/api/vault/${id}/thumb?vault_token=${encodeURIComponent(vaultToken ?? '')}`,
    /**
     * The hidden original, fetched with the vault token in a *header*.
     *
     * There is deliberately no `rawUrl` counterpart. Opening one meant
     * navigating to a URL with the vault token in it, which wrote that token
     * into the address bar and the browsing history — on the one screen in the
     * app whose entire purpose is that nothing about it is on show. A blob URL
     * is scoped to this document and carries no credential.
     */
    rawBlob: async (id: number): Promise<Blob> => {
      const h: Record<string, string> = {}
      const token = useSession.getState().token
      if (token) h['Authorization'] = `Bearer ${token}`
      if (vaultToken) h['X-Vault-Token'] = vaultToken
      const res = await fetch(`${getBase()}/api/vault/${id}/raw`, { headers: h })
      if (!res.ok) throw new ApiError(res.status, await errorMessage(res))
      return res.blob()
    },
  },

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

  // `<img>` can't set an Authorization header, so these carry a token in the
  // query string — the *media* token, not the login. It only reads, and only
  // for an hour, so a URL that ends up in an access log or a proxy is not the
  // account. The server refuses a login token here outright.
  thumbUrl: (id: number) => `${getBase()}/api/media/${id}/thumb?token=${mediaQuery()}`,
  /** Full-screen-quality render — use wherever the user actually judges a photo. */
  previewUrl: (id: number) => `${getBase()}/api/media/${id}/preview?token=${mediaQuery()}`,
  rawUrl: (id: number) => `${getBase()}/api/media/${id}/raw?token=${mediaQuery()}`,

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
  /** Make an empty album — the "+" in the Albums tab. */
  albumCreate: (name: string) =>
    request<{ album_id: number; name: string }>('POST', '/api/albums', { name }),
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

  // `EventSource` can't set headers either, so progress rides the media token
  // as well. Read-only, which is all a progress stream ever needed.
  scanStatusStream: () => new EventSource(`${getBase()}/api/scan/status?token=${mediaQuery()}`),
}

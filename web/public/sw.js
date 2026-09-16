/**
 * EasyClean service worker.
 *
 * Hand-written rather than generated: the caching rules here are specific to
 * this app (thumbnails are worth caching hard, API state never is), and a
 * build-time precache plugin would add a dependency without adding accuracy.
 *
 * Strategies
 *   navigations      → network-first, fall back to the cached app shell
 *   /assets/*        → cache-first (Vite content-hashes these, so they're immutable)
 *   /api/media/:id/thumb → cache-first, capped — the gallery re-requests the same
 *                          thumbs constantly and they never change for an id
 *   everything else under /api → network-only (auth + mutable state; caching it
 *                          would show stale counts or leak one user's data)
 */

// Bumped to v4 to evict what v3 may already be holding: until the fetch
// handler below put the API check ahead of the navigation branch, opening an
// original in a new tab wrote the photo itself into the shell cache under
// `/index.html`. Renaming the caches is what actually deletes those copies from
// a browser that already has them — the `activate` handler drops every cache
// whose name is not on the current VERSION.
const VERSION = 'v4'
const SHELL_CACHE = `easyclean-shell-${VERSION}`
const ASSET_CACHE = `easyclean-assets-${VERSION}`
const THUMB_CACHE = `easyclean-thumbs-${VERSION}`

/** Max thumbnails held offline. Trimmed oldest-first (insertion order). */
const THUMB_LIMIT = 600

const SHELL_URLS = ['/', '/index.html', '/manifest.webmanifest', '/icon-192.png']

/*
 * No `skipWaiting()` here on purpose.
 *
 * Taking over immediately swapped the worker under a page that was still
 * running the previous build's JavaScript, and left no "waiting" worker for the
 * app to notice — so an update was invisible and, in a standalone PWA that is
 * resumed rather than reloaded, effectively never arrived. Now a new worker
 * waits, the page sees it and offers to reload, and `skipWaiting` happens on
 * that tap (see the `message` handler below).
 */
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(SHELL_CACHE)
      // Best-effort: a missing shell entry must not fail the whole install.
      .then((cache) => Promise.allSettled(SHELL_URLS.map((u) => cache.add(u)))),
  )
})

self.addEventListener('activate', (event) => {
  const keep = new Set([SHELL_CACHE, ASSET_CACHE, THUMB_CACHE])
  event.waitUntil(
    caches
      .keys()
      .then((names) => Promise.all(names.filter((n) => !keep.has(n)).map((n) => caches.delete(n))))
      .then(() => self.clients.claim()),
  )
})

/** Keep a cache from growing without bound. */
async function trim(cacheName, limit) {
  const cache = await caches.open(cacheName)
  const keys = await cache.keys()
  if (keys.length <= limit) return
  await Promise.all(keys.slice(0, keys.length - limit).map((k) => cache.delete(k)))
}

/**
 * @param request       the request to fetch on a miss
 * @param cacheName     cache to read/write
 * @param limit         optional entry cap
 * @param key           cache key to use instead of `request` (see thumbnails)
 */
async function cacheFirst(request, cacheName, limit, key = request) {
  const cache = await caches.open(cacheName)
  const hit = await cache.match(key)
  if (hit) return hit

  const response = await fetch(request)
  // Only successes are cached — a 404 for a thumbnail that detection hasn't
  // written yet must not become permanent.
  if (response.ok) {
    await cache.put(key, response.clone())
    if (limit) trim(cacheName, limit)
  }
  return response
}

/**
 * Thumbnails are fetched by `<img src>`, which can't send an Authorization
 * header, so the token rides along as `?token=…`. Keying the cache on the full
 * URL would both persist a JWT in Cache Storage and miss on every new token —
 * so key on the path alone. Safe because `/api/media/:id/thumb` is immutable
 * for a given id.
 *
 * Consequence to be aware of: a cache hit is served without re-checking the
 * token, so a logged-out session can still see thumbnails this same browser
 * already downloaded — no different from the browser's own image cache, and
 * scoped to the client that was already authorized. Full-size originals
 * (`/raw`) are never cached.
 */
function thumbCacheKey(url) {
  return new Request(url.origin + url.pathname)
}

/**
 * Network-first with an offline fallback to the cached shell.
 *
 * A successful fetch also refreshes the cached copy, so the offline shell is
 * the last build seen rather than the one installed months ago — the stale
 * copy was part of why a deploy never seemed to land.
 */
async function navigationHandler(request) {
  try {
    const fresh = await fetch(request)
    if (fresh.ok) {
      const copy = fresh.clone()
      caches.open(SHELL_CACHE).then((c) => c.put('/index.html', copy)).catch(() => {})
    }
    return fresh
  } catch {
    const cache = await caches.open(SHELL_CACHE)
    const shell = (await cache.match('/index.html')) || (await cache.match('/'))
    return (
      shell ||
      new Response('<h1>EasyClean is offline</h1>', {
        status: 503,
        headers: { 'content-type': 'text/html; charset=utf-8' },
      })
    )
  }
}

self.addEventListener('fetch', (event) => {
  const { request } = event
  if (request.method !== 'GET') return

  const url = new URL(request.url)
  if (url.origin !== self.location.origin) return

  // The API is decided BEFORE the navigation branch, and the order is the
  // whole point. Opening an original in a new tab — Share, and the vault's own
  // "open" — is a top-level navigation to `/api/**/raw`, so with the navigate
  // check first those bytes reached `navigationHandler`, which stores whatever
  // it fetched under the fixed key `/index.html`. A full-resolution photo, the
  // vault's included, ended up in Cache Storage readable with no token at all
  // — the one copy the server takes care to delete everywhere else.
  if (/^\/api\/media\/\d+\/thumb$/.test(url.pathname)) {
    event.respondWith(cacheFirst(request, THUMB_CACHE, THUMB_LIMIT, thumbCacheKey(url)))
    return
  }

  // Auth, stats, batches, raw originals: always live, never cached.
  if (url.pathname.startsWith('/api/')) return

  if (request.mode === 'navigate') {
    event.respondWith(navigationHandler(request))
    return
  }

  if (url.pathname.startsWith('/assets/')) {
    event.respondWith(cacheFirst(request, ASSET_CACHE))
  }
})

/**
 * Lets the page trigger an immediate update after a deploy.
 *
 * The page posts this when the user taps Update; the worker activates, claims
 * the open clients, and the page reloads on `controllerchange`.
 */
/**
 * Forget the cached thumbnails for these ids.
 *
 * A cached thumbnail is a readable copy of the photo, and it is keyed on the
 * path alone (see `thumbCacheKey`) — deliberately, so a new token still hits,
 * but it also means the entry outlives the session that fetched it. The case
 * that matters is a photo moved into the vault: the server drops its preview,
 * moves its thumbnail into `.vault/.thumbs/` and deletes its embedding, and
 * none of that reaches a copy already sitting in this browser. So the page
 * tells us, and we delete it here.
 */
async function evictThumbs(ids) {
  const cache = await caches.open(THUMB_CACHE)
  await Promise.all(
    ids.map((id) => cache.delete(new Request(`${self.location.origin}/api/media/${id}/thumb`))),
  )
}

self.addEventListener('message', (event) => {
  const msg = event.data
  if (msg === 'skip-waiting') {
    self.skipWaiting()
    return
  }
  if (!msg || typeof msg !== 'object') return

  if (msg.type === 'evict-thumbs' && Array.isArray(msg.ids)) {
    event.waitUntil(evictThumbs(msg.ids.filter((id) => Number.isInteger(id))))
    return
  }
  // Logging out leaves the shell and the hashed assets alone — they are the
  // same for everyone and dropping them would cost an offline launch. The
  // thumbnails are the only cache holding anything of the user's.
  if (msg.type === 'drop-thumbs') {
    event.waitUntil(caches.delete(THUMB_CACHE))
  }
})

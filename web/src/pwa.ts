/**
 * Service-worker registration and the update path.
 *
 * Only in production builds: in `vite dev` a cached shell would serve stale
 * modules and make HMR lie to you. Browsers restrict service workers to secure
 * contexts, which for LAN use means `localhost` — over plain HTTP at
 * `http://192.168.x.x:8080` the app still runs, it just isn't installable.
 * That's the documented trade for LAN-only HTTP; a tunnel (Tailscale /
 * Cloudflare) gives HTTPS and full PWA install when we get there.
 *
 * Updating is the hard half. An installed PWA is resumed, not reloaded, so
 * without asking it will sit on the build it first saw — which is why
 * reinstalling the app used to be the only way to pick up a deploy. Three
 * things fix that, and all three are needed:
 *
 *   1. The server marks the shell `no-cache`, so a check actually reaches it.
 *   2. We call `registration.update()` on launch, on resume, and hourly.
 *   3. A waiting worker surfaces in the UI as an Update button, which is what
 *      finally swaps the build — `skipWaiting()` on its own would swap the
 *      worker under a page still running the old JavaScript.
 */

/** How often a long-lived tab re-checks for a new build. */
const UPDATE_INTERVAL_MS = 60 * 60 * 1000

/** Set once a reload is already in flight, so a double tap can't loop. */
let reloading = false

type UpdateListener = (available: boolean) => void

const listeners = new Set<UpdateListener>()
let registration: ServiceWorkerRegistration | null = null
let updateReady = false

function announce(available: boolean) {
  updateReady = available
  listeners.forEach((fn) => fn(available))
}

/** Subscribe to "a new build is waiting"; returns an unsubscribe. */
export function onUpdateAvailable(fn: UpdateListener): () => void {
  listeners.add(fn)
  // A worker may already have been waiting before this component mounted.
  if (updateReady) fn(true)
  return () => listeners.delete(fn)
}

/**
 * Ask the server whether there's a newer build.
 *
 * Resolves to `true` when one is now waiting. Safe to call when there is no
 * service worker at all (plain-HTTP LAN), where it simply reports `false`.
 */
export async function checkForUpdate(): Promise<boolean> {
  if (!registration) return updateReady
  try {
    await registration.update()
  } catch {
    /* offline, or the server is down — nothing to report */
  }
  if (registration.waiting) announce(true)
  return updateReady
}

/**
 * Apply the waiting build and reload.
 *
 * The reload is driven by `controllerchange` rather than fired straight away,
 * so the page comes back under the *new* worker instead of racing it and
 * re-fetching the old assets.
 */
export function applyUpdate() {
  const waiting = registration?.waiting
  if (!waiting) {
    // No worker to swap (or none at all) — a plain reload is still the right
    // answer for "give me the latest".
    window.location.reload()
    return
  }
  waiting.postMessage('skip-waiting')
}

/** Watch one registration for a worker arriving and reaching `installed`. */
function watch(reg: ServiceWorkerRegistration) {
  registration = reg
  if (reg.waiting && navigator.serviceWorker.controller) announce(true)

  reg.addEventListener('updatefound', () => {
    const installing = reg.installing
    if (!installing) return
    installing.addEventListener('statechange', () => {
      // A controller already present means this is an update, not the very
      // first install — the first install has nothing to announce.
      if (installing.state === 'installed' && navigator.serviceWorker.controller) {
        announce(true)
      }
    })
  })
}

/**
 * Ask the worker to forget cached copies of these photos.
 *
 * Call it whenever a photo stops being freely readable — moving one into the
 * vault is the case that matters. The server is thorough about deleting its own
 * derived copies, but a thumbnail already in this browser's Cache Storage is
 * out of its reach and keyed on the path alone, so it would go on being served
 * with no token at all.
 *
 * Silent when there is no worker (dev, plain-HTTP LAN): nothing was cached
 * there either, so there is nothing to forget.
 */
export function evictCachedThumbs(ids: number[]) {
  if (!ids.length) return
  navigator.serviceWorker?.controller?.postMessage({ type: 'evict-thumbs', ids })
}

/** Drop every cached thumbnail — on logout, when the session that fetched them ends. */
export function dropCachedThumbs() {
  navigator.serviceWorker?.controller?.postMessage({ type: 'drop-thumbs' })
}

export function registerServiceWorker() {
  if (!import.meta.env.PROD) return
  if (!('serviceWorker' in navigator)) return
  if (!window.isSecureContext) {
    console.info('[pwa] insecure context (plain-HTTP LAN) — service worker skipped')
    return
  }

  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (reloading) return
    reloading = true
    window.location.reload()
  })

  window.addEventListener('load', () => {
    navigator.serviceWorker
      .register('/sw.js')
      .then((reg) => {
        watch(reg)
        // Launch, resume, and a slow background beat. The resume check is the
        // one that matters on a phone: reopening the installed app is the only
        // "navigation" it ever gets.
        void checkForUpdate()
        document.addEventListener('visibilitychange', () => {
          if (document.visibilityState === 'visible') void checkForUpdate()
        })
        setInterval(() => void checkForUpdate(), UPDATE_INTERVAL_MS)
      })
      .catch((err) => {
        console.warn('[pwa] service worker registration failed', err)
      })
  })
}

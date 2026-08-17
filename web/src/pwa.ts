/**
 * Service-worker registration.
 *
 * Only in production builds: in `vite dev` a cached shell would serve stale
 * modules and make HMR lie to you. Browsers restrict service workers to secure
 * contexts, which for LAN use means `localhost` — over plain HTTP at
 * `http://192.168.x.x:8080` the app still runs, it just isn't installable.
 * That's the documented trade for LAN-only HTTP; a tunnel (Tailscale /
 * Cloudflare) gives HTTPS and full PWA install when we get there.
 */
export function registerServiceWorker() {
  if (!import.meta.env.PROD) return
  if (!('serviceWorker' in navigator)) return
  if (!window.isSecureContext) {
    console.info('[pwa] insecure context (plain-HTTP LAN) — service worker skipped')
    return
  }

  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch((err) => {
      console.warn('[pwa] service worker registration failed', err)
    })
  })
}

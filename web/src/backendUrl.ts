/**
 * Where the API lives, and what is allowed to be written there.
 *
 * Its own module because two very different callers need the same rule:
 * the setup screen, which validates what someone types, and the session
 * store, which has to re-check what an older build already persisted.
 */

/** Empty string = same origin as this page. See the session store. */
export const SAME_ORIGIN = ''

/**
 * Reduce what the user typed to a bare `scheme://host[:port]`, or explain why
 * it is not usable.
 *
 * Whatever goes in here is where the passcode is sent, and then the session
 * token with every request after it — so this field is a credential
 * destination, not a preference. The checks are the ones that actually matter
 * for that:
 *
 *   * **http/https only.** `javascript:` and `data:` are inert under `fetch`,
 *     but this string is also concatenated into image URLs, and a base that
 *     isn't a real origin has no business being stored.
 *   * **No embedded credentials.** `http://real.local@evil.example` reads as
 *     "real.local" at a glance and resolves to evil.example. The userinfo form
 *     exists to be misread, and nothing here needs it.
 *   * **Origin only.** A path, query or fragment on a base URL silently changes
 *     what every later request means; drop them rather than carry them.
 *   * **No downgrade.** A page served over HTTPS cannot call plain HTTP anyway
 *     — the browser blocks it as mixed content — so say so here rather than
 *     let it fail as "could not reach the server".
 */
export function normalizeBackendUrl(raw: string): { url: string } | { error: string } {
  const text = raw.trim()
  if (!text) return { error: 'Enter your server address.' }

  let u: URL
  try {
    u = new URL(text)
  } catch {
    return { error: 'That is not a full address. Try http://192.168.1.x:8080' }
  }

  if (u.protocol !== 'http:' && u.protocol !== 'https:') {
    return { error: 'The address must start with http:// or https://' }
  }
  if (u.username || u.password) {
    return { error: 'Remove the username and password from the address.' }
  }
  if (!u.hostname) {
    return { error: 'That address has no server name in it.' }
  }
  if (window.location.protocol === 'https:' && u.protocol === 'http:') {
    return { error: 'This page is secure, so it can only talk to an https:// server.' }
  }
  // `origin` is already scheme + host + port with no trailing slash.
  return { url: u.origin }
}

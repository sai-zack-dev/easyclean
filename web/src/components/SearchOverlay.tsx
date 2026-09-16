import { useEffect, useMemo, useRef, useState } from 'react'
import { IC } from '../icons'
import { useScrollLock } from '../hooks/useScrollLock'

/** Where recent searches live. Per-device convenience, not synced state. */
const RECENT_KEY = 'ec.recent-searches'
const RECENT_MAX = 6

/** Starters for a library nobody has searched yet — all things CLIP is good at. */
const EXAMPLES = ['food', 'beach', 'documents', 'sunset', 'my cat', 'birthday cake']

function readRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === 'string') : []
  } catch {
    // Private windows and blocked site data both throw here.
    return []
  }
}

function pushRecent(q: string) {
  try {
    const next = [q, ...readRecent().filter((r) => r !== q)].slice(0, RECENT_MAX)
    localStorage.setItem(RECENT_KEY, JSON.stringify(next))
  } catch {
    /* nothing to do — the list is a nicety */
  }
}

interface Props {
  /** What's already applied, so reopening shows it rather than an empty box. */
  initial?: string
  onSearch: (q: string) => void
  onClose: () => void
}

/**
 * The search overlay.
 *
 * Replaces the inline field that used to expand inside the Gallery header. That
 * field was about 150px wide on a phone, sat beside the tabs, and pushed the
 * title around as it opened — you could not see what you had typed, and
 * suggestions had nowhere to go. A full sheet gets the keyboard, the query and
 * the suggestions on screen together.
 *
 * Anchored to the top rather than centred, for the same reason the New album
 * box moved: a centred panel with a text field is exactly where the keyboard
 * appears.
 */
export function SearchOverlay({ initial = '', onSearch, onClose }: Props) {
  const [q, setQ] = useState(initial)
  const ref = useRef<HTMLInputElement>(null)
  const recent = useMemo(() => readRecent(), [])
  useScrollLock()

  useEffect(() => {
    ref.current?.focus()
    ref.current?.select()
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const submit = (value: string) => {
    const text = value.trim()
    if (!text) return
    pushRecent(text)
    onSearch(text)
  }

  const suggestions = q.trim()
    ? // Filtering the lists rather than asking the server: these are words you
      // have typed before, and a round trip per keystroke buys nothing.
      [...recent, ...EXAMPLES]
        .filter((s) => s.toLowerCase().includes(q.trim().toLowerCase()) && s !== q.trim())
        .slice(0, 6)
    : recent.length
      ? recent
      : EXAMPLES

  return (
    <div
      className="search-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="search-panel" role="dialog" aria-modal="true" aria-label="Search">
        <div className="search-field">
          <IC.search />
          <input
            ref={ref}
            value={q}
            placeholder="Search your photos…"
            enterKeyHint="search"
            autoCapitalize="none"
            autoCorrect="off"
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submit(q)
            }}
          />
          {q && (
            <button className="search-clear" onClick={() => setQ('')} aria-label="Clear">
              <IC.close />
            </button>
          )}
          <button className="search-go" onClick={() => submit(q)} disabled={!q.trim()}>
            Search
          </button>
        </div>

        <div className="search-sugs">
          <div className="search-sugs-lbl">
            {q.trim() ? 'Suggestions' : recent.length ? 'Recent' : 'Try'}
          </div>
          {suggestions.length === 0 ? (
            <div className="search-none">Press Search to look for &ldquo;{q.trim()}&rdquo;.</div>
          ) : (
            suggestions.map((s) => (
              <button key={s} className="search-sug" onClick={() => submit(s)}>
                <IC.search />
                <span>{s}</span>
              </button>
            ))
          )}
        </div>

        <button className="search-cancel" onClick={onClose}>
          Cancel
        </button>
      </div>
    </div>
  )
}

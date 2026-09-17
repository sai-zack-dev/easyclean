/*
 * A registry of icon components, looked up by name (`IC[icon]`) so screens can
 * take an icon key as a prop. That single named export is what trips
 * react-refresh's "only export components" rule; the cost is one extra HMR
 * reload when this file changes, which is worth the callsite ergonomics.
 */
/* eslint-disable react-refresh/only-export-components */
import { MARK_PATH, MARK_VIEWBOX } from './brand'
import broomUrl from './assets/icons/broom.png'
import galleryUrl from './assets/icons/gallery.png'
import settingsUrl from './assets/icons/settings.png'
import fireUrl from './assets/icons/fire.png'
import trophyUrl from './assets/icons/trophy.png'

/**
 * An icon supplied as artwork rather than as a path.
 *
 * The designed set arrived as Figma exports that wrap a 90x90 white-on-
 * transparent PNG — raster, despite the `.svg` extension. Drawing them with
 * `<img>` would nail them to white, so instead the artwork is used purely as an
 * alpha *mask* over `currentColor`: the glyph then inherits text colour and
 * follows the theme and the accent exactly like the hand-drawn paths do.
 *
 * 90px of source for an 18-22px icon is ~4x density, so it stays crisp on any
 * display. Each file is under Vite's inline limit and ships as a data: URI.
 */
function maskIcon(url: string) {
  return () => <span className="ic-mask" style={{ maskImage: `url(${url})`, WebkitMaskImage: `url(${url})` }} />
}

export const IC = {
  /*
   * The brand mark — the only filled glyph in here; the rest are stroked line
   * icons. Callers colour it with `fill`, same as the others.
   */
  mark: () => (
    <svg viewBox={MARK_VIEWBOX} xmlns="http://www.w3.org/2000/svg">
      <path d={MARK_PATH} />
    </svg>
  ),
  // Designed artwork (see `maskIcon`). The stroked glyph this replaced read as
  // a pen, not a broom.
  broom: maskIcon(broomUrl),
  photo: maskIcon(galleryUrl),
  /*
   * Drawn to sit beside the designed artwork above, which is solid and filled —
   * the old four-square outline was the odd one out once Clean, Gallery and
   * Setting went solid. `ic-solid` opts out of the stroked-icon CSS.
   */
  album: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M7.4 3h13.1A2.5 2.5 0 0123 5.5v9.2a2.5 2.5 0 01-2.5 2.5H7.4a2.5 2.5 0 01-2.5-2.5V5.5A2.5 2.5 0 017.4 3zm11.4 3.6a1.5 1.5 0 100 3 1.5 1.5 0 000-3zM7 14.7h12.3l-3.6-4.3-2.7 3.1-1.9-2-4.1 3.2z" />
      <path d="M2.6 6.3a1 1 0 011 1v9.5c0 .6.4 1 1 1h13a1 1 0 010 2h-13a3 3 0 01-3-3V7.3a1 1 0 011-1z" opacity=".55" />
    </svg>
  ),
  settings: maskIcon(settingsUrl),
  scan: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M3 7V5a2 2 0 012-2h2M17 3h2a2 2 0 012 2v2M21 17v2a2 2 0 01-2 2h-2M7 21H5a2 2 0 01-2-2v-2" />
      <line x1="3" y1="12" x2="21" y2="12" />
    </svg>
  ),
  calendar: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M8 2a1 1 0 011 1v1h6V3a1 1 0 112 0v1h1.5A2.5 2.5 0 0121 6.5V9H3V6.5A2.5 2.5 0 015.5 4H7V3a1 1 0 011-1z" />
      <path d="M3 9h18v10.5a2.5 2.5 0 01-2.5 2.5h-13A2.5 2.5 0 013 19.5V9zm4 3a1.2 1.2 0 100 2.4A1.2 1.2 0 007 13zm5 0a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4zm5 0a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4zM7 17.4a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4zm5 0a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4z" opacity=".85" />
    </svg>
  ),
  clock: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      {/* One path with a hole: `evenodd` knocks the hands out of the disc, so
          the glyph stays a single colour and still reads at 18px. */}
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M12 2a10 10 0 100 20 10 10 0 000-20zM11 6.2h2v4.8h4v2h-6V6.2z"
      />
    </svg>
  ),
  /*
   * Drawn rather than taken from `Duplicate Images.svg`: that export layers two
   * photo glyphs around an opaque #333 card, so as an alpha mask it flattens to
   * a featureless blob, and the baked dark fill would not survive light mode.
   */
  copy: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M4.3 5.1a1 1 0 01.7 1.23L2.9 14.2a1.5 1.5 0 001.06 1.84l8.2 2.2a1 1 0 11-.52 1.93l-8.2-2.2a3.5 3.5 0 01-2.47-4.29l2.1-7.87a1 1 0 011.23-.71z" opacity=".55" />
      <path d="M9.5 2h10A2.5 2.5 0 0122 4.5v8.6a2.5 2.5 0 01-2.5 2.5h-10A2.5 2.5 0 017 13.1V4.5A2.5 2.5 0 019.5 2zm8.4 3.2a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8zM8.9 13.3h11.2l-3.4-4.1-2.5 2.9-1.8-1.9-3.5 3.1z" />
    </svg>
  ),
  blur: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      {/* Filled rings fading outwards — the softness *is* the metaphor. */}
      <circle cx="12" cy="12" r="9.5" opacity=".22" />
      <circle cx="12" cy="12" r="6.2" opacity=".45" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
  chevRight: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="9 18 15 12 9 6" />
    </svg>
  ),
  chevLeft: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="15 18 9 12 15 6" />
    </svg>
  ),
  check: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  ),
  undo: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="1 4 1 10 7 10" />
      <path d="M3.51 15a9 9 0 102.13-9.36L1 10" />
    </svg>
  ),
  fire: maskIcon(fireUrl),
  trophy: maskIcon(trophyUrl),
  server: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <rect x="2" y="2" width="20" height="8" rx="2" />
      <rect x="2" y="14" width="20" height="8" rx="2" />
      <line x1="6" y1="6" x2="6.01" y2="6" />
      <line x1="6" y1="18" x2="6.01" y2="18" />
    </svg>
  ),
  link: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71" />
      <path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71" />
    </svg>
  ),
  folder: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M2 6.5A2.5 2.5 0 014.5 4h4.2a2 2 0 011.66.89L11.6 7h7.9A2.5 2.5 0 0122 9.5V19a2 2 0 01-2 2H4a2 2 0 01-2-2V6.5z" />
    </svg>
  ),
  sliders: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <line x1="4" y1="21" x2="4" y2="14" />
      <line x1="4" y1="10" x2="4" y2="3" />
      <line x1="12" y1="21" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12" y2="3" />
      <line x1="20" y1="21" x2="20" y2="16" />
      <line x1="20" y1="12" x2="20" y2="3" />
      <line x1="1" y1="14" x2="7" y2="14" />
      <line x1="9" y1="8" x2="15" y2="8" />
      <line x1="17" y1="16" x2="23" y2="16" />
    </svg>
  ),
  sync: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="23 4 23 10 17 10" />
      <polyline points="1 20 1 14 7 14" />
      <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
    </svg>
  ),
  /*
   * An artist's palette — the thumb-hole notch is what separates it from a
   * plain circle of dots, which is what the previous drawing collapsed into at
   * the 15px these render at.
   */
  palette: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M12 3a9 9 0 000 18c1.1 0 1.8-.8 1.8-1.7 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.8-1.7 1.7-1.7H16a5 5 0 005-5c0-4-4-7.2-9-7.2z" />
      <circle cx="7.6" cy="11.4" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="10.4" cy="7.6" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="15" cy="8.4" r="1.05" fill="currentColor" stroke="none" />
    </svg>
  ),
  /*
   * Half-filled circle: the standard light/dark mark. Reads at a glance and
   * survives being 15px, which a sun-and-moon pairing does not.
   */
  contrast: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 3a9 9 0 010 18z" fill="currentColor" stroke="none" />
    </svg>
  ),
  globe: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="10" />
      <line x1="2" y1="12" x2="22" y2="12" />
      <path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z" />
    </svg>
  ),
  logout: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4" />
      <polyline points="16 17 21 12 16 7" />
      <line x1="21" y1="12" x2="9" y2="12" />
    </svg>
  ),
  user: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  ),
  close: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <line x1="6" y1="6" x2="18" y2="18" />
      <line x1="18" y1="6" x2="6" y2="18" />
    </svg>
  ),
  plus: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  ),
  search: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  ),
  zap: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
    </svg>
  ),
  imgOff: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <line x1="1" y1="1" x2="23" y2="23" />
      <path d="M21 21H3a2 2 0 01-2-2V5a2 2 0 012-2h3m3.66 0H21a2 2 0 012 2v9.34M8.5 8.5a2 2 0 102.83 2.83" />
      <path d="M9.26 9.26L5 21l6.24-6.24M15 15l3 4" />
    </svg>
  ),
  heart: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M20.8 4.6a5.5 5.5 0 00-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 00-7.8 7.8l1.1 1L12 21.2l7.7-7.7 1.1-1a5.5 5.5 0 000-7.9z" />
    </svg>
  ),
  trash: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" />
      <line x1="10" y1="11" x2="10" y2="17" />
      <line x1="14" y1="11" x2="14" y2="17" />
    </svg>
  ),
  share: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M12 2.6a1 1 0 01.7.29l4.2 4.2a1 1 0 01-1.42 1.42L13 5.99V15a1 1 0 11-2 0V5.99L8.5 8.51A1 1 0 017.1 7.09l4.2-4.2a1 1 0 01.7-.29z" />
      <path d="M4.5 12.4a1 1 0 011 1v5.1a.5.5 0 00.5.5h12a.5.5 0 00.5-.5v-5.1a1 1 0 112 0v5.1a2.5 2.5 0 01-2.5 2.5H6a2.5 2.5 0 01-2.5-2.5v-5.1a1 1 0 011-1z" />
    </svg>
  ),
  info: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M12 2a10 10 0 100 20 10 10 0 000-20zm0 3.6a1.4 1.4 0 110 2.8 1.4 1.4 0 010-2.8zM10.8 10.6h2.4v7.2h-2.4v-7.2z"
      />
    </svg>
  ),
  /* Solid, and the only pair in here that sits on top of a photograph — a
     stroked triangle over a busy frame reads as a scratch rather than a
     control. */
  play: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <path d="M8.4 4.6a1.4 1.4 0 012.1-1.2l9.3 6.4a1.4 1.4 0 010 2.4l-9.3 6.4a1.4 1.4 0 01-2.1-1.2V4.6z" />
    </svg>
  ),
  pause: () => (
    <svg viewBox="0 0 24 24" className="ic-solid" xmlns="http://www.w3.org/2000/svg">
      <rect x="6" y="4" width="4" height="16" rx="1.3" />
      <rect x="14" y="4" width="4" height="16" rx="1.3" />
    </svg>
  ),
  restore: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M3 12a9 9 0 109-9 9 9 0 00-6.36 2.64L3 8" />
      <polyline points="3 3 3 8 8 8" />
    </svg>
  ),
  eye: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7-10-7-10-7z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
  eyeOff: () => (
    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <path d="M9.9 5.2A9.9 9.9 0 0112 5c6.4 0 10 7 10 7a18 18 0 01-2.4 3.4M6.2 6.7A18 18 0 002 12s3.6 7 10 7a9.9 9.9 0 004.2-.9" />
      <path d="M9.9 9.9a3 3 0 004.2 4.2" />
      <line x1="3" y1="3" x2="21" y2="21" />
    </svg>
  ),
}

export type IconName = keyof typeof IC

export function Icon({ n, className, style }: { n: IconName; className?: string; style?: React.CSSProperties }) {
  const El = IC[n]
  if (!El) return null
  return (
    <span className={className} style={style}>
      <El />
    </span>
  )
}

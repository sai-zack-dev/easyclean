/**
 * Accent palette.
 *
 * Index 0 is deliberately `null`, not a colour: "Default" means *follow the
 * theme* — white on dark, black on light — which is what `--accent` already
 * resolves to in `tokens.css`. Pinning index 0 to `#ffffff` (as the prototype
 * did) makes the default invisible the moment someone switches to light mode.
 */
export interface Accent {
  name: string
  /** `null` ⇒ inherit the theme's own `--accent`. */
  color: string | null
}

export const ACCENTS: Accent[] = [
  { name: 'Default', color: null },
  { name: 'Teal', color: '#00c7be' },
  { name: 'Green', color: '#30d158' },
  { name: 'Yellow', color: '#ffd60a' },
  { name: 'Orange', color: '#ff9f0a' },
  { name: 'Blue', color: '#0a84ff' },
  { name: 'Purple', color: '#bf5af2' },
]

/**
 * Push the chosen accent onto the document.
 *
 * A chosen colour overrides `--accent` inline on `<html>`; "Default" clears the
 * override so the theme's own value applies again. `--accent-on` is the text
 * colour that stays readable *on* the accent — light accents (yellow) need dark
 * text, which a single hardcoded value could never get right for both.
 */
export function applyAccent(index: number) {
  const root = document.documentElement
  const accent = ACCENTS[index] ?? ACCENTS[0]
  if (!accent.color) {
    root.style.removeProperty('--accent')
    root.style.removeProperty('--accent-on')
    return
  }
  root.style.setProperty('--accent', accent.color)
  root.style.setProperty('--accent-on', readableOn(accent.color) )
}

/** Black or white, whichever has more contrast against `hex`. */
function readableOn(hex: string): string {
  const n = parseInt(hex.slice(1), 16)
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255]
  // Rec. 601 luma — close enough for picking one of two text colours.
  const luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255
  return luma > 0.6 ? '#000000' : '#ffffff'
}

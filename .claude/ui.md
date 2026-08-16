# EasyClean — UI / Design System

Source of truth for look & feel and screen structure. The design is **fully realized in
`frontend/index.html`** (a complete React app) — M3 ports that into Vite + TS rather than rebuilding it.
The mobile prototype screenshots are that same app on a phone. **Both themes are locked** — dark from the
app/prototypes, light defined here (the file is dark-only; light is the one addition). See
[CLAUDE.md](CLAUDE.md) and [spec.md](spec.md).

## Visual direction (locked)

Minimal, true-black "OLED" dark UI. Big bold headers, generous spacing, rounded cards, a **floating
pill bottom tab bar**, circular/linear progress indicators, iOS-style green/red action accents. Flat
dark surfaces with subtle hairline borders — not glassmorphism-heavy. (This supersedes the earlier
"Liquid Glass vs Notion" question.) Font: bold geometric sans (Inter / SF Pro family), extra-bold headers.

## Navigation & information architecture

Bottom tab bar — floating rounded pill, 4 tabs, active tab gets a raised lighter pill:
**Clean · Gallery · Albums · Setting**. A **Swipe session** screen is pushed modally from Clean.

## Screens

### Clean (home)
- Title "EasyClean" + **Quick Scan** pill (top-right). "Welcome, {name}".
- **Weekly streak:** 7 circular progress rings (Mon…Today) + "N days strike!" + "N swipes" (trophy).
- Shortcut cards: **On This Day** (date badge), **Recents** (clock).
- **Duplicates** (N pairs) and **Blurs** (N files) cards.
- **Timeline Checklist:** month rows with "reviewed/total" + completion donut; fully-cleaned past months
  show "X GB Cleaned" + check.

### Swipe session (pushed from Clean / a month / Duplicates / Blurs)
- Top bar: back ‹ · center **MONTH YEAR** + "{done}/{total} · {pct}%" · **UNDO** pill (top-right).
- Full-bleed media card; swipe left = delete, right = keep.
- Bottom: **DELETE** (red, running count) left · **KEEP** (green, running count) right, with chevrons.
- Keyboard: ←delete →keep Space=fullscreen Z=undo. Undo = last ~10.

### Gallery
- Title "Gallery" + current date · **search** (semantic/AI) · **Select** (multi-select).
- Justified/masonry photo grid, lazy-loaded (100K+).

### Albums
- **Collections:** Favourites, Recents + user/auto albums (Pet, Food, Travel…), horizontal scroll (›).
- **Suggested Collections:** AI-suggested (Hiking, Cycling, Shopping…).
- **Memories:** month buckets (May 2026…), **Recap**, **Trash**.
- "+" creates a collection. Cards show cover + name.

### Setting
- Header "Setting" + profile chip "{name}" · role line "‹ Admin User ›".
- **Storage Saved** chart: total (e.g. 28.4 GB last 30 days, +12% vs prior) + daily bar chart.
- **Connection & Storage:** Server Status (Online dot) · Server URL · **Folder Paths** →
  Source / Cleaned / Trash (`easystore/storage/{user}/source|clean|.trash`).
- **App Settings:** Duplicate Sensitivity (slider Low↔High) · Zoom Sync (toggle, sync zoom across devices).
- **Appearance:** Theme = **Dark / Light / System** · Accent Color swatches (+ custom).
- **Language** · **Log out**.

> **Architecture implied by Settings (confirm vs PRD + backend decision):** a **server** exists (Server
> URL + Online status) and storage is **per-user, three folders** — `source` (to review), `clean`
> (kept/organized → physical albums), `.trash` (deleted, retained). Multi-user with an Admin role.

## Theme system

Three modes — **Dark / Light / System** — plus a user **accent color**. Implement as semantic tokens:
switching theme swaps the token set; switching accent swaps `--accent`. Drive off `data-theme` +
`data-accent` on `<html>`; persist the choice; honor `prefers-color-scheme` when System.

### Tokens — Dark (from prototype) · Light (locked)

| Token | Dark | Light |
|-------|------|-------|
| `--bg` (screen) | `#000000` | `#FFFFFF` |
| `--surface` (card) | `#1C1C1E` | `#F2F2F7` |
| `--surface-2` (raised / photo placeholder) | `#2C2C2E` | `#E5E5EA` |
| `--nav-bg` (floating bar) | `rgba(28,28,30,0.92)` | `rgba(255,255,255,0.92)` |
| `--text` | `#FFFFFF` | `#000000` |
| `--text-2` | `rgba(255,255,255,0.55)` | `rgba(0,0,0,0.55)` |
| `--text-3` | `rgba(255,255,255,0.28)` | `rgba(0,0,0,0.30)` |
| `--border` | `rgba(255,255,255,0.10)` | `rgba(0,0,0,0.08)` |
| `--keep` | `#30D158` | `#34C759` |
| `--delete` | `#FF453A` | `#FF3B30` |
| `--shadow` | none | `0 1px 3px rgba(0,0,0,0.10)` |

Both modes share radii (card 16–20, pill 100), spacing, and typography. Light mode adds soft shadows to
replace the dark-on-dark border separation; keep/delete are slightly stronger for contrast on white.

### Accent colors
Selectable; default differs per theme — **Dark → white `#FFFFFF`**, **Light → graphite `#1C1C1E`**.
Palette: white/graphite · teal `#2DD4BF` · green `#30D158` · yellow `#FFD60A` · orange `#FF9F0A` · + custom.
`--accent` drives selected states, active nav, sliders, links. Never hardcode colors in components —
use tokens only.

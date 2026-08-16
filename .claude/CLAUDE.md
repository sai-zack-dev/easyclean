# EasyClean — Project Context

Self-hosted photo/video cleaner. Point it at a folder, it serves a web UI on your LAN, and you swipe to
keep/delete, auto-detect blurry shots and duplicates, and organize into albums. Browser-based — any
device on the same network can use it.

See [spec.md](spec.md) for architecture/API, [ui.md](ui.md) for the design system, and
[checklist.md](checklist.md) for the build plan.

> **Source of truth:** the UI prototype images + these specs. No PRD PDF exists; if one arrives later it
> takes precedence. Read and align with these before writing implementation code.

## Architecture — locked: "one core, two shells"

A single **Rust** HTTP server (axum) serves the web UI + JSON API and does all the work (scan,
thumbnails, blur, duplicate detection, albums, trash). It binds `0.0.0.0:PORT` for LAN access. Shipped
two ways from the same core:

- **Desktop (Mac / Windows / Linux):** a **Tauri** app — control panel to pick a folder, Start/Stop, and
  show the LAN URL + QR code. Embeds the same server.
- **Linux home server:** the same server binary in a **Docker** image — set the media path in
  `docker-compose.yml`, `docker compose up -d`, browse the port.

Clients = **any browser on the LAN** (MVP). Mobile = installable **PWA** later; native later still.

## Decisions (locked)

- **Backend:** Rust only (axum + tokio). No Python.
- **AI / CLIP semantic search:** **later phase** (ONNX via `ort`), NOT in MVP.
- **Frontend:** React 18 + Vite + TS, served as static assets by the Rust server; PWA-capable. Design in [ui.md](ui.md).
- **Access:** LAN over HTTP; bind `0.0.0.0`; desktop shows IP:port + QR. Remote access later via
  Tailscale / Cloudflare Tunnel — never port-forwarding.
- **Multi-user / auth:** per-user folders + an Admin user (from the prototype). MVP needs at least a
  passcode/PIN — anyone on the Wi-Fi can otherwise see/delete photos.
- **Storage (per user):** three folders — `source` (to review), `clean` (kept/organized), `.trash`
  (deleted, retained). SQLite for metadata + index.

## MVP scope

**In:** scan + index; thumbnails (incl. video first-frame); blur detection (Laplacian variance);
duplicate detection (pHash + Hamming distance); swipe keep/delete; trash + restore; albums /
auto-categorization; stats + streaks; Settings; web UI (dark + light); LAN serving + desktop folder-pick.
**Later:** CLIP semantic search; native mobile; remote hosting.

## Tech stack

- **Server:** Rust — axum + tower-http (static + API), tokio. SQLite via `rusqlite` or `sqlx`.
- **Imaging:** `image` (decode/resize/thumbnails), `img_hash` (pHash), Laplacian blur, `kamadak-exif`
  (EXIF date/GPS), `libheif-rs` (HEIC — needs system libheif), `ffmpeg` (video thumbs / `ffprobe` dates).
- **Desktop:** Tauri (Rust) — folder-pick dialog, local-IP + QR display, supervises the server.
- **Frontend:** React 18 + Vite + TypeScript (PWA), Tailwind, Framer Motion (swipe), Zustand. Tokens in [ui.md](ui.md).
- **Deploy:** Docker + docker-compose (Linux); Tauri bundles for Mac/Windows (code-signing/notarization).
- **Later:** `ort` (ONNX Runtime) + a CLIP model for semantic search.

## Engineering standards

Production-grade, modular, type-safe. Must handle **100K+ images** — lazy loading + caching, no blocking
ops; long work (scan/thumb/detect) runs async and streams progress. AI thresholds user-configurable.

## How to work (interaction rules)

1. **Do NOT assume** — ask if unclear. 2. **Prefer incremental fixes** (exact patch/diff, not whole-file
regen). 3. **Visualize before build** — architecture + data flow first. 4. **Follow the spec/images
strictly.** 5. **Output order:** architecture + data-flow → file structure → modular code → run/test.

## Non-negotiable rules

1. **Path security** — resolve + validate every input-derived path; reject `..`; a user only ever touches
   their own folder.
2. **Trash-first** — move to `.trash`; never hard-delete in a session; permanent delete only via explicit
   purge or retention cleanup.
3. **LAN-only by default** — require a passcode; never expose to the internet without explicit opt-in
   (Tailscale / Cloudflare Tunnel).
4. **Secrets/config** from env / app config; never log them.

## Key gotchas

- **HEIC** (iPhone photos) needs system `libheif` — install in the Docker image; bundle/notarize on Mac.
- **Video** needs `ffmpeg` (thumbnails + `ffprobe` dates).
- **Long work** (scan / thumb / detect) async + progress (SSE/WS); never block at 100K+.
- **Undo** = last ~10 swipes.
- **Files move** between `source`/`clean`/`.trash` — operations must be atomic and trash-first.
- **SQLite schema** changes → migrate or wipe.

## Design system

Locked, both themes (dark from prototypes, light defined) — see [ui.md](ui.md).

## Existing assets

- **`frontend/index.html` — the canonical web UI.** A complete React 18 app (single file, React via CDN)
  implementing every screen with the exact OLED design in [ui.md](ui.md); the mobile prototype
  screenshots are this app on a phone. **M3 = port it to Vite + TS** — split into components, add the
  light theme (it's dark-only today), repoint the API client at the Rust server. Do NOT redesign it.
- `backend/` (Python FastAPI) — logic reference only (scan/pHash/blur shapes); not reused; build is Rust.
- Prototype runs via `docker compose up -d` at http://localhost:3333 (uses these two).

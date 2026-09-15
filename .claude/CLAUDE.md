# EasyClean — Project Context

Self-hosted photo/video cleaner. Point it at a folder, it serves a web UI on your LAN, and you swipe to
keep/delete, auto-detect blurry shots and duplicates, and organize into albums. Browser-based — any
device on the same network can use it.

See [spec.md](spec.md) for architecture/API, [ui.md](ui.md) for the design system, and
[checklist.md](checklist.md) for the build plan.

> **Source of truth:** the UI prototype images + these specs. No PRD PDF exists; if one arrives later it
> takes precedence. Read and align with these before writing implementation code.

## Architecture — locked: "one core, one shell"

A single **Rust** HTTP server (axum) serves the web UI + JSON API and does all the work (scan,
thumbnails, blur, duplicate detection, albums, trash). It binds `0.0.0.0:PORT` for LAN access. Shipped
one way:

- **Docker:** the server binary in a **Docker** image — set the media path in `docker-compose.yml`,
  `docker compose up -d`, browse the port. This is the only shell; the backend is never built on a dev
  machine, only inside the image.

The **Tauri desktop shell was removed on 2026-09-09** (`desktop/` crate, its release job, and the
workspace member). It lives in git history at `da8f7f5` if the folder-pick + QR control panel is ever
wanted back. Scope for now is deliberately: **Dockerfile for the backend, `web/` for the frontend.**

Clients = **any browser on the LAN** (MVP). Mobile = installable **PWA** later; native later still.

## Decisions (locked)

- **Backend:** Rust only (axum + tokio). No Python.
- **AI / CLIP semantic search:** **built** (CLIP ViT-B/32 int8 ONNX via `ort`). Weights are
  downloaded at runtime into `<data_dir>/models/`, never baked into the image. Vectors live in
  SQLite as int8 BLOBs and are scanned exactly — no vector database, no ANN index.
- **Frontend:** React 18 + Vite + TS, served as static assets by the Rust server; PWA-capable. Design in [ui.md](ui.md).
- **Access:** LAN over HTTP; bind `0.0.0.0`. Remote access later via
  Tailscale / Cloudflare Tunnel — never port-forwarding.
- **Multi-user / auth:** per-user folders + an Admin user (from the prototype). MVP needs at least a
  passcode/PIN — anyone on the Wi-Fi can otherwise see/delete photos.
- **Storage (per user):** three folders — `source` (to review), `clean` (kept/organized), `.trash`
  (deleted, retained). SQLite for metadata + index.

## MVP scope

**In:** scan + index; semantic search; thumbnails (incl. video first-frame); blur detection (Laplacian variance);
duplicate detection (pHash + Hamming distance); swipe keep/delete; trash + restore; albums /
auto-categorization; stats + streaks; Settings; web UI (dark + light); LAN serving.
**Later:** native mobile; remote hosting.

## Tech stack

- **Server:** Rust — axum + tower-http (static + API), tokio. SQLite via `rusqlite` or `sqlx`.
- **Imaging:** `image` (decode/resize/thumbnails), `img_hash` (pHash), Laplacian blur, `kamadak-exif`
  (EXIF date/GPS), `libheif-rs` (HEIC — needs system libheif), `ffmpeg` (video thumbs / `ffprobe` dates).
- **Frontend:** React 18 + Vite + TypeScript (PWA), Tailwind, Framer Motion (swipe), Zustand. Tokens in [ui.md](ui.md).
- **Deploy:** Docker + docker-compose (Linux). No desktop bundles.
- **Semantic search:** `ort` (ONNX Runtime, statically linked) + CLIP ViT-B/32 int8, `tokenizers`
  for the CLIP BPE vocabulary, `ureq` to fetch the weights on first use.

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

- **HEIC** (iPhone photos) is behind the `heic` cargo feature — it links system `libheif` ≥ 1.17, so
  Debian **trixie**, not bookworm, and the `libheif-plugin-libde265` package must be installed or
  decode fails at runtime. The Docker image always builds with it.
- **Video** needs `ffmpeg` (thumbnails + `ffprobe` dates).
- **Long work** (scan / thumb / detect) async + progress (SSE/WS); never block at 100K+.
- **Undo** = last ~10 swipes.
- **Files move** between `source`/`clean`/`.trash` — operations must be atomic and trash-first.
- **SQLite schema** changes → migrate or wipe.

## Design system

Locked, both themes (dark from prototypes, light defined) — see [ui.md](ui.md).

## Existing assets

- **`web/` is the canonical web UI** — React 18 + Vite + TS, the finished port of the original
  single-file prototype, with both themes and the PWA layer. Do NOT redesign it; match [ui.md](ui.md).
- The two prototypes it replaced (`frontend/index.html`, the Python `backend/`) were **deleted on
  2026-08-17**. They live in git history at commit `2763e16` if you ever need the original markup or
  the old scan/pHash reference.

## Deployment

Production is a Linux home server at **`/opt/easyclean`**, running the GHCR image via
`deploy/docker-compose.yml`. CI/CD: `ci.yml` gates every push and PR (fmt · clippy · tests · web ·
Docker build + container smoke test) but deploys nothing. **Pushing a `v*` tag is the promote step** —
`deploy.yml` re-runs the gate on the tagged commit, publishes the versioned image, and rolls it out
through a **self-hosted runner installed on that server**, with a health check and auto-rollback.
Manual dispatch with an existing tag rolls back without rebuilding. Setup and runbook:
[deploy/README.md](../deploy/README.md).

MVP ships **web + PWA only**. A native mobile client (Flutter or React Native — decision deferred) is
M7, after the web app has proven itself in daily use.

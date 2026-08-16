# EasyClean — Session Handoff

> Rewritten 2026-06-19. The previous version (end of session 1) was badly stale — it claimed M3/M4
> were not started when both are essentially built. Trust this file + [checklist.md](checklist.md).
> Full specs: [CLAUDE.md](CLAUDE.md) · [spec.md](spec.md) · [ui.md](ui.md)

---

## TL;DR — true state as of 2026-06-19

- **M0–M2 (Rust core + axum server):** ✅ done, compiles clean, **verified live this session**.
- **M3 (React web UI):** ✅ built — every screen in `web/src/`, builds to `web/dist`, served by the
  Rust server. (Had a build break — fixed this session, see below.)
- **M4 (Tauri desktop):** ✅ **functionally complete & spawn-path verified** (2026-06-21). `desktop/`
  builds, launches, and its server-spawn path is proven (see below). Only the manual GUI click-through is
  unrun — blocked on macOS Accessibility/Screen-Recording grant, not on code.
- **M5 (Packaging & deploy):** ✅ **artifacts complete** (2026-06-21). Multi-stage `Dockerfile`,
  single-service `docker-compose.yml` (replaces the old Python one), `.dockerignore`, `.env`/`.env.example`,
  rewritten `README.md`, `CHANGELOG.md`, `.github/workflows/release.yml`, Tauri `bundle.icon` populated.
  ⚠️ **Docker image build NOT yet run here** — host disk was ~95% full (~10 GiB free) so Docker Desktop's
  VM couldn't boot ("no space left on device"). `docker compose config` validates; the server binary the
  image packages is already proven. To verify: free disk space, then `docker compose up -d --build`.
  Bundle signing/notarization still needs your Apple/Windows certs (CI hook is in place).

## M4 completion notes (2026-06-21)

Three real gaps found & fixed in `desktop/src/main.rs`:
1. **`EASYCLEAN_WEB_DIST` now passed (absolute).** The server defaults to a *relative* `web/dist`; when
   the desktop spawns it the CWD isn't the workspace root, so the web UI would 404. Now passes
   `<workspace>/web/dist`. Verified: spawned the server from `/tmp` → `GET /` serves the UI, JS assets 200.
2. **Persistent random JWT secret.** Desktop generates 32 random bytes once, stores in `config.json`,
   passes as `EASYCLEAN_SECRET` (reused across restarts so sessions survive Stop/Start). Was using the
   server's forgeable hardcoded dev default — a LAN-security hole. Verified: dev-secret warning gone.
3. **Fail-fast.** `start_server` probes the child after 400ms; returns an error (e.g. "port in use")
   instead of leaving the UI on "Running" against a dead process.

To finish the last 1%: grant Claude Accessibility + Screen Recording (Claude desktop app → permissions),
then launch `./target/debug/easyclean-desktop`, click Start, and open the LAN URL / scan the QR.
Note: `cargo tauri build` (bundling) needs `cargo install tauri-cli` and `tauri.conf.json` `bundle.icon`
populated (currently `[]`, though `desktop/icons/` has the PNGs) — that's M5.2.

## Verified live this session (server + web stack)

Ran `./target/debug/easyclean-server` against `/Users/saiz/Downloads/storage` (160 files):
- binds `0.0.0.0:8080` (LAN-ready) · serves built web UI at `/` (`<title>EasyClean</title>`)
- `POST /api/auth/login` → JWT · `GET /api/auth/me` → `{role:admin, username}`
- `POST /api/scan` → indexes 160, blur detection runs, thumbnails written to `data/thumbs/*.jpg`
- `GET /api/media/batch?mode=blurs` → 35 blurry · `GET /api/media/:id/thumb` → 200 JPEG ·
  `/raw` → 200 PNG · `/api/stats/summary` → correct counts · `/api/albums` → ok

## ⚠️ Gotchas discovered this session (don't relearn these)

1. **Login response field is `access_token`, not `token`.** Shape:
   `{access_token, token_type:"Bearer", user:{role,username}}`. (Old handoff said `token` — wrong.)
2. **Default `node` is v18.20.5; Vite 8 needs ≥20.19.** Node 22.22.3 is installed via nvm —
   run `nvm use 22` before `npm run build`/`npm run dev`, or the build dies with `CustomEvent is not defined`.
3. **`cargo` not on PATH** until `source ~/.cargo/env` (zsh).
4. **Fixed this session:** `web/src/screens/LoginScreen.tsx` + `ServerSetup.tsx` used
   `import { useState, FormEvent }` → broke `tsc` under `verbatimModuleSyntax`. Changed to `import type`.

---

## How to run

### Rust server (API + serves built web UI)
```bash
source ~/.cargo/env
EASYCLEAN_MEDIA_ROOT=/Users/saiz/Downloads/storage \
EASYCLEAN_USER=username EASYCLEAN_PASSCODE=easyclean \
EASYCLEAN_DATA_DIR=./data EASYCLEAN_WEB_DIST=./web/dist \
cargo run -p easyclean-server          # → http://localhost:8080  (login: username / easyclean)
```

### Web UI dev (hot reload, proxies /api → :8080)
```bash
nvm use 22 && cd web && npm run dev     # → http://localhost:5173
```

### Build web for production (server serves web/dist)
```bash
nvm use 22 && cd web && npm run build
```

### Tauri desktop (not yet verified)
```bash
cargo build -p easyclean-server         # desktop spawns this binary from target/<profile>/
cd desktop && cargo tauri dev           # needs tauri-cli; see desktop/tauri.conf.json (frontendDist: "ui")
```

---

## Recommended next steps (priority order)

1. **M4 GUI click-through** (last 1%) — grant screen permissions, launch `target/debug/easyclean-desktop`,
   Start, scan the QR from a phone, run login→scan→swipe→finish→albums. Code + spawn path already verified.
2. **M5.1 Docker** — multi-stage Dockerfile (build Rust + web, runtime with `libheif`+`ffmpeg`,
   non-root, healthcheck). **Replace** the current `docker-compose.yml` (it still runs the old Python
   app on :3333) with one that runs `easyclean-server`.
3. **M5.2 desktop bundle** — `cargo tauri build` (macOS sign/notarize, Windows sign).
4. **Cleanup** — delete the old `backend/` (Python) and decide the fate of `frontend/index.html`
   (design reference; the port is done). Add `git init` + README (still no git repo here).

## Known deferred / not-in-MVP-yet
- HEIC decode (needs `brew install libheif`; none in test set) · EXIF-orientation correction on thumbs
- `ffmpeg` not installed on this Mac → video thumbnails skip gracefully (the 1 MP4 has no thumb)
- Benchmark on 10K+ images (validated on 160) · M6 CLIP semantic search (post-MVP)

---

## File structure (unchanged from before — see git/tree)
`core/` (engine lib) · `server/` (axum bin) · `web/` (React/Vite, built → `web/dist`) ·
`desktop/` (Tauri) · `frontend/index.html` (design reference) · `backend/` (old Python, delete) ·
`.claude/` (CLAUDE.md, spec.md, ui.md, checklist.md, this file)

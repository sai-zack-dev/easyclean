# EasyClean — Roadmap (Milestones · Phases · Tasks)

Rust-only "one core, two shells" (see [CLAUDE.md](CLAUDE.md) / [spec.md](spec.md)); screens & design in
[ui.md](ui.md). Task legend: `[ ]` todo · `[~]` in progress · `[x]` done.

**MVP = M0 → M5.** M6+ is post-MVP.

| Milestone | Goal | Exit criteria |
|-----------|------|---------------|
| **M0 Foundations** | Compiling workspace, config, DB, storage model | `cargo build` + `cargo test` green; server boots; DB migrates |
| **M1 Core engine** | Headless lib: scan, thumbs, detect, fileops | Scan a test dir → DB + thumbs + blur/dupe flags; fileops tested |
| **M2 API server** | axum HTTP/SSE API + auth over the core | Full API callable; SSE progress; auth enforced |
| **M3 Web UI** | The 5 screens (PWA), both themes, against the API | Full user journey in a browser; dark+light; PWA passes |
| **M4 LAN + Desktop** | Tauri app: pick folder → serve on LAN | Mac app → pick folder → URL+QR → open from phone |
| **M5 Packaging** | Docker image + desktop bundles + docs | `docker compose up -d` on clean box; signed builds; README |
| **M6 CLIP search** | Semantic search (post-MVP) | text→image + similar, adjustable threshold |
| **M7 Beyond** | Native mobile · remote access (post-MVP) | — |

Cross-cutting quality gates (every milestone): `cargo clippy` + `fmt` clean, unit/integration tests,
trash-first + path-traversal safety, no blocking ops, works toward 100K+ images.

---

## M0 — Foundations & scaffolding

**✅ M0 COMPLETE** — `cargo build` + `cargo test` (6 passing) green; server boots, migrates, serves `/api/health`.

### Phase 0.1 — Workspace
- [x] Cargo workspace: `core/` (lib), `server/` (axum bin) — *`desktop/` Tauri deferred to M4 (needs the web build to embed)*
- [x] `web/` React + Vite + TS app initialized — *done in M3*
- [x] Tooling: `rustfmt.toml`, `.gitignore` — *`.editorconfig` + CI deferred*
- [ ] `git init`; README skeleton; LICENSE — *not yet (M5 for README)*
- [ ] CI: build + test + clippy + fmt on push — *deferred*

### Phase 0.2 — Config & logging
- [x] Settings module (`EASYCLEAN_*` env + defaults): media root, data dir, port, blur/dup thresholds, retention
- [x] Structured logging (`tracing` + env-filter)
- [x] Error type (`thiserror`) + `Result` alias — *HTTP error mapping lands in M2*

### Phase 0.3 — Database
- [x] SQLite via `sqlx` + connection pool
- [x] Migrations: `media_file`, `album`, `album_file`, `user`
- [x] Indexes: `(user, status)`, `group_id`, `(user, created_at)`, `phash`
- [x] Repository (insert + count) + unit test — *full CRUD grows in M1*

### Phase 0.4 — Storage model
- [x] Per-user three-folder resolver: `source/` · `clean/` · `.trash/`
- [x] Path-safety util: lexical normalize + assert inside user root, reject `..` (unit-tested)
- [ ] Document + script system deps: `libheif`, `ffmpeg` — *next, before M1*

---

## M1 — Core engine (headless library)

**✅ M1 COMPLETE (core)** — `cargo test` 12 passing; ran on **160 real photos → 159 analyzed, 42 blurry,
23 duplicate groups, 159 thumbnails**. dHash + Laplacian blur implemented in-house. Dev tool:
`cargo run -p easyclean-core --example scan -- <dir> [user]`.

### Phase 1.1 — Scanner
- [x] Recursive walk; filter image + video extensions
- [x] Metadata: size, mtime; EXIF `DateTimeOriginal` + GPS (`kamadak-exif`)
- [ ] HEIC/HEIF decode — *deferred: needs `libheif` (not installed; none in test set). `.heic` is indexed, not yet decoded.*
- [x] Upsert into DB; dedupe by path (incremental rescan via `existing_paths`)
- [x] Async + progress callback

### Phase 1.2 — Thumbnails
- [x] Image thumbnails (`image`, JPEG, 480px longest edge)
- [ ] EXIF-orientation correction — *refinement TODO*
- [ ] HEIC thumbnails — *with libheif (see 1.1)*
- [x] Video first-frame via `ffmpeg` — *implemented; degrades gracefully when ffmpeg absent*
- [x] Thumbnail cache dir (`<data>/thumbs/<id>.jpg`)

### Phase 1.3 — Detection
- [x] Blur: grayscale → Laplacian variance vs `BLUR_THRESHOLD`; score stored
- [x] Duplicates: in-house dHash; union-find grouping by Hamming distance ≤ threshold → `group_id`
- [x] Process only un-hashed rows; async + heavy decode on `spawn_blocking`

### Phase 1.4 — File operations
- [x] Move to `.trash/` — rename + copy/remove cross-device fallback (trash-first)
- [x] Restore from trash
- [x] Move kept → `clean/<album>/`
- [x] Purge (permanent delete)
- [x] Filename collision handling (unit-tested)
- [ ] Benchmark on ~10K images — *validated on 160; larger benchmark pending*

---

## M2 — HTTP API server (axum)

**✅ M2 COMPLETE** — 23 tests pass (19 core + 4 server). All endpoints verified live.
Run: `EASYCLEAN_USER=<user> EASYCLEAN_DATA_DIR=./data cargo run -p easyclean-server`

### Phase 2.1 — Server skeleton
- [x] axum app + router + shared state (db pool, config, scan progress)
- [x] `tower-http`: tracing + permissive CORS — *static `web/` serving wired in M3*
- [x] `/api/health`; error → HTTP mapping (`ApiError`)

### Phase 2.2 — Auth  ✅
- [x] Seed users from config; passcode hashing (**bcrypt**); JWT (HS256) tokens
- [x] `POST /api/auth/login` → token; `AuthUser` extractor; `GET /api/auth/me`
- [x] Per-user scoping on all data routes (ownership-checked); Admin vs user role
- [x] Rate-limit login — *done in Phase 2.4 (`LoginRateLimiter`, 5/15min → 429)*

### Phase 2.3 — Media & review
- [x] `POST /api/scan` (background task) + `GET /api/scan/status` — *JSON polling now; SSE in part 2*
- [x] `GET /api/media/batch` (timeline | blurs | duplicates, month filter)
- [x] `POST /api/media/swipe` keep/delete (sets status; reversible via trash restore)
- [x] `POST /api/media/session/finish` → move kept to `clean/` + categorize *(done in Phase 2.4)*
- [x] `GET /api/media/:id/thumb` · `/raw` (path-validated to media root)
- [x] `POST /api/media/:id/favorite`

### Phase 2.4 — Albums · trash · stats ✅
- [x] Categorizer (`categorize.rs`): month/year, screenshots (name heuristic), videos, favorites
- [x] `GET /api/albums` · `GET /api/albums/:id/files?page=&size=` · `POST /api/albums/refresh`
- [x] `POST /api/media/session/finish` — moves kept files to `clean/<album>/`, runs categorization
- [x] `GET /api/trash` · `POST /api/trash/restore/:id` · `DELETE /api/trash/purge/:id` · `DELETE /api/trash/purge-all`
- [x] Retention auto-cleanup background task (runs every hour, purges files older than `TRASH_RETENTION_DAYS`)
- [x] `GET /api/stats/summary` · `GET /api/stats/streak` (daily streak + last-7-days + total swipes)
- [x] SSE on `GET /api/scan/status` — streams `progress` events, closes with `done`
- [x] Login rate-limit — 5 attempts / 15 min per IP; returns HTTP 429 on excess
- [x] 23 tests (19 core + 4 server); verified live: login→JWT, rate-limit 401×5→429, streak, SSE, albums, purge

---

## M3 — Web UI MVP (PWA)

**✅ M3 COMPLETE** — Vite + React 18 + TS app in `web/` with full design port from `frontend/index.html`.
Dark/light/system theme. Zustand store + typed API client. Framer Motion swipe. `npm run build` → `web/dist/`
(358 KB JS, 18 KB CSS). Rust server serves `web/dist` via `tower-http ServeDir` fallback.
Dev: `cd web && npm run dev` (Vite proxies `/api/*` → `:8080`).

### Phase 3.1 — App shell & infra
- [x] Vite + TS + Tailwind CSS (`@tailwindcss/vite`); PWA-ready HTML meta tags
- [x] Design tokens (Dark / Light / System + accent); `data-theme` on `<html>`; persisted via Zustand
- [x] Zustand store (token, server URL, user, theme, accentIndex) — persisted to localStorage
- [x] Typed API client + 401 auto-logout; Bearer token injection
- [x] Floating tab bar (mobile) + sidebar (desktop) navigation

### Phase 3.2 — Server setup + login
- [x] `ServerSetup` screen (enter backend URL, health-check before save)
- [x] `LoginScreen` (username + passcode → JWT)

### Phase 3.3 — Clean (home)
- [x] Quick Scan button → `POST /api/scan` + SSE progress stream
- [x] Weekly streak rings (done / today states) + fire/trophy badges
- [x] On This Day · Recents · Duplicates (N pairs) · Blurs (N files) quick cards
- [x] Timeline Checklist (progress rings, reviewed/total, "Cleaned" state)

### Phase 3.4 — Swipe session
- [x] `SwipeCard` with Framer Motion `drag="x"` + `dragElastic` throw physics
- [x] Mode header (label + progress counter); UNDO (last ~10 swipes)
- [x] DELETE / KEEP buttons + running counts (padded `000`)
- [x] KEEP/DELETE overlay labels fade in on drag direction
- [x] BLUR / DUPLICATE badges on cards

### Phase 3.5 — Gallery
- [x] Photo grid grouped by date with date headers
- [x] Thumbnail loading with error fallback placeholder

### Phase 3.6 — Albums
- [x] Collections / Suggested / Memories horizontal-scroll sections
- [x] Real albums from `/api/albums` with static fallbacks

### Phase 3.7 — Settings
- [x] Profile pill + Storage-Saved chart (bar chart)
- [x] Server status / URL input + save
- [x] Duplicate Sensitivity slider; Zoom Sync toggle
- [x] Theme (Dark/Light/System) + Accent color picker; Log out

### Rust server wiring
- [x] `tower-http ServeDir` fallback on `web/dist` (override via `EASYCLEAN_WEB_DIST`)

---

## M4 — LAN serving + Desktop shell (Tauri)

### Phase 4.1 — LAN serving
- [x] Bind `0.0.0.0:PORT` — *verified live: server binds `0.0.0.0:8080`*; detect primary LAN IPv4 (`local_ip_address` in `desktop/src/main.rs`)
- [ ] (optional) mDNS `easyclean.local`

### Phase 4.2 — Tauri app  ✅ *(spawn path verified; GUI click-through pending screen-recording grant)*
- [x] Launch the axum core as a child process (`start_server`/`stop_server` in `desktop/src/main.rs`)
- [x] **Server spawn passes `EASYCLEAN_WEB_DIST` (absolute)** — fixes the embedded server 404'ing the
      web UI when its CWD isn't the workspace root; verified by spawning from `/tmp` → UI + assets served
- [x] **Persistent random JWT secret** — generated once, stored in `config.json`, passed as
      `EASYCLEAN_SECRET`; replaces the forgeable hardcoded dev default (LAN-security rule #3)
- [x] **Fail-fast on startup** — `start_server` probes the child after 400ms and errors (e.g. port in use)
      instead of showing "Running" against a dead process
- [x] Folder picker → set media root (`pick_folder`, `tauri-plugin-dialog`)
- [x] Start/Stop server + status indicator (`server_status` + 4s health poll)
- [x] Show URL + QR code (`qr_svg` via `qrcodegen`); "open in browser" (`tauri-plugin-opener`)
- [x] First-run onboarding; persist config (`load_config`/`save_config` → app data dir)
- [ ] **GUI click-through** still unrun (needs macOS Accessibility/Screen-Recording grant): launch
      `target/debug/easyclean-desktop`, pick folder, Start, scan QR from a phone, full journey

---

## M5 — Packaging & deploy

### Phase 5.1 — Docker ✅
- [x] Multi-stage `Dockerfile` (Node web build → Rust release → slim Debian runtime w/ `ffmpeg` + `libheif1`)
      — server-only workspace manifest swapped in so `desktop/` (GTK/webkit) isn't built; `.dockerignore` added
- [x] `docker-compose.yml` — media volume **rw** (app moves files), `./data` volume, `${PORT}`, env;
      requires `EASYCLEAN_SECRET`. `.env` + `.env.example` updated (old Python compose replaced)
- [x] `HEALTHCHECK` curl `/api/health`; runs as non-root (`easyclean`, uid 10001)

### Phase 5.2 — Desktop bundles  *(config + CI ready; signing needs the user's certs)*
- [x] `tauri.conf.json` `bundle.icon` populated; `bundle.targets: all`; build steps in README
- [ ] macOS `.app` sign + notarize — needs Apple Developer cert (documented; CI hook in place)
- [ ] Windows `.msi/.exe` sign — needs code-signing cert (documented)
- [x] Release workflow builds Linux/macOS/Windows bundles via `tauri-action` (draft release)

### Phase 5.3 — Docs & release ✅
- [x] README rewritten: Docker `compose up -d` + desktop build, LAN access, **security notes**
- [x] `CHANGELOG.md` (Keep a Changelog) + `.github/workflows/release.yml` (tag `v*` → GHCR image + bundles)

---

## M6 — CLIP semantic search (post-MVP)
- [ ] CLIP ONNX model via `ort`; 512-dim embeddings on scan
- [ ] Vector store (sqlite-vec / faiss); cosine similarity
- [ ] `GET /api/search?q=` (text→image) + visually-similar; adjustable threshold
- [ ] Gallery search UI wired to it

## M7 — Beyond (post-MVP)
- [ ] Native mobile client
- [ ] Remote access (Tailscale / Cloudflare Tunnel, auto-HTTPS)
- [ ] Cloud upload; multi-tenant hardening

# EasyClean — Roadmap (Milestones · Phases · Tasks)

> **2026-09-09 — the Tauri desktop shell was removed.** M4 Phase 4.2, M5 Phase 5.2, and every other
> `desktop/` item below are kept as a record of work that was done and then deleted; they are history,
> not open work. The shipping scope is now the Dockerfile for the backend and `web/` for the frontend.
> The deleted code is in git history at `3ffbe50`.

Rust-only "one core, one shell" (see [CLAUDE.md](CLAUDE.md) / [spec.md](spec.md)); screens & design in
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
| **M7 Native mobile** | Flutter or React Native client (post-MVP) | installs from a store, talks to the same API |
| **M8 Beyond** | Remote access · multi-tenant (post-MVP) | — |

**MVP delivery = web + PWA only.** Native mobile is deliberately after the web app is proven in daily
use on the home server (M7).

Cross-cutting quality gates (every milestone): `cargo clippy` + `fmt` clean, unit/integration tests,
trash-first + path-traversal safety, no blocking ops, works toward 100K+ images.

---

## M0 — Foundations & scaffolding

**✅ M0 COMPLETE** — `cargo build` + `cargo test` (6 passing) green; server boots, migrates, serves `/api/health`.

### Phase 0.1 — Workspace
- [x] Cargo workspace: `core/` (lib), `server/` (axum bin) — *`desktop/` Tauri deferred to M4 (needs the web build to embed)*
- [x] `web/` React + Vite + TS app initialized — *done in M3*
- [x] Tooling: `rustfmt.toml`, `.gitignore`
- [x] `git init`; README; LICENSE (MIT) — *done 2026-08-17*
- [x] CI: build + test + clippy + fmt on push — *`.github/workflows/ci.yml`, done 2026-08-17*

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
- [x] Document + script system deps: `libheif`, `ffmpeg` — *installed in the image; documented in
      README + [deploy/README.md](../deploy/README.md); `deploy/install.sh` covers host setup*

---

## M1 — Core engine (headless library)

**✅ M1 COMPLETE (core)** — `cargo test` 12 passing; ran on **160 real photos → 159 analyzed, 42 blurry,
23 duplicate groups, 159 thumbnails**. dHash + Laplacian blur implemented in-house. Dev tool:
`cargo run -p easyclean-core --example scan -- <dir> [user]`.

### Phase 1.1 — Scanner
- [x] Recursive walk; filter image + video extensions
- [x] Metadata: size, mtime; EXIF `DateTimeOriginal` + GPS (`kamadak-exif`)
- [x] HEIC/HEIF decode — *done 2026-08-17 via `libheif-rs` behind the `heic` cargo feature
      (registers an `image`-crate hook, so `image::open` handles `.heic`). Docker image builds with it
      + ships `libheif-plugin-libde265` for the HEVC decode. Desktop bundles still build without it.*
- [x] Upsert into DB; dedupe by path (incremental rescan via `existing_paths`)
- [x] Async + progress callback

### Phase 1.2 — Thumbnails
- [x] Image thumbnails (`image`, JPEG, 480px longest edge)
- [x] EXIF-orientation correction — *done 2026-08-17: `core/src/decode.rs::open_image` applies the
      EXIF orientation on every decode, so portrait phone photos stop coming out sideways*
- [x] HEIC thumbnails — *falls out of the decode hook in 1.1*
- [x] Video first-frame via `ffmpeg` — *implemented; degrades gracefully when ffmpeg absent*
- [x] Thumbnail cache dir (`<data>/thumbs/<id>.jpg`)

### Phase 1.3 — Detection
- [x] Blur: grayscale → Laplacian variance vs `BLUR_THRESHOLD`; score stored
- [x] Duplicates: in-house dHash; union-find grouping by Hamming distance ≤ threshold → `group_id`
- [x] Process only un-hashed rows; async + heavy decode on `spawn_blocking`
- [x] **Parallel across cores** *(2026-08-17)* — `run_detection` used to await each `spawn_blocking`
      before starting the next, so 1 core worked and the rest idled. Now `buffer_unordered`
      (`available_parallelism()` in flight), DB writes applied in completion order: **168.8s → 60.6s
      for 10K images (2.8×)**, identical output. Test:
      `detect::tests::detection_processes_every_file_concurrently`

### Phase 1.4 — File operations
- [x] Move to `.trash/` — rename + copy/remove cross-device fallback (trash-first)
- [x] Restore from trash
- [x] Move kept → `clean/<album>/`
- [x] Purge (permanent delete)
- [x] Filename collision handling (unit-tested)
- [x] Benchmark on ~10K images — *`cargo run --release -p easyclean-core --example bench -- 10000`
      generates a synthetic library and times scan · detect · the per-screen queries · incremental
      rescan. 10K: scan 464/s, detect 165/s, queries 2.6–18.7 ms, rescan 105K/s, DB 7.4 MB. Full
      numbers + caveats in [session-handoff.md](session-handoff.md).*
- [ ] Benchmark on real 12 MP photos — *synthetic 320×240 images understate decode cost; the honest
      100K figure needs a real library. Known next lever: O(n²) `group_duplicates` → BK-tree.*

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
- [x] **Installable PWA** *(done 2026-08-17)* — `manifest.webmanifest`, icon set (192/512/maskable/
      apple-touch, generated by `web/scripts/generate-icons.mjs`, no image deps), and a hand-written
      `sw.js`: network-first navigations with an offline shell, cache-first hashed assets, cached
      thumbnails (capped at 600), and never-cached API state. Registered only in prod + secure
      contexts — over plain-HTTP LAN it runs but isn't installable (needs HTTPS via a tunnel)
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
- [x] **API base URL defaults to same-origin** *(fixed 2026-08-17)* — the client used to persist a
      hardcoded `http://localhost:8080`, which on a phone points at the *phone*, so the first screen
      from a LAN device was always "enter your server address". Verified in a browser: load → Login →
      scan → thumbnails, with no setup detour. `Settings → Backend URL` remains the override for a
      split dev setup; a zustand `migrate` clears the stale localhost value from existing clients

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

### Phase 5.1 — Docker ✅ *(image actually built + smoke-tested 2026-08-17)*
- [x] Multi-stage `Dockerfile` (Node web build → Rust release → slim Debian runtime)
- [x] **Base moved bookworm → trixie** — `libheif-rs` needs libheif ≥ 1.17 and bookworm ships 1.15.
      Runtime installs `ffmpeg`, `libheif1`, `libheif-plugin-libde265` (the actual HEVC decoder — without
      the plugin libheif opens an iPhone `.heic` and then fails to decode it) + `libheif-plugin-aomdec`
- [x] **Fixed: the build never worked.** The stub workspace manifest the Dockerfile wrote dropped
      `[workspace.dependencies]`, so cargo failed with "error inheriting `serde` from workspace root".
      Now it copies the real `Cargo.toml` and only strips `desktop` from `members`
- [x] Server compiled with `--features heic`
- [x] `docker-compose.yml` — media volume **rw**, `./data` volume, `${PORT}`, env; `EASYCLEAN_SECRET`
      now optional (server generates + persists one)
- [x] `HEALTHCHECK` curl `/api/health`; runs as non-root (`easyclean`, uid 10001)
- [x] **Verified live 2026-08-17** — image built (210 MB) and smoke-tested: health · UI · manifest ·
      `sw.js` · icons · 401 without a token · login → JWT · uid 10001 · `/data` writable ·
      `secret.key` created `0600` · ffmpeg 7.1.5 present · both libheif decoder plugins present ·
      real `.heic`/`.heif` files scanned → blur scores + valid JPEG thumbnails + cross-container
      duplicate grouping

### Phase 5.2 — Desktop bundles  *(config + CI ready; signing needs the user's certs)*
- [x] `tauri.conf.json` `bundle.icon` populated; `bundle.targets: all`; build steps in README
- [ ] macOS `.app` sign + notarize — needs Apple Developer cert (documented; CI hook in place)
- [ ] Windows `.msi/.exe` sign — needs code-signing cert (documented)
- [x] Release workflow builds Linux/macOS/Windows bundles via `tauri-action` (draft release)

### Phase 5.3 — Docs & release ✅
- [x] README rewritten: Docker `compose up -d` + desktop build, LAN access, **security notes**
- [x] `CHANGELOG.md` (Keep a Changelog) + tag `v*` → GHCR image (was `release.yml`; folded into `deploy.yml`)

### Phase 5.4 — CI/CD to the home server ✅ *(2026-08-17)*
- [x] `ci.yml` — on every push/PR: `cargo fmt --check`, `clippy -D warnings`, tests (all with
      `--features heic`), web lint + typecheck + build, PWA-asset check, **Docker build + container
      smoke test** (health, UI, manifest, 401-without-token, login→token, HEIC-enabled log line)
- [x] `deploy.yml` — **push tag `v*`** → re-run the CI gate on the tagged commit → build & push
      `ghcr.io/<owner>/easyclean:<version>` + `:<major>.<minor>` + `:sha-<commit>` + `:latest` → roll
      out on a **self-hosted runner on the home server** (`runs-on: [self-hosted, linux, x64,
      easyclean]`): `docker compose up -d`, wait for health, **auto-rollback to the previous tag** if
      it never goes healthy. `environment: production` so a manual approval gate can be switched on.
      Manual dispatch with an existing tag = rollback, no rebuild *(tag-driven since 2026-09-13; was
      auto-deploy on green `main`)*
- [x] `deploy/` production kit — `docker-compose.yml` (pulls GHCR, tag pinned in `.env`),
      `.env.example`, `install.sh` (creates `/opt/easyclean`, chowns to uid 10001, first boot),
      `README.md` (runner setup, rollback, backup, security, troubleshooting)
- [ ] Register the runner on the home server + first live rollout — *needs the server; see
      [deploy/README.md](../deploy/README.md) §2*

---

## M6 — CLIP semantic search (post-MVP)
- [ ] CLIP ONNX model via `ort`; 512-dim embeddings on scan
- [ ] Vector store (sqlite-vec / faiss); cosine similarity
- [ ] `GET /api/search?q=` (text→image) + visually-similar; adjustable threshold
- [ ] Gallery search UI wired to it

## M7 — Native mobile app (post-MVP)

Only after the web/PWA MVP has run in daily use on the home server. The API and design system are
already the contract, so this is a client, not a rewrite — no server work should be needed beyond
what's listed here.

### Phase 7.1 — Decide the stack
- [ ] **Flutter vs React Native** — weigh: RN reuses the existing TS API client, component logic, and
      the team's React knowledge; Flutter gives smoother 60fps swipe physics and one codebase for
      tablets. Decide before any code.
- [ ] Spike the swipe deck in the chosen stack — it's the screen that decides how the app *feels*.

### Phase 7.2 — Client parity
- [ ] Server discovery: manual URL entry + mDNS if available *(the desktop app's QR is gone as of 2026-09-09)*
- [ ] Login → JWT stored in the platform keychain (not plain preferences)
- [ ] The 5 screens against the same API; design tokens ported from [ui.md](ui.md)
- [ ] Native-only wins: hardware back/gestures, haptics on swipe, share sheet

### Phase 7.3 — Beyond-the-browser features
- [ ] Offline queue: swipe without the server reachable, sync on reconnect
- [ ] Background thumbnail prefetch + on-device cache
- [ ] Optional: import from the phone's own camera roll into `source/`

### Phase 7.4 — Ship
- [ ] App Store / Play Store accounts, signing, privacy labels ("no data leaves your LAN")
- [ ] CI: build both platforms on tag; TestFlight / internal track

## M8 — Beyond (post-MVP)
- [ ] Remote access (Tailscale / Cloudflare Tunnel, auto-HTTPS)
- [ ] Cloud upload; multi-tenant hardening

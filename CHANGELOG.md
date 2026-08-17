# Changelog

All notable changes to EasyClean are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **HEIC/HEIF decode** (`heic` cargo feature, `libheif-rs`) — iPhone photos now get thumbnails, blur
  scores, and duplicate hashes like any other format. Always enabled in the Docker image.
- **EXIF orientation** is applied on decode, so portrait phone photos no longer render sideways.
  All decoding now goes through one place, `core/src/decode.rs`.
- **Installable PWA** — web app manifest, generated icon set (192/512/maskable/apple-touch), and a
  service worker: offline app shell, cache-first hashed assets, cached thumbnails (capped), and
  never-cached API state.
- **CI** (`ci.yml`) — fmt, clippy `-D warnings`, tests, web lint/typecheck/build, PWA-asset check, and
  a Docker build whose container is smoke-tested (health, UI, auth enforcement, login, HEIC support).
- **CD** (`deploy.yml`) — green CI on `main` publishes `ghcr.io/<owner>/easyclean:sha-<commit>` and
  rolls it out to the home server through a self-hosted runner, with a health check and automatic
  rollback to the previous tag.
- **`deploy/` production kit** for `/opt/easyclean`: GHCR-based compose file, `.env.example`,
  `install.sh`, and a runbook covering runner setup, rollback, backup, and security.
- **Scale benchmark** (`core/examples/bench.rs`) — generates a synthetic library and times scan,
  detection, per-screen queries, and incremental rescan.

### Changed
- **Detection now runs across all cores.** It awaited each blocking decode before starting the next,
  leaving one core busy and the rest idle; it now keeps `available_parallelism()` files in flight and
  applies results to SQLite as they land. 10 000 images: **168.8s → 60.6s (2.8×)**, identical output.
- MIT `LICENSE`.
- **M5 — Packaging & deploy.** Multi-stage `Dockerfile` (Node web build → Rust release build → slim
  Debian runtime with ffmpeg + libheif, non-root, healthcheck) and a single-service `docker-compose.yml`.
  `.env.example` template and a tag-triggered GitHub release workflow.
- **M4 — Tauri desktop shell.** Control panel: folder picker, Start/Stop the embedded server, status
  indicator, LAN URL + QR code, "Open in Browser", persisted config. Generates a persistent random JWT
  secret on first run; passes an absolute web-dist path so the embedded server serves the UI regardless
  of working directory; fails fast if the server can't start.
- **M3 — Web UI.** React 18 + Vite + TS port of the prototype: Clean/Swipe/Gallery/Albums/Settings,
  Framer-Motion swipe, dark/light/system themes + accent picker, served by the Rust server.
- **M2 — HTTP API.** axum server: auth (bcrypt + JWT, rate-limited), scan + SSE progress, media batch,
  swipe, session finish, thumbnails, albums, trash, stats/streaks, hourly retention purge.
- **M1 — Core engine.** Scanner (EXIF date/GPS), thumbnails (image + ffmpeg video first-frame), blur
  (Laplacian variance) + duplicate (dHash + Hamming) detection, trash-first file ops, categorizer.
- **M0 — Foundations.** Cargo workspace, `EASYCLEAN_*` config, SQLite migrations, per-user storage model
  with path-traversal guard.

### Fixed
- **The web app now talks to the server that served it.** The API base URL defaulted to a hardcoded
  `http://localhost:8080` and was persisted into every browser — on a phone that address is the phone,
  so opening the app from a LAN device always landed on the "enter your server address" screen. It now
  defaults to same-origin, with `Settings → Backend URL` as the override and a store migration that
  clears the stale value.
- Service-worker thumbnail cache keys no longer include the `?token=` query, so a JWT isn't persisted
  in Cache Storage and cache hits survive a new token.
- **The Docker image never actually built.** The Dockerfile synthesized a stub workspace manifest that
  omitted `[workspace.dependencies]`, so cargo failed with "error inheriting `serde` from workspace
  root". It now edits the real manifest instead. The base also moved from Debian bookworm to trixie,
  because `libheif-rs` needs libheif ≥ 1.17 and bookworm ships 1.15.
- Web lint errors: a component declared during render in `SettingsTab` (remounted on every render,
  losing state), a `setState` called synchronously inside an effect in `SwipeScreen` (which also
  raced — a slow batch could overwrite a newer deck), and a stale-closure effect dependency in
  `TrashScreen`.
- All `cargo clippy` warnings; `cargo fmt` is clean. `npm audit` reports 0 vulnerabilities.

### Removed
- The superseded prototypes: `frontend/index.html` (single-file React app, now ported to `web/`) and
  the Python FastAPI `backend/`. Both remain in git history at `2763e16`.

### Security
- **No shared default JWT secret.** The forgeable `dev-insecure-secret-change-me` fallback is gone.
  The secret comes from `EASYCLEAN_SECRET`, else a persisted `<data_dir>/secret.key` (mode 0600), else
  a freshly generated random key — resolved and logged on *every* boot, not just the first.
- **CORS is closed by default.** `CorsLayer::permissive()` is replaced by same-origin-only, opt-in via
  `EASYCLEAN_CORS_ORIGINS`. Previously any page on the LAN could read the API with a stolen token.
- Media is mounted read-write (the app moves files); README documents the LAN-only posture, passcode
  requirements, and trash-first deletion.

### Notes
- CLIP semantic search (M6) and a native mobile client (M7) remain post-MVP; MVP ships web + PWA.

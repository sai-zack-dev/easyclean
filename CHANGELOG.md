# Changelog

All notable changes to EasyClean are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **The pre-rollout index backup could not write.** The snapshot step added in 0.1.1 ran `mkdir
  data/backups` as the deploy user, but the data dir belongs to uid 10001 (the container's user) and
  that user only owns `.env` — so the rollout aborted with EPERM before touching anything. It now
  runs inside a container as that uid, and is best-effort: a failed backup warns instead of blocking
  the deployment. Caught on the v0.1.1 rollout; the server was left untouched on 0.1.0 because the
  failure happened before the image swap.

## [0.1.1] — 2026-09-13

Review quality, detail views, and settings that actually do something.

### Added
- **Full-quality previews** (`GET /api/media/:id/preview`) — a 1600px, quality-88 JPEG rendered from
  the original and cached under `<data_dir>/previews/`. The swipe deck and the new detail viewer use
  it instead of the 480px thumbnail, which was far too soft to judge a photo by. The thumbnail is
  still painted first as an instant placeholder and cross-fades out. Videos fall back to their
  first-frame thumbnail, and an undecodable file falls back rather than breaking the card.
- **Photo detail viewer** — full-screen view opened from the Gallery grid or from inside an album:
  arrow-key and on-screen paging across the whole set, Escape to close, favourite toggle, and
  filename/date/size/flags. Videos play inline.
- **Album detail view** — album cards open their contents as a grid (paged, 60 at a time), and any
  photo opens in the viewer.
- **Real "Storage Saved" chart** (`GET /api/stats/storage?days=N`) — bytes and file counts binned per
  day, zero-filled across the window. The Settings chart previously drew a hardcoded array of bars
  and a fake "+12% vs last 30 days".
- **Per-user settings** (`GET`/`POST /api/settings`, `user_setting` table) — the first key is
  `duplicate_distance`. `EASYCLEAN_*` env vars remain the fallback for anything never set.
- **Gallery filename search** — the search button in the Gallery header now filters; it previously
  did nothing. Semantic search remains a later phase.
- **Designed icon set.** The supplied Broom, Pictures Folder, Settings, Fire and Trophy artwork now
  drives Clean/Gallery/Setting in the nav, the streak badges, and the Settings Library rows. The
  files are Figma exports that wrap a 90x90 white-on-transparent PNG inside an `.svg`, so they are
  used as CSS alpha *masks* over `currentColor` rather than as images — that is what lets raster
  artwork follow the theme and the accent. Each is inlined by Vite, so there are no extra requests.

### Fixed
- **Duplicate Sensitivity now works.** The slider was inert — no `onChange`, no persistence, no
  effect on detection. It now maps Low/Medium/High to pHash Hamming distances 4/10/16, saves per
  user, and re-clusters the library immediately via `detect::regroup_duplicates`, which reuses the
  stored hashes so changing the setting never re-decodes anything.
- **Accent Color now works.** `--accent` was defined in both themes but referenced by no rule, so
  picking a swatch changed nothing. It now drives the active nav/tab item, today's streak ring, the
  storage bars, the selected theme chip, the slider thumb, progress rings and focus outlines. Swatch
  0 is "Default", meaning *follow the theme* — white on dark, black on light — rather than the old
  hardcoded `#ffffff`, which was invisible in light mode.
- **Rollback survives a migration.** sqlx aborts by default when a database
  carries a migration the binary doesn't know about — exactly the state after deploying a release
  that adds one. An automatic rollback would then crash-loop instead of restoring service, silently
  defeating the safety net in `deploy.yml`. Migrations now open with `ignore_missing`, so an older
  image boots normally against a newer database and ignores tables it doesn't use. `deploy.yml` also
  snapshots the index into `data/backups/` before every rollout (last 10 kept), and the runbook
  documents recovery. **Rolling back from 0.1.1 to 0.1.0 still needs that DB restore** — 0.1.0 was
  built before this fix; from 0.1.1 onwards rollback is clean.
- **The Clean icon no longer looks like a pen** — it is the supplied broom glyph.
- **Icon style is consistent per row.** Adopting filled artwork left outline glyphs stranded beside
  it, so `album`, `calendar`, `clock`, `copy`, `blur` and `folder` were redrawn as filled glyphs.
  `Calendar.svg` and `Clock.svg` were not used: both are drawn in `#D9D9D9` with colours hardcoded,
  so they could not follow the theme. `Duplicate Images.svg` was not used either — it layers two
  photo glyphs around an opaque `#333` card, which flattens to a featureless blob as a mask.
- **The Gallery no longer hides photos you kept.** `get_batch` scoped every mode to
  `status = 'pending'`, which is right for a swipe deck and wrong for a library view, so a kept photo
  vanished from the Gallery. Added `mode=library` (everything except trash) and pointed the Gallery
  at it.
- **Progress rings are visible in light mode.** `ProgressRing` hardcoded `rgba(255,255,255,…)`,
  leaving near-invisible rings on the light theme's white cards; it now uses theme tokens.
- Purging a file now also deletes its cached thumbnail and preview, instead of leaving them behind.

### Removed
- Settings rows that told the user nothing actionable: **Server Status**, **Server URL**,
  **Folder Paths**, and the **Backend URL** input. `ServerSetup` is still the place to point the app
  at a different backend.
- **Zoom Sync** — a prototype leftover wired to nothing; there is no zoom feature for it to sync and
  no cross-device state channel. Its toggle CSS went with it.
- The Gallery's **Select** button, which had no multi-select or bulk action behind it.
- **Env-file clutter.** `.env`/`.env.example` are down to the five values you must actually set;
  anything that only repeated a server default (`EASYCLEAN_TRASH_RETENTION_DAYS`, `RUST_LOG`) is
  documented as an optional override instead of shipped pre-set. Dropped
  `EASYCLEAN_CORS_ORIGINS` from `.env.example`, which the local compose file never forwarded, so it
  silently did nothing. Removed `EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD` from the production compose
  and env example now that duplicate sensitivity is a per-user setting — the server still uses it as
  the first-run default, and the README now spells out which settings are operator-level (env) and
  which are per-user (Settings screen).

## [0.1.0] — 2026-09-13

First release deployed to the home server.

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
- **CD** (`deploy.yml`) — pushing a `v*` tag promotes that commit to production: it re-runs the CI
  gate on the tagged commit, publishes `ghcr.io/<owner>/easyclean` at `:<version>`, `:<major>.<minor>`,
  `:sha-<commit>` and `:latest`, then rolls it out to the home server through a self-hosted runner,
  with a health check and automatic rollback to the previous tag. Pushes to `main` are gated but
  deploy nothing; a manual run with an existing tag rolls back without rebuilding. Replaces the
  separate `release.yml`, which built the same image on the same trigger.
- **`deploy/` production kit** for `/opt/easyclean`: GHCR-based compose file, `.env.example`,
  `install.sh`, and a runbook covering runner setup, rollback, backup, and security.
- **Scale benchmark** (`core/examples/bench.rs`) — generates a synthetic library and times scan,
  detection, per-screen queries, and incremental rescan.

### Changed
- **Brand mark** — the isometric "e" logo replaces the placeholder broom glyph and the generated teal
  sparkle icon. Traced from the source artwork to a single outline in `web/src/brand.ts`, which the
  React icon and `scripts/generate-icons.mjs` both read, so the in-app logo, favicon and PWA icons
  can't drift apart. In-app it's coloured by its container and inverts with the theme; the app icons
  are a white glyph on a true-black tile.
- **Login screen redesign** — ambient gradient backdrop, logo tile, large heading, labeled fields, a
  passcode reveal toggle, and a footer note, in both themes. `ServerSetup` shares the same shell.
- **Friendlier auth errors.** `ApiError` now carries the HTTP status and unwraps the server's
  `{"error": …}` body, so a rate-limited login shows "Too many attempts. Wait 15 minutes and try
  again." instead of raw JSON, and a bad passcode reads "Incorrect username or passcode."
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
- **The Tauri desktop shell.** The `desktop/` crate, its workspace member, its `release.yml` bundle job,
  and the `--exclude easyclean-desktop` flags in CI are gone; `Cargo.lock` drops 312 packages
  (660 → 348). EasyClean now ships one way — the Docker image — and the backend is only ever built
  inside it. The code remains in git history at `da8f7f5`.
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

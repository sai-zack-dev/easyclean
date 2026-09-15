# Changelog

All notable changes to EasyClean are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.2.1] — 2026-09-15

Phone-testing fixes: duplicates you can actually compare, date decks that filter by date, and an
installed PWA that picks up a deploy.

Patch rather than minor: nothing changes shape for existing data — no migration, no schema change,
and every endpoint that existed still answers the same way.

### Added
- **Duplicates get their own compare screen.** Reviewing copies in the one-card swipe deck asked
  "keep this?" about a photo whose only problem was that another copy existed — and never showed you
  the other copy. Now a whole cluster is on screen at once: tap the copies you mean, and one button
  settles the group. Selected go one way, the rest go the other, which covers every case in a single
  gesture — keep both, delete both, or split a pair. The largest copy is pre-selected and marked
  BEST (more bytes means less re-compression, which is how the original differs from the one that
  came back through a chat app); an eye button enlarges either copy when the difference is in the
  detail. Undo takes back a whole group, and Skip defers one.

  New endpoints: `GET /api/media/duplicates` returns whole clusters (never a group of one), and
  `POST /api/media/duplicates/resolve` settles or undoes one in a single call — a half-decided
  cluster would simply come back on the next pass.
- **Confirmation before anything is destroyed.** Delete in the photo viewer, Delete forever in the
  trash, and Empty trash each ask first, showing the photo in question and saying plainly whether it
  is recoverable. Empty trash was a two-tap EMPTY → SURE? in the same spot, which is no protection
  against the mis-tap it was meant to catch; Cancel now takes the focus, so the keyboard's easy path
  is the safe one too.
- **An update path for the installed PWA.** The app checks for a new build on launch, whenever it is
  resumed, and hourly, and offers a banner plus a Setting › About row when one is ready. Setting also
  shows the app's version beside the one the server reports, so "did my deploy reach this phone?" is
  answerable without reinstalling.

### Fixed
- **On This Day showed recent photos, not photos from this day.** It and Recents both fell through to
  the unfiltered `timeline` query, so the two cards showed the same deck as each other and as
  everything else. They are now real batch modes: `onthisday` matches this calendar day in any year,
  `recents` the last seven days.
- **The dashboard's On This Day badge was the day of the month.** It read as a count and never agreed
  with the deck behind it — 15 on the card, 200 in the session. Every Clean card now badges the
  number of photos actually waiting, and Duplicates reads "N groups · M files".
- **The Gallery's scope tabs slid under the title when scrolled.** They stuck to a hardcoded header
  height that was shorter than the real header. Header and tabs now pin as one block, so nothing has
  to know anyone's height.
- **The photo viewer was dark in light mode.** Its chrome, sheets and ground are theme-aware.
- **Albums had no subtitle**, so "Collections" — the first section heading — read as the page's
  summary. It now says "N collections", matching the Gallery's "N items".
- **A deploy never reached an installed PWA.** Three separate causes: the server sent no cache
  headers, so the browser applied its own freshness to `index.html` and `sw.js` (now `no-cache`,
  with content-hashed `/assets/` marked immutable for a year); the service worker called
  `skipWaiting()` during install, swapping itself under a page still running the old JavaScript and
  leaving no waiting worker for the app to notice; and nothing ever called `registration.update()`,
  which an app that is resumed rather than reloaded needs. Deleting and reinstalling the app was the
  only way out.

## [0.2.0] — 2026-09-15

Semantic search, a real photo viewer, and a Gallery that holds the whole library.

Minor rather than patch: this adds a database migration and changes what the Gallery shows.

### Added
- **Semantic photo search.** Type what a photo *shows* — "a crane at a harbour", "green rolling
  hills" — and find it regardless of its filename. CLIP ViT-B/32 (int8 ONNX) embeds every photo and
  your query into the same 512-dimension space; the closest vectors are the results.

  One search box, two kinds of answer: exact filename/album matches first (that search always works),
  then photos that merely look like the query, de-duplicated against the first group.

  Design notes, since each was a real decision:
  - **No vector database.** Benchmarked on this stack: an exact scan of 100K vectors is 15ms (int8),
    less than the 23ms the text encoder spends on the query itself. Qdrant or Milvus would add a
    second service and a sync problem to optimise the cheap half, and would break the locked
    "one core, one shell" architecture. Vectors are int8 BLOBs in the existing SQLite.
  - **Embeddings come from the cached thumbnail**, not the original — CLIP downsamples to 224x224
    regardless, so re-decoding a 12MB HEIC would be pure waste. Indexing measured 27.6ms per photo.
  - **Weights download on first use** (~149MB) into `<data_dir>/models/`, so the image grew only
    48MB (ONNX Runtime, statically linked) and the Docker build stays offline and reproducible.
    `EASYCLEAN_CLIP_BASE_URL` retargets it at a mirror.
  - **The relevance cut-off is relative.** CLIP similarities bunch in 0.15-0.30 whatever you ask, so
    an absolute threshold is meaningless. Results are kept by z-score against the library's own
    distribution for that query: subjects actually present measured z = 2.3-2.7, while gibberish and
    absent-but-valid subjects ("a giraffe in a spacesuit") measured 1.5-1.6. Below z=2 results are
    still shown, labelled "closest guesses" — a ranking that silently returns nothing is worse than
    one that admits it is unsure.

  New endpoints: `GET /api/search`, `GET /api/search/status`, `POST /api/search/index`. Settings
  gains a Photo Search panel with the download/index progress.
- **Pinch-to-zoom and swipe navigation in the photo viewer.** Two fingers zoom (up to 6x, double-tap
  toggles), one finger pans when zoomed, swipes left/right between photos when not, and swipes down
  to dismiss with the backdrop fading as it goes. The `<` `>` arrows are gone; arrow keys still work.
  Only the photo transforms — the top bar and action bar stay pixel-identical at full zoom, and the
  browser's own pinch stays disabled.
- **Viewer action bar**, shaped like the tab bar it replaces while the viewer is open: Share, Info,
  Album, Delete. In the trash it becomes Put back, Info, Delete forever, and the favourite button is
  hidden — favouriting is a review signal that means nothing for a binned file. Share uses the Web
  Share API where the browser offers it and otherwise downloads the original; on plain-HTTP LAN the
  API is unavailable (it needs a secure context), so the download is what actually runs today.
- **Gallery search runs on the server, across the whole library** (`q=`), matching the filename and
  the path a photo is filed under — so an album name finds its contents. It replaces a browser-side
  filter that could only see the pages already scrolled through: on a 213-photo fixture with 120
  loaded, searching now finds a file sitting at row ~213. Typing is debounced, so a six-letter word
  is one request. `%` and `_` are escaped rather than left to act as LIKE wildcards — searching
  `IMG_2203` no longer also matches `IMG-2203`.
- **Photos / Videos filter** (`type=`) beside the Gallery tabs, toggling off when tapped again. An
  unknown type is a 400 rather than a silent empty result.
- **Gallery tabs — All / Original / Cleaned** (`mode=library|original|cleaned`), so un-reviewed
  photos and filed ones can be told apart at a glance.
- **Manual album filing** (`POST /api/albums/add`): file a photo under an existing album or a new one
  by name, alongside the automatic categorisation that runs at session finish.
- **Bin a photo straight from the Gallery or viewer** (`POST /api/media/:id/trash`) — a swipe only
  records a decision for session finish, and there is no session to finish here. Still trash-first.
- **Days-left countdown in the trash**, turning red in the last three days, with `trash_retention_days`
  now exposed on `/api/settings`.

### Changed
- **Page headers are sticky.** Clean, Gallery, Albums, Setting and album detail keep their title bar
  pinned while the content scrolls, with a hairline appearing once something has scrolled under it.
- **The trash is ordered by when each file was binned**, not when the photo was taken — a 2019 photo
  deleted a minute ago belongs at the top, and `created_at` buried it. Rows open the viewer.

### Fixed
- **The Gallery stopped at 500 photos.** `/api/media/batch` had no pagination, so the Gallery asked
  for the server's maximum page and showed only that. It now pages (`page`, 1-based) and loads more as
  you scroll. The ordering gained an `id` tie-break: without it two photos sharing a timestamp were
  free to swap places between pages, so one could appear twice and another never.
- **The page scrolled behind the open photo viewer.** The lock set `body { overflow: hidden }`, but
  the scrolling element is `.screen` — the app is a flex column that owns its own overflow — so it
  did nothing. A class on `<body>` now freezes whichever `.screen` is mounted.
- **A pinch behaved like a one-finger drag.** `setPointerCapture` ran before the pointer was
  registered and throws `NotFoundError` for a pointer the browser no longer considers active, which
  aborted the handler before the two-finger state was set up. It now runs last, inside a try/catch.
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

# EasyClean — Specification

Architecture + API. Stack is locked (Rust-only — see [CLAUDE.md](CLAUDE.md)). Product/UX and the design
system are in [ui.md](ui.md).

## Architecture — one core, two shells

```
              ┌──────────────────────────────────────┐
              │  CORE — Rust (axum) server            │
              │  • serves React build (static)        │
              │  • JSON + SSE API                     │
              │  • scan · thumbs · blur · dedupe ·     │
              │    albums · trash · stats             │
              │  • owns SQLite + the media folders    │
              │  • binds 0.0.0.0:PORT                  │
              └───────────────┬──────────────────────┘
                              │
                              ▼
                  ┌────────────────────┐
                  │ Shell — Docker      │
                  │ (Linux home server) │
                  │ same core, headless │
                  │ media path + port   │
                  │ via compose/env     │
                  └─────────┬──────────┘
                            ▼
                   browsers on the LAN
                 phones · tablets · laptops
```

The cleaning logic is built **once** (a Rust core crate + an axum server binary). The Docker image is the
only launcher around it — no second backend, and the backend is never built outside the image.

> The Tauri desktop shell (Shell A: folder pick · Start/Stop · URL + QR) was **removed on 2026-09-09**.
> Its code is in git history at `da8f7f5`. Current scope: Dockerfile for the backend, `web/` for the
> frontend.

## Storage model

Per user, under a media root:

```
<media_root>/<user>/
  source/    # originals to review
  clean/     # kept + organized  (physical albums)
  .trash/    # deleted, retained TRASH_RETENTION_DAYS
```

Cleaning **moves** files: kept → `clean/` (organized into album subfolders), deleted → `.trash/`.
The media root is mounted read-write. SQLite (in the app data dir) holds metadata + indexes.

## Data model (SQLite)

- `media_file(id, user, path, rel_path, filename, media_type, size_bytes, created_at, phash,
  blur_score, is_blurry, status[pending|keep|trashed], trashed_at, group_id, gps_lat, gps_lon,
  is_favorite, thumb_path)`
- `album(id, user, name, category_type, cover_file_id, file_count, created_at)`
- `album_file(album_id, file_id)`
- `user(username, pass_hash, role[admin|user])`
- *embeddings table — later (CLIP).*

## Detection

- **Blur:** grayscale → Laplacian variance; `< BLUR_THRESHOLD` ⇒ blurry. Threshold user-configurable.
- **Duplicates:** pHash (`img_hash`) grouped by **Hamming distance ≤ threshold** (not exact-match).
  "Duplicate Sensitivity" slider in Settings maps to this threshold.
- **CLIP semantic search — BUILT:** CLIP ViT-B/32 (int8 ONNX) via `ort`, statically linked. The
  vision encoder embeds each photo *from its cached thumbnail* (CLIP downsamples to 224x224 anyway,
  so re-decoding the original is pure waste); the text encoder embeds the query into the same space.
  512-dim, L2-normalised, stored int8 + per-vector scale in `media_embedding`.

  **No vector database.** An exact scan of 100K vectors takes ~15ms — less than the ~23ms the text
  encoder spends on the query — so an ANN index would add a dependency to optimise the cheap half.
  Weights (~149MB) download on first use into `<data_dir>/models/`, so the image stays lean and the
  Docker build stays offline; `EASYCLEAN_CLIP_BASE_URL` points it at a mirror.

  **Ranking cut-off is relative, not absolute.** CLIP similarities bunch in 0.15-0.30 regardless of
  the query, so results are kept by z-score against the library's own distribution for that query.
  Measured: subjects actually present score z = 2.3-2.7; gibberish and absent-but-valid subjects
  ("a giraffe in a spacesuit") score 1.5-1.6. Below z=2 the answer is returned but labelled a guess.

## API (HTTP/JSON; auth on all data routes, scoped to the user)

```
POST /api/auth/login   {user, passcode} -> {token}      GET /api/auth/me
POST /api/scan         (async)                           GET /api/scan/status   (SSE)
GET  /api/media/batch?mode=timeline|blurs|duplicates|library|original|cleaned
                       &month=&size=&page=&q=&type=  # page 1-based; q/type filter in SQL
POST /api/media/swipe  {file_id, action: "keep"|"delete"|"undo"}
POST /api/media/session/finish     # move kept -> clean/, run categorization
GET  /api/media/{id}/thumb         # 480px grid thumbnail
GET  /api/media/{id}/preview       # 1600px view-quality JPEG, cached in <data>/previews/
GET  /api/media/{id}/raw           # the original bytes
GET  /api/albums       GET /api/albums/{id}/files?page=&size=    POST /api/albums/refresh
GET  /api/trash        POST /api/trash/restore/{id}    DELETE /api/trash/purge[/{id}]
GET  /api/stats/summary            GET /api/stats/streak
GET  /api/stats/timeline           GET /api/stats/storage?days=30
GET  /api/settings     POST /api/settings  {duplicate_distance}   # per-user; re-clusters on save
GET  /api/search?q=&mode=&type=    # name matches + "looks like", merged and de-duplicated
GET  /api/search/status            # model present? how much indexed? build progress
POST /api/search/index             # download weights if needed, then embed what's missing
POST /api/media/{id}/favorite      # toggle
POST /api/media/{id}/trash         # bin now (Gallery/viewer), not a swipe
POST /api/albums/add   {file_id, album_id | name}   # manual filing
```

`mode` scopes `media/batch`. Review modes (`timeline`, `blurs`, `duplicates`) return only `pending`
files — a swipe deck must not re-offer a decided photo. The Gallery scopes back its All / Original /
Cleaned tabs: `library` is everything but the trash, `original` is un-reviewed (`pending`), `cleaned`
is kept. All of them page with `page`, so the Gallery holds a library of any size.

`q` matches the filename and the path inside the library (so an album name finds what's filed under
it), and `type` narrows to `image`/`video`. Both are applied by SQLite across the *whole* library —
filtering in the browser would only ever have searched the pages already scrolled through. `%` and
`_` in a query are escaped, so they match literally instead of acting as LIKE wildcards.

Per-user settings live in `user_setting` and override the matching `EASYCLEAN_*` default. Saving
`duplicate_distance` re-groups from the stored perceptual hashes, so sensitivity changes never
re-decode the library.

Every data route: authenticated + path-validated (no `..`, must resolve inside the user's root).

## Networking & access

- Bind `0.0.0.0:PORT` (default `8080`). Reach it at `http://<server-ip>:<port>` from any device on the
  LAN.
- Optional nicety: mDNS (`easyclean.local`).
- **HTTP** is fine on the LAN. **Remote access later:** Tailscale or Cloudflare Tunnel (auto-HTTPS, no
  port-forwarding). Never expose the port directly to the internet.
- **Auth:** passcode/login required even on LAN; rate-limit login attempts.

## Docker (Linux server)

```yaml
services:
  easyclean:
    image: easyclean:latest          # the Rust core, headless
    ports: ["8080:8080"]
    volumes:
      - /path/to/media:/media         # read-write (files move to clean/ and .trash/)
      - ./data:/app/data              # SQLite + thumbnail cache
    environment:
      EASYCLEAN_MEDIA_ROOT: /media
      EASYCLEAN_PORT: "8080"
      EASYCLEAN_TRASH_RETENTION_DAYS: "30"
    restart: unless-stopped
```

`docker compose up -d` → browse `http://<server-ip>:8080`.

## Config (env / app settings)

`EASYCLEAN_MEDIA_ROOT`, `EASYCLEAN_PORT`, `TRASH_RETENTION_DAYS`, `BLUR_THRESHOLD`,
`DUPLICATE_DISTANCE_THRESHOLD`, users + passcodes, default theme/accent.

## Out of MVP scope

CLIP semantic search · native mobile (PWA first) · remote hosting / multi-tenant · cloud upload.

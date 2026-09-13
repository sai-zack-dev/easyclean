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
- **CLIP semantic search — LATER:** `ort` + a CLIP ONNX model, 512-dim embeddings, cosine similarity
  ("find photos of X" + visually-similar). Out of MVP.

## API (HTTP/JSON; auth on all data routes, scoped to the user)

```
POST /api/auth/login   {user, passcode} -> {token}      GET /api/auth/me
POST /api/scan         (async)                           GET /api/scan/status   (SSE)
GET  /api/media/batch?mode=timeline|blurs|duplicates|library&month=&size=
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
POST /api/media/{id}/favorite      # toggle
```

`mode` scopes `media/batch`: every review mode returns only `pending` files (a swipe deck must not
re-offer a decided photo), while `library` — which backs the Gallery — spans pending and kept alike
and hides only the trash.

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

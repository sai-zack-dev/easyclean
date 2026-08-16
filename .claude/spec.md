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
            ┌─────────────────┴──────────────────┐
            ▼                                      ▼
  ┌────────────────────┐                ┌────────────────────┐
  │ Shell A — Tauri     │               │ Shell B — Docker    │
  │ desktop (Mac/Win/   │               │ (Linux home server) │
  │ Linux): folder      │               │ same core, headless │
  │ pick · Start/Stop · │               │ media path + port   │
  │ show URL + QR       │               │ via compose/env     │
  └────────────────────┘                └────────────────────┘
            └───────────────► browsers on the LAN ◄───────────┘
                          phones · tablets · laptops
```

The cleaning logic is built **once** (a Rust core crate + an axum server binary). The desktop app and the
Docker image are two launchers around it — no second backend.

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
GET  /api/media/batch?mode=timeline|blurs|duplicates&month=&size=
POST /api/media/swipe  {file_id, action: "keep"|"delete"}
POST /api/media/session/finish     # move kept -> clean/, run categorization
GET  /api/media/{id}/thumb         GET /api/media/{id}/raw
GET  /api/albums       GET /api/albums/{id}/files?page=&size=    POST /api/albums/refresh
GET  /api/trash        POST /api/trash/restore/{id}    DELETE /api/trash/purge[/{id}]
GET  /api/stats/summary            GET /api/stats/streak
POST /api/media/{id}/favorite      # toggle
```

Every data route: authenticated + path-validated (no `..`, must resolve inside the user's root).

## Networking & access

- Bind `0.0.0.0:PORT` (default `8080`). Detect the primary LAN IPv4; the desktop shell shows
  `http://<ip>:<port>` + a QR code; "open in browser".
- Optional nicety: mDNS (`easyclean.local`).
- **HTTP** is fine on the LAN. **Remote access later:** Tailscale or Cloudflare Tunnel (auto-HTTPS, no
  port-forwarding). Never expose the port directly to the internet.
- **Auth:** passcode/login required even on LAN; rate-limit login attempts.

## Desktop app (Tauri)

- First run: pick the media folder (becomes the per-user root).
- Start/Stop the server; show status, the LAN URL, and a QR; open in browser.
- Runs the same axum core (in-process or as a bundled sidecar binary).
- Bundles: macOS `.app` (notarized), Windows `.msi/.exe` (signed), Linux AppImage/deb (optional).

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

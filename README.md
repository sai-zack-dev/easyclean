# EasyClean

Self-hosted photo/video cleaner. Point it at a folder, it serves a web UI on your LAN, and you swipe to
keep/delete — with automatic blur detection, duplicate detection, and album organization. Any device on
the same Wi-Fi can use it from a browser.

A single Rust server (axum) does all the work and serves the web UI + JSON API. It ships as a Docker
image: set your media path, `docker compose up -d`, and browse the port from any device on the Wi-Fi.

---

## Running it (Docker)

Requires Docker + Docker Compose.

```bash
cp .env.example .env
# edit .env: set MEDIA_ROOT and a passcode.
# EASYCLEAN_SECRET is optional — the server generates and persists one.

docker compose up -d --build
```

Open **http://&lt;server-ip&gt;:8080** from any device on the LAN and sign in with
`EASYCLEAN_USER` / `EASYCLEAN_PASSCODE`.

The image bundles **ffmpeg** (video thumbnails) and **libheif + libde265** (HEIC/HEIF decode, so iPhone
photos get thumbnails, blur scores, and duplicate hashes like anything else). Migrations are embedded;
the SQLite DB, thumbnail cache, and generated JWT secret live in the `./data` volume.

> **Deploying for real?** [`deploy/README.md`](deploy/README.md) covers the production setup at
> `/opt/easyclean`: the GHCR image, a self-hosted GitHub Actions runner, and rollout with health
> check + rollback. Shipping a version is `git push origin v0.3.1` — pushing a tag deploys it.

### Media folder layout

`MEDIA_ROOT` contains one subfolder per user; the server creates `source/ clean/ .trash/` inside each:

```
$MEDIA_ROOT/
└── admin/
    ├── source/   ← media to review (drop your photos here, or any subfolders)
    ├── clean/    ← kept + organized into albums
    └── .trash/   ← deleted, retained until retention purge
```

The scanner walks `source/` recursively. The media volume is mounted **read-write** — EasyClean *moves*
files between these folders, so it cannot be read-only.

### Common commands

```bash
docker compose logs -f                  # follow logs
docker compose down                     # stop
docker compose up -d --build            # rebuild after code changes
docker compose down && rm data/easyclean.db && docker compose up -d   # wipe DB + rescan
```

---

## Configuration (`EASYCLEAN_*`)

| Variable | Default | Purpose |
|---|---|---|
| `EASYCLEAN_MEDIA_ROOT` | `./media` | Root containing per-user folders |
| `EASYCLEAN_DATA_DIR` | `./data` | SQLite DB + thumbnail cache |
| `EASYCLEAN_PORT` | `8080` | HTTP port (binds `0.0.0.0`) |
| `EASYCLEAN_USER` | `default` | Seeded admin username |
| `EASYCLEAN_PASSCODE` | **required** | Admin passcode. There is no default — the server refuses to create a first account without one. Re-applied on every boot, so editing it and restarting is how you change it |
| `EASYCLEAN_USERS` | — | Multi-user: `user:pass[:role],user2:pass2`. A passcode may contain `:`; `role` is `admin` or `user` and is only read as one when it is the final field |
| `EASYCLEAN_SECRET` | *generated* | JWT signing secret, minimum 32 characters (shorter is refused). Empty ⇒ a random key is generated into `<data>/secret.key` (0600) and reused |
| `EASYCLEAN_CORS_ORIGINS` | — | Extra allowed browser origins (comma-separated). Empty ⇒ same-origin only |
| `EASYCLEAN_BLUR_THRESHOLD` | `100.0` | Laplacian-variance blur cutoff |
| `EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD` | `10` | Max pHash Hamming distance for duplicates. **First-run default only** — each user sets this from Settings → Duplicate Sensitivity, which overrides it |
| `EASYCLEAN_TRASH_RETENTION_DAYS` | `30` | Days in `.trash` before auto-purge |
| `EASYCLEAN_CLIP_BASE_URL` | Hugging Face, pinned revision | Where semantic-search weights are fetched from. Point at a mirror for an air-gapped install — the pinned SHA-256 of each file still applies, so a mirror must serve identical bytes |

Settings live in two places, on purpose:

- **Environment variables** are *operator* settings — where the media is, what port to bind, who gets
  seeded, how long the trash is kept. They apply to the whole server and need a restart.
- **The app's Settings screen** holds *per-user* preferences, stored in the database and applied
  immediately. Duplicate Sensitivity is the first of these; the env var above only decides what a
  brand-new user starts on.

---

## Security

EasyClean is **LAN-only by default** and is not hardened for direct internet exposure.

- **The JWT secret is never a shared default.** Set `EASYCLEAN_SECRET` yourself, or let the server
  generate a random one into `<data_dir>/secret.key` on first boot and keep it. Deleting the data dir
  rotates it and logs everyone out.
- **CORS is closed by default** — only the origin serving the UI can call the API. Open it with
  `EASYCLEAN_CORS_ORIGINS` only for a split dev setup.
- **Change the passcode** — anyone on the Wi-Fi who reaches the port can otherwise see and delete photos.
  Logins are rate-limited to 5 attempts / 15 min per IP.
- **Don't port-forward.** For remote access use Tailscale or a Cloudflare Tunnel (planned), never a
  router port-forward.
- Deletions are **trash-first** — files move to `.trash/` and are only purged on explicit action or after
  the retention window. That is a delay, not a backup.

---

## First use

1. Open the app → **Clean** tab → tap **Quick Scan**.
2. The server indexes all media and runs blur + duplicate detection (progress streams live).
3. Open **Duplicates**, **Blurs**, or a month in the Timeline Checklist.
4. Swipe to keep/delete; finish the session to move kept files into `clean/` and build albums.

---

## Project layout

```
core/      Rust engine (scan, decode, thumbnails, blur/dup detection, file ops, albums, DB)
server/    axum HTTP server (REST + SSE; serves the web UI)
web/       React 18 + Vite + TS PWA (built → web/dist, served by the server)
deploy/    Production kit for /opt/easyclean (compose + install.sh + runbook)
Dockerfile, docker-compose.yml   Build-from-source deployment
```

### Development

The backend is built and run through Docker — no Rust toolchain needed on your machine. The frontend
runs on Node directly.

```bash
docker compose up -d --build          # backend + bundled web UI
cd web && npm run lint && npm run build

# With a local Rust toolchain (optional — CI runs these on every push):
cargo test --workspace                # 36 tests
cargo clippy --workspace --all-targets -- -D warnings

# Scale check — generates synthetic photos and times scan/detect/queries
cargo run --release -p easyclean-core --example bench -- 10000

# Regenerate the PWA icon set after changing the mark
cd web && node scripts/generate-icons.mjs
```

HEIC support is a cargo feature (`--features heic`) because it links system libheif ≥ 1.17; the Docker
image always builds with it. Without it, `.heic` files are indexed but not decoded.

See `.claude/spec.md` (API), `.claude/ui.md` (design system), `.claude/checklist.md` (build plan).

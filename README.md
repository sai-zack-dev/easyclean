# EasyClean

Self-hosted photo/video cleaner. Point it at a folder, it serves a web UI on your LAN, and you swipe to
keep/delete — with automatic blur detection, duplicate detection, and album organization. Any device on
the same Wi-Fi can use it from a browser.

**One core, two shells:** a single Rust server (axum) does all the work and serves the web UI + JSON API.
Ship it two ways:

- **Docker** — for a Linux home server. `docker compose up -d`, browse the port.
- **Desktop app** (Tauri, Mac/Windows/Linux) — a control panel to pick a folder, Start/Stop the server,
  and show the LAN URL + QR code for phones.

---

## Option A — Docker (Linux home server)

Requires Docker + Docker Compose.

```bash
cp .env.example .env
# edit .env: set MEDIA_ROOT, a passcode, and a secret:
#   EASYCLEAN_SECRET=$(openssl rand -hex 32)

docker compose up -d --build
```

Open **http://&lt;server-ip&gt;:8080** from any device on the LAN and sign in with
`EASYCLEAN_USER` / `EASYCLEAN_PASSCODE`.

The image bundles **ffmpeg** (video thumbnails) and **libheif** runtime. Migrations are embedded; the
SQLite DB and thumbnail cache live in the `./data` volume.

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

## Option B — Desktop app (Mac / Windows / Linux)

A small control-panel window that supervises the same server.

**Run from source (dev):**
```bash
cargo build --release -p easyclean-server   # the desktop app spawns this binary
(cd web && npm ci && npm run build)         # build the UI it serves (needs Node 20.19+ / 22)
cargo run -p easyclean-desktop
```

Pick a media folder → **Start** → scan the QR code with your phone (same Wi-Fi) or click **Open in
Browser**. The app generates and persists a random JWT secret on first run.

**Build installers:**
```bash
cargo install tauri-cli --version '^2'
cargo tauri icon desktop/icons/icon.png     # generate the full macOS/Windows icon set
cargo tauri build                           # → desktop/target/release/bundle/
```

Code-signing / notarization needs your own Apple Developer / Windows certificates — see
[Tauri's signing docs](https://tauri.app/distribute/). Unsigned bundles run locally but warn on download.

---

## Configuration (`EASYCLEAN_*`)

| Variable | Default | Purpose |
|---|---|---|
| `EASYCLEAN_MEDIA_ROOT` | `./media` | Root containing per-user folders |
| `EASYCLEAN_DATA_DIR` | `./data` | SQLite DB + thumbnail cache |
| `EASYCLEAN_PORT` | `8080` | HTTP port (binds `0.0.0.0`) |
| `EASYCLEAN_USER` | `default` | Seeded admin username |
| `EASYCLEAN_PASSCODE` | `easyclean` | Seeded admin passcode |
| `EASYCLEAN_USERS` | — | Multi-user: `user:pass[:role],user2:pass2` |
| `EASYCLEAN_SECRET` | *dev default* | JWT signing secret — **set this** |
| `EASYCLEAN_BLUR_THRESHOLD` | `100.0` | Laplacian-variance blur cutoff |
| `EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD` | `10` | Max pHash Hamming distance for duplicates |
| `EASYCLEAN_TRASH_RETENTION_DAYS` | `30` | Days in `.trash` before auto-purge |

---

## Security

EasyClean is **LAN-only by default** and is not hardened for direct internet exposure.

- **Always set `EASYCLEAN_SECRET`** (`openssl rand -hex 32`). The built-in dev default is public, so
  tokens signed with it are forgeable.
- **Change the passcode** — anyone on the Wi-Fi who reaches the port can otherwise see and delete photos.
- **Don't port-forward.** For remote access use Tailscale or a Cloudflare Tunnel (planned), never a
  router port-forward.
- Deletions are **trash-first** — files move to `.trash/` and are only purged on explicit action or after
  the retention window.

---

## First use

1. Open the app → **Clean** tab → tap **Quick Scan**.
2. The server indexes all media and runs blur + duplicate detection (progress streams live).
3. Open **Duplicates**, **Blurs**, or a month in the Timeline Checklist.
4. Swipe to keep/delete; finish the session to move kept files into `clean/` and build albums.

---

## Project layout

```
core/      Rust engine (scan, thumbnails, blur/dup detection, file ops, albums, DB)
server/    axum HTTP server (REST + SSE; serves the web UI)
web/       React 18 + Vite + TS frontend (built → web/dist, served by the server)
desktop/   Tauri control-panel app (spawns the server, shows LAN URL + QR)
Dockerfile, docker-compose.yml   Linux home-server deployment
```

See `.claude/spec.md` (API), `.claude/ui.md` (design system), `.claude/checklist.md` (build plan).

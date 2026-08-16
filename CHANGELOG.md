# Changelog

All notable changes to EasyClean are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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

### Security
- Docker compose requires `EASYCLEAN_SECRET` and mounts media read-write (the app moves files).
- README documents LAN-only posture, passcode/secret requirements, and trash-first deletion.

### Notes
- HEIC decode and CLIP semantic search remain deferred (post-MVP).

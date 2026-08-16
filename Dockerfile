# syntax=docker/dockerfile:1
# ─────────────────────────────────────────────────────────────────────────────
# EasyClean — multi-stage build
#   1. web    : build the React/Vite frontend → /web/dist
#   2. rust   : compile the axum server in release mode
#   3. runtime: slim Debian + ffmpeg (+ libheif), non-root, serves UI + API
# ─────────────────────────────────────────────────────────────────────────────

# ── 1. Web build ─────────────────────────────────────────────────────────────
FROM node:22-slim AS web
WORKDIR /web
# Install deps first so they cache independently of source changes.
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build          # → /web/dist

# ── 2. Rust build ────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS rust
WORKDIR /src
# Copy the whole workspace except the desktop crate (Tauri pulls in GTK/webkit
# system libs we don't want in a headless build). We swap in a server-only
# workspace manifest so cargo doesn't try to build `desktop/`.
COPY Cargo.lock ./
COPY core/ ./core/
COPY server/ ./server/
RUN printf '[workspace]\nresolver = "2"\nmembers = ["core", "server"]\n\n[workspace.package]\nedition = "2021"\nversion = "0.1.0"\nlicense = "MIT"\n' > Cargo.toml
# libsqlite3-sys vendors SQLite, so no system sqlite needed. Build with a cache
# mount to speed up rebuilds.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p easyclean-server && \
    cp /src/target/release/easyclean-server /usr/local/bin/easyclean-server

# ── 3. Runtime ───────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
# ffmpeg: video first-frame thumbnails + ffprobe dates.
# libheif1: runtime lib for HEIC/HEIF (forward-looking; decode lands when
#           libheif-rs is wired). ca-certificates + curl for the healthcheck.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg libheif1 ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

# Non-root user owning the data dir.
RUN useradd --system --create-home --uid 10001 easyclean
WORKDIR /app

COPY --from=rust /usr/local/bin/easyclean-server /usr/local/bin/easyclean-server
COPY --from=web  /web/dist                       /app/web/dist

# Defaults — override via compose / `-e`. MEDIA_ROOT + DATA_DIR are volumes.
ENV EASYCLEAN_MEDIA_ROOT=/media \
    EASYCLEAN_DATA_DIR=/data \
    EASYCLEAN_WEB_DIST=/app/web/dist \
    EASYCLEAN_PORT=8080 \
    RUST_LOG=info

RUN mkdir -p /media /data && chown -R easyclean:easyclean /app /media /data
USER easyclean

EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:8080/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/easyclean-server"]

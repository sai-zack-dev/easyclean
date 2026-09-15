# syntax=docker/dockerfile:1
# ─────────────────────────────────────────────────────────────────────────────
# EasyClean — multi-stage build
#   1. web    : build the React/Vite frontend → /web/dist
#   2. rust   : compile the axum server in release mode (with HEIC support)
#   3. runtime: slim Debian + ffmpeg + libheif, non-root, serves UI + API
#
# Debian *trixie* (not bookworm) throughout: libheif-rs needs libheif >= 1.17
# and bookworm only ships 1.15.
# ─────────────────────────────────────────────────────────────────────────────

# ── 1. Web build ─────────────────────────────────────────────────────────────
FROM node:22-slim AS web
WORKDIR /web
# Install deps first so they cache independently of source changes.
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
# The UI stamps itself with the workspace version so Setting can show it beside
# the one the server reports. `vite.config.ts` reads it from `../Cargo.toml`,
# which means this stage needs that one file — without it the build falls back
# to "dev" and the version row can't tell you whether a deploy landed.
COPY Cargo.toml /Cargo.toml
RUN npm run build          # → /web/dist

# ── 2. Rust build ────────────────────────────────────────────────────────────
FROM rust:1-trixie AS rust
WORKDIR /src

# libheif-dev + pkg-config: HEIC/HEIF decode (the `heic` cargo feature).
RUN apt-get update && \
    apt-get install -y --no-install-recommends libheif-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# The workspace is just core + server, so it copies in as-is.
COPY Cargo.toml Cargo.lock ./
COPY core/ ./core/
COPY server/ ./server/

# libsqlite3-sys vendors SQLite, so no system sqlite needed. Cache mounts keep
# rebuilds fast; the binary is copied out of the cached target dir immediately,
# because /src/target does not survive the mount.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p easyclean-server --features heic && \
    cp /src/target/release/easyclean-server /usr/local/bin/easyclean-server

# ── 3. Runtime ───────────────────────────────────────────────────────────────
FROM debian:trixie-slim AS runtime
# ffmpeg      : video first-frame thumbnails + ffprobe dates
# libheif1    : HEIC/HEIF container support
# libheif-plugin-libde265 : the actual HEVC decoder — without it libheif opens
#               an iPhone .heic and then fails to decode it
# curl        : HEALTHCHECK
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ffmpeg libheif1 libheif-plugin-libde265 libheif-plugin-aomdec \
        ca-certificates curl && \
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

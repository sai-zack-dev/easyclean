#!/usr/bin/env bash
#
# One-time setup of an EasyClean production host.
#
#   sudo ./deploy/install.sh                  # install into /opt/easyclean
#   sudo DEPLOY_DIR=/srv/easyclean ./deploy/install.sh
#
# Creates the deploy directory, seeds .env and docker-compose.yml, fixes
# ownership for the container's uid, and does a first `docker compose up -d`.
# Re-running is safe: an existing .env is never overwritten.
set -euo pipefail

DEPLOY_DIR="${DEPLOY_DIR:-/opt/easyclean}"
# The image runs as this uid (see Dockerfile). Host dirs must match or the
# container cannot write thumbnails, the DB, or move any files.
APP_UID=10001
APP_GID=10001

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root (sudo $0)"
command -v docker >/dev/null || die "docker is not installed"
docker compose version >/dev/null 2>&1 || die "the docker compose plugin is missing"

say "Deploy directory: $DEPLOY_DIR"
mkdir -p "$DEPLOY_DIR"

say "Installing docker-compose.yml"
install -m 0644 "$SRC_DIR/docker-compose.yml" "$DEPLOY_DIR/docker-compose.yml"

# The Tailscale sidecar and its serve config ride along unconditionally but stay
# inert: nothing starts them until COMPOSE_FILE in .env names the overlay.
install -m 0644 "$SRC_DIR/docker-compose.tailscale.yml" "$DEPLOY_DIR/docker-compose.tailscale.yml"
install -d -m 0755 "$DEPLOY_DIR/tailscale"
install -m 0644 "$SRC_DIR/tailscale/serve.json" "$DEPLOY_DIR/tailscale/serve.json"

FRESH_ENV=0
if [ -f "$DEPLOY_DIR/.env" ]; then
  say ".env already exists — leaving it alone"
else
  say "Seeding .env"
  install -m 0600 "$SRC_DIR/.env.example" "$DEPLOY_DIR/.env"
  FRESH_ENV=1
fi

# Read a single key out of .env. Deliberately not `source`: this script runs as
# root, and sourcing would execute anything in that file — plus values like a
# passcode containing spaces or `#` don't survive shell parsing the way docker
# compose reads them.
env_get() {
  sed -n "s/^$1=//p" "$DEPLOY_DIR/.env" | tail -n1
}

if [ "$FRESH_ENV" -eq 1 ]; then
  # Don't start against placeholder values: compose would create a root-owned
  # bind-mount directory for the placeholder MEDIA_ROOT and boot with the
  # published default passcode.
  say "Next: edit $DEPLOY_DIR/.env"
  say "  MEDIA_ROOT           where your photos live (must exist)"
  say "  EASYCLEAN_PASSCODE   anything but 'change-me'"
  say "  GITHUB_REPOSITORY    owner/repo the GHCR image comes from"
  say "Then re-run this script to start EasyClean."
  exit 0
fi

MEDIA_ROOT="$(env_get MEDIA_ROOT)"
DATA_DIR="$(env_get DATA_DIR)"
PORT="$(env_get PORT)"
EASYCLEAN_PASSCODE="$(env_get EASYCLEAN_PASSCODE)"

DATA_DIR="${DATA_DIR:-$DEPLOY_DIR/data}"
PORT="${PORT:-8080}"
say "Data directory: $DATA_DIR"
mkdir -p "$DATA_DIR"
chown -R "$APP_UID:$APP_GID" "$DATA_DIR"

# Tailscale sidecar enabled? Its state dir holds the node key and the HTTPS
# cert, so it is root-owned and private — tailscaled runs as root in its
# container, unlike the app.
if [ -n "$(env_get COMPOSE_FILE)" ]; then
  TS_STATE_DIR="$(env_get TS_STATE_DIR)"
  TS_STATE_DIR="${TS_STATE_DIR:-$DEPLOY_DIR/tailscale-state}"
  say "Tailscale state directory: $TS_STATE_DIR"
  mkdir -p "$TS_STATE_DIR"
  chmod 0700 "$TS_STATE_DIR"
  [ -n "$(env_get TS_AUTHKEY)" ] || die "COMPOSE_FILE enables the Tailscale sidecar but TS_AUTHKEY is empty.
       Mint a reusable, pre-authorized, tagged key at
       https://login.tailscale.com/admin/settings/keys, or remove COMPOSE_FILE from $DEPLOY_DIR/.env."
fi

if [ -n "$MEDIA_ROOT" ] && [ -d "$MEDIA_ROOT" ]; then
  say "Media root: $MEDIA_ROOT"
  # Only the top level: chown -R across a 100K-photo library is slow and
  # usually unwanted. Per-user folders are created by the app as uid 10001.
  chown "$APP_UID:$APP_GID" "$MEDIA_ROOT"
  if [ ! -w "$MEDIA_ROOT" ]; then
    say "WARNING: $MEDIA_ROOT is not writable — EasyClean moves files, it needs write access"
  fi
else
  say "WARNING: MEDIA_ROOT (${MEDIA_ROOT:-unset in .env}) does not exist — create it, then re-run"
fi

# Stop, rather than warn and carry on. The warning scrolled past on a screen
# that then printed a working URL, so the install looked finished — and the
# placeholder is published in this repo, which makes it a password anyone on the
# network can look up.
if [ -z "$EASYCLEAN_PASSCODE" ] || [ "$EASYCLEAN_PASSCODE" = "change-me" ]; then
  die "the passcode is still '${EASYCLEAN_PASSCODE:-unset}'. Anyone on your Wi-Fi could browse
       and delete your photos. Set EASYCLEAN_PASSCODE in $DEPLOY_DIR/.env, then re-run this script."
fi

say "Starting EasyClean"
cd "$DEPLOY_DIR"
docker compose pull || say "pull failed (private image? run: docker login ghcr.io)"
docker compose up -d

say "Waiting for health..."
for _ in $(seq 1 60); do
  if curl -fsS "http://localhost:$PORT/api/health" >/dev/null 2>&1; then
    IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
    say "EasyClean is up:  http://${IP:-localhost}:$PORT"
    if [ -n "$(env_get COMPOSE_FILE)" ]; then
      say "Tailscale: watch it register with  docker compose logs -f tailscale"
      say "  then confirm the name it actually took at https://login.tailscale.com/admin/machines"
    fi
    exit 0
  fi
  sleep 1
done

die "did not become healthy — check: docker compose -f $DEPLOY_DIR/docker-compose.yml logs"

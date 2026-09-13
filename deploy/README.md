# Deploying EasyClean to a Linux home server

Target layout: the app lives in `/opt/easyclean` and runs as a Docker container
from a GHCR image. **Pushing a version tag ships it** — GitHub Actions builds the
image and rolls it out through a **self-hosted runner installed on the server**.
Nothing about your server is exposed to the internet; the runner dials out to
GitHub.

Pushing to `main` runs the quality gate but changes nothing in production.
Tagging is the promote step, so what's running is always a version you chose to
ship.

```
push to main  → ci.yml (fmt·clippy·test·web·docker)          ← gate only, no deploy

push tag v*   → deploy.yml
                  ├── gate    → re-runs ci.yml on the tagged commit
                  ├── publish → ghcr.io/<owner>/easyclean:0.3.1 · :0.3 · :sha-… · :latest
                  └── rollout → self-hosted runner on the server
                                docker compose up -d + health check
                                (auto-rollback to the previous tag on failure)
```

## Shipping a release

```bash
git tag v0.3.1
git push origin v0.3.1
```

That's the whole deploy. Watch it in Actions → *deploy*. Tags must start with
`v`; the image is published under the version without it (`v0.3.1` → `:0.3.1`),
which is what the server pins in `.env`. A prerelease tag (`v0.4.0-rc1`) deploys
too but doesn't move `:latest`.

---

## 1. Prepare the server

```bash
# Docker + compose plugin
curl -fsSL https://get.docker.com | sh

# Where the photos live. Per user: <MEDIA_ROOT>/<user>/source|clean|.trash
sudo mkdir -p /srv/photos/admin/source
```

Copy this repo's `deploy/` folder to the server (or clone the repo) and run:

```bash
sudo ./deploy/install.sh
```

The first run creates `/opt/easyclean/{docker-compose.yml,.env}` and then **stops**, listing what to
fill in — it deliberately refuses to boot against the placeholder passcode.

Edit `/opt/easyclean/.env` (at minimum `MEDIA_ROOT`, `EASYCLEAN_PASSCODE`, and `GITHUB_REPOSITORY`),
then run the script again. The second pass chowns the data dir to uid `10001` (the container's user),
starts the stack, and waits for `/api/health`.

### About the volumes

| Path | Contains | Notes |
|------|----------|-------|
| `MEDIA_ROOT` → `/media` | your photos | mounted **read-write** — EasyClean *moves* files into `clean/` and `.trash/` |
| `DATA_DIR` → `/data` | SQLite index, thumbnails, `secret.key` | back this up; losing it means a rescan + logged-out sessions, never lost photos |

---

## 2. Install the self-hosted runner

On the server, as a non-root user that is in the `docker` group:

```bash
sudo usermod -aG docker "$USER" && newgrp docker

mkdir -p ~/actions-runner && cd ~/actions-runner
# Grab the current runner tarball + token from:
#   GitHub → repo → Settings → Actions → Runners → New self-hosted runner (Linux x64)
curl -o actions-runner-linux-x64.tar.gz -L <URL FROM THAT PAGE>
tar xzf actions-runner-linux-x64.tar.gz

./config.sh --url https://github.com/sai-zack-dev/easyclean \
            --token <TOKEN FROM THAT PAGE> \
            --labels easyclean \
            --name home-server \
            --unattended

sudo ./svc.sh install && sudo ./svc.sh start   # run as a systemd service
```

The `easyclean` label matters: `deploy.yml` targets
`runs-on: [self-hosted, linux, x64, easyclean]`, so the rollout can only land on
this machine.

### Give the deploy user permission to write `/opt/easyclean/.env`

The rollout job rewrites `EASYCLEAN_IMAGE_TAG` in that file:

```bash
sudo chown "$USER" /opt/easyclean/.env
sudo chmod 600 /opt/easyclean/.env
```

### Optional: require approval before a rollout

Repo → Settings → Environments → **production** → add yourself as a required
reviewer. `deploy.yml` uses `environment: production`, so every rollout then
waits for your click.

---

## 3. Everyday operation

```bash
cd /opt/easyclean

docker compose ps                     # what's running
docker compose logs -f easyclean      # follow logs
docker compose restart                # restart
```

`EASYCLEAN_IMAGE_TAG` in `.env` is what pins the running version — the rollout
job rewrites it. Don't `docker compose pull` to update by hand; that chases
whatever tag is pinned and won't move you forward. Push a tag instead.

**Roll back** from GitHub: Actions → *deploy* → *Run workflow*, and give it an
already-published tag (`0.3.0`, or `sha-abc1234` for an exact commit). That path
skips the build entirely and just re-points the server — so rolling back takes
seconds, not a rebuild. It's also how you do the very first rollout, before any
version tag exists.

**Roll back on the box itself**, if GitHub is unreachable:

```bash
sudo sed -i 's/^EASYCLEAN_IMAGE_TAG=.*/EASYCLEAN_IMAGE_TAG=0.3.0/' .env
docker compose up -d
```

Note the rollout job already backs itself out: if the new image doesn't answer
`/api/health` within 60s it restores the previous tag on its own. Reaching for
either of the above means something got past that.

**Back up** what you can't rebuild:

```bash
tar czf easyclean-data-$(date +%F).tar.gz -C /opt/easyclean data
```

---

## 4. Security notes — read before opening any port

EasyClean serves **plain HTTP** and is designed for a trusted LAN.

- **Never port-forward 8080 to the internet.** For remote access use
  [Tailscale](https://tailscale.com/) or a Cloudflare Tunnel, which give you
  HTTPS and an identity layer without exposing the port.
- **Change `EASYCLEAN_PASSCODE`.** Anyone who reaches the port and knows the
  passcode can delete photos. Login is rate-limited to 5 attempts / 15 min per
  IP, which slows guessing but does not replace a real passcode.
- **The JWT secret** is generated into `/data/secret.key` on first boot if you
  don't set `EASYCLEAN_SECRET`. Keep `/data` — a new secret logs everyone out.
- **Deleted files go to `.trash`** and are purged after
  `EASYCLEAN_TRASH_RETENTION_DAYS` (default 30). It's a delay, not a backup:
  EasyClean is not a substitute for backing up your photo library.
- **Service worker / PWA install** needs a secure context. Over
  `http://<lan-ip>:8080` the app works in the browser but isn't installable;
  over a Tailscale/Cloudflare HTTPS hostname it installs as a PWA.

---

## 5. Troubleshooting

| Symptom | Likely cause |
|---|---|
| `permission denied` writing thumbnails | `DATA_DIR`/`MEDIA_ROOT` not owned by uid `10001` — re-run `install.sh` |
| Photos scan but no thumbnails for `.heic` | check the log for `HEIF/HEIC decoding enabled`; if absent the image was built without `--features heic` |
| Videos have no thumbnail | `ffmpeg` missing — it's in the image, so suspect a corrupt/unsupported file |
| Everyone logged out after an update | `/data` volume was replaced, so `secret.key` changed |
| Rollout fails with `denied` on pull | GHCR package is private — the runner logs in with `GITHUB_TOKEN`; make sure the package's access includes this repo |
| Runner offline | `cd ~/actions-runner && sudo ./svc.sh status` |

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

### Rolling back across a database migration

A release that adds a migration changes `data/easyclean.db` in place, and a
migration cannot be un-applied. Rolling back to an image built **before
v0.1.1** will fail to start against such a database — sqlx aborts when the
database carries a migration the binary doesn't know about, so the container
crash-loops rather than recovering. From v0.1.1 on the server tolerates this
and an older image boots normally, ignoring tables it doesn't use.

The rollout job snapshots the index into `data/backups/` before every
deployment (last 10 kept). To go back to a pre-v0.1.1 image:

```bash
cd /opt/easyclean
docker compose down
ls -1t data/backups/                       # pick the snapshot from before the upgrade
cp data/backups/easyclean-<stamp>-pre-<tag>.db data/easyclean.db
sudo sed -i 's/^EASYCLEAN_IMAGE_TAG=.*/EASYCLEAN_IMAGE_TAG=0.1.0/' .env
docker compose up -d
```

Review decisions made after the upgrade are lost by that restore — the photos
themselves are untouched, since the database only holds the index.

**Back up** what you can't rebuild:

```bash
tar czf easyclean-data-$(date +%F).tar.gz -C /opt/easyclean data
```

---

## 4. Security notes — read before opening any port

EasyClean serves **plain HTTP** and is designed for a trusted LAN.

- **Never port-forward 8080 to the internet.** For remote access use
  [Tailscale](https://tailscale.com/) or a Cloudflare Tunnel, which give you
  HTTPS and an identity layer without exposing the port. Tailscale is set up
  in §5 — one `COMPOSE_FILE` line and an auth key.
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

## 5. Remote access over Tailscale

Gives EasyClean its own HTTPS address on your tailnet —
`https://saiz-easyclean.<your-tailnet>.ts.net` — reachable by you and whoever
you share it with, and by nobody else. No port forwarding, no certificate to
renew, and the LAN address keeps working exactly as before.

**It gets its own machine name on purpose.** A MagicDNS name belongs to a
machine, not a service, so a second name means a second node — here a
`tailscale` sidecar container beside the app. That is the point, not a
workaround: Tailscale shares access **one node at a time**, so sharing
EasyClean with your sister gives her EasyClean and nothing else you run.

```
  phone on 5G                     home server
┌──────────────┐            ┌──────────────────────────────────┐
│  Tailscale   │  WireGuard │  easyclean-tailscale             │
│   (family)   │═══════════▶│    tailscale serve :443 → HTTPS  │
└──────────────┘  encrypted │              │ docker network    │
                            │              ▼                   │
   laptop on LAN            │  easyclean  :8080  (plain HTTP)  │
   http://<ip>:8080 ───────▶│                                  │
                            └──────────────────────────────────┘
```

Nothing is published to the internet. This is `tailscale serve`, **not**
`tailscale funnel` — funnel would put your photo library on the public web,
which is the one thing this app is written not to do.

### 5.1 Mint an auth key

In the admin console → **Settings → Keys → Generate auth key**:

- **Reusable** — so a rebuilt container re-registers instead of needing a new key.
- **Pre-approved** — otherwise the node sits unauthorized and the URL 404s.
- **Tagged** `tag:easyclean` — *this is the one that bites later.* An untagged
  node inherits your user's key expiry and drops off the tailnet after ~6
  months; the URL then stops answering with nothing in EasyClean's logs to
  explain it. A tagged node does not expire.

Tags need an owner in your policy file before the key will issue:

```json
"tagOwners": {
  "tag:easyclean": ["autogroup:admin"]
}
```

If your policy file is still the default allow-all, that's all you need. If
you've narrowed it, add a rule reaching the tag — and include `autogroup:shared`
or the people you share with can't get in:

```json
{ "action": "accept",
  "src": ["autogroup:member", "autogroup:shared"],
  "dst": ["tag:easyclean:443"] }
```

Edit the policy file in the admin console, not here, and don't paste over rules
that `saiz-cloud` depends on.

### 5.2 Turn it on

In `/opt/easyclean/.env`:

```bash
COMPOSE_FILE=docker-compose.yml:docker-compose.tailscale.yml
TS_HOSTNAME=saiz-easyclean
TS_AUTHKEY=tskey-auth-…
TS_STATE_DIR=/opt/easyclean/tailscale
```

`COMPOSE_FILE` is read by `docker compose` itself, so from here on a plain
`docker compose up -d` in `/opt/easyclean` brings up both containers — including
the one the deploy workflow runs on the self-hosted runner. Rollouts and
rollbacks keep the tunnel up without knowing it exists.

```bash
cd /opt/easyclean
sudo ./install.sh          # or: docker compose up -d
docker compose logs -f tailscale
```

Then confirm at <https://login.tailscale.com/admin/machines> that the node came
up as `saiz-easyclean` — Tailscale lowercases the name and, if it is already
taken, quietly appends `-1`, which changes your URL.

```bash
curl -fsS https://saiz-easyclean.<tailnet>.ts.net/api/health
```

### 5.3 Letting family and friends in

Two ways, and for relatives the first is usually right:

- **Share the node** (admin console → the machine → **Share…**). They install
  Tailscale, sign in with their own account, accept the share, and reach
  `saiz-easyclean` — and only that. Their account stays theirs; you aren't
  administering it.
- **Invite them to your tailnet** as users. Right for your own household
  devices, heavier for a friend, and users count against your plan — check
  current limits before promising six people access.

Either way, **a tailnet address is not a login**. Give each person their own
EasyClean account so they get their own folders and their own trash:

```bash
EASYCLEAN_USERS=saiz:<passcode>:admin,mum:<passcode>,ben:<passcode>
```

and create `<MEDIA_ROOT>/<name>/source` for each. Reaching the URL without a
passcode still gets them a login screen.

**Keep `BIND_ADDRESS=0.0.0.0`.** At home the LAN path stays direct and fast, and
it is what you fall back to if Tailscale itself is having a bad day. Set
`127.0.0.1` only if you want the tunnel to be the *only* way in.

### 5.4 Two things the tunnel changes

- **Login rate-limiting becomes shared.** The limiter keys on the connecting IP,
  and through the proxy every remote request arrives from the sidecar — so one
  relative fat-fingering their passcode five times locks *everyone* out of login
  for 15 minutes. LAN clients are unaffected, and the vault PIN throttle is
  keyed per account, so it isn't affected either. The fix is for the server to
  trust `X-Forwarded-For` from the sidecar; until it does, this is the one rough
  edge.
- **The PWA becomes installable.** Service workers need a secure context, which
  `http://<lan-ip>:8080` isn't. Over the `ts.net` name it is — so "Add to Home
  Screen" on an iPhone now gives a real app window instead of a Safari tab.
  Tell family to open the tailnet URL, not the LAN one, when installing.

---

## 6. Troubleshooting

| Symptom | Likely cause |
|---|---|
| `permission denied` writing thumbnails | `DATA_DIR`/`MEDIA_ROOT` not owned by uid `10001` — re-run `install.sh` |
| Photos scan but no thumbnails for `.heic` | check the log for `HEIF/HEIC decoding enabled`; if absent the image was built without `--features heic` |
| Videos have no thumbnail | `ffmpeg` missing — it's in the image, so suspect a corrupt/unsupported file |
| Everyone logged out after an update | `/data` volume was replaced, so `secret.key` changed |
| Rollout fails with `denied` on pull | GHCR package is private — the runner logs in with `GITHUB_TOKEN`; make sure the package's access includes this repo |
| Runner offline | `cd ~/actions-runner && sudo ./svc.sh status` |
| `ts.net` URL doesn't resolve | the client isn't on the tailnet, or MagicDNS is off for it |
| `ts.net` URL resolves but times out | node unauthorized (key wasn't pre-approved), or an ACL doesn't reach `tag:easyclean:443` |
| Tunnel worked for months, then died | untagged node's auth key expired — re-tag it, or disable key expiry on the machine |
| URL became `saiz-easyclean-1.…` | the state dir was lost, so the node re-registered as a new machine; restore `TS_STATE_DIR` or re-share under the new name |
| `502` through the tunnel, fine on the LAN | the sidecar can't resolve `easyclean` — check `TS_ACCEPT_DNS=false` is still set |
| `429 too many login attempts` for everyone at once | one shared rate-limit bucket behind the proxy — see §5.4 |

# Changelog

All notable changes to EasyClean are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.6.0] — 2026-10-02

Minor, not patch: the viewer gained a mode it did not have. Nothing breaks, no API or configuration
changes, no new migration.

### Added
- **Slideshow.** A play button in the photo viewer opens a sheet for direction (next or previous) and
  interval (1, 3, 5 or 10 seconds), and the deck then advances on its own until it reaches either
  end. The timer is keyed off the current photo rather than looping independently, so each one gets
  its whole interval — including one a manual swipe landed on — and touching the controls stops the
  show instead of fighting it. It pauses behind a confirmation or the share sheet: those close over
  the photo that was on screen when they opened, so advancing underneath one would delete or share
  something else. Not offered in the trash, or on a deck of one.

### Changed
- **The tab bar is 58px, not 72px.** 72 was quietly standing in for the home indicator as well as the
  bar's own content. Since 0.5.1 turned `env(safe-area-inset-bottom)` on, that space is added
  properly on top — so the old number left about 20px of dead air under the icon-and-label stack on
  every screen.
- **The vault's Select and Cancel are text pills again**, matching the Gallery's, with
  `.swipe-title-wrap`'s gutter widened to 136px so a centred title clears them and the Add button.

### Fixed
- **The toast sat behind the status bar.** It was pinned 56px from the top, which after
  `viewport-fit=cover` is measured from the physical top edge rather than below the status bar — so
  on any notched phone it was mostly hidden.
- **The version the app reports.** 0.5.1 was tagged and deployed without bumping the workspace
  `Cargo.toml`, so both `/api/health` and the Settings readout have been saying `0.5.0` while running
  the 0.5.1 image. The bundle reads the same file, so the two agreed with each other and nothing
  flagged it.

## [0.5.1] — 2026-09-23

Five things reported after 0.5.0 went into daily use, all in the client or in a read-only query —
patch rather than minor: no API changes, no configuration changes, no new migration, nothing to do
before upgrading.

### Fixed
- **An upload no longer holds the app hostage.** A batch lives in its own store rather than in the
  screen that starts it, so sending 50 photos over a home link is something you set going and then
  leave — Clean, Gallery, the swipe deck and the trash all stay usable while it runs, and a pill
  above the tab bar reports progress and leads back. Closing the Add screen no longer means
  cancelling; Cancel is its own button now. The tab still has to stay open: there is no Background
  Fetch on iOS, and a service worker cannot be handed files picked in a document that has gone away,
  so a `beforeunload` guard speaks up on the platforms that will ask.

- **The upload list stopped destroying its own thumbnails.** Previews were derived from the item
  array, which the run replaces several times a second to report progress — so every tick revoked
  all sixty object URLs and minted sixty more, and the browser re-decoded every thumbnail it had
  just decoded. That is what made rows go blank and the list tear under a fast scroll. They are now
  built once when the files are picked and left alone, progress is coalesced to one render per
  frame, and each row is `contain`ed and lazily decoded so a 200-file list lays out once.

- **Streaks are counted in your calendar, not UTC's.** Both the streak and the Settings storage
  chart bucketed `timestamp / 86400`, which draws the day boundary at 06:30 in Yangon and 19:00 in
  New York. One morning's cleaning could therefore land on two different "days" and paper over a
  real gap. Both endpoints now take the client's UTC offset (`?tz=`, minutes ahead) and bucket by it.

- **The vault has a photo viewer.** Tapping a hidden photo opened a new browser tab holding a blob
  URL — the one way out of that screen that leaves the photo sitting in the app switcher next to
  Safari's other tabs. It now opens the same viewer as the Gallery, in a `vault` mode: pinch, page
  and swipe-to-dismiss as usual, with Put back and Delete forever in place of Share and Album, no
  favourite, and no album lookup. The original still arrives as a blob fetched with the vault token
  in a header, so nothing about it reaches a URL.

- **`viewport-fit=cover`, which every `env(safe-area-inset-*)` in the stylesheet was waiting for.**
  Without it iOS reports all four insets as 0, so the carefully-written padding did nothing: action
  bars sat under the home indicator, and the photo viewer's chrome was 45px taller at the top (status
  bar plus header) than at the bottom. A photo centred in the gap between the two bars is therefore
  not centred on the screen — it sits half that difference low, which is the first thing you notice
  on a short photo. The viewer now also gives the difference back to whichever end is shorter, so the
  photo's centre and the screen's are the same point.

## [0.5.0] — 2026-09-21

Minor, not patch: the server gained a way to receive files, which it never had. Nothing breaks and
nothing has to be configured — without `EASYCLEAN_QUOTAS` every account is unlimited, and the disk
floor defaults to 5 GiB. One new migration, `0009`, adding a nullable column and an index; additive,
so an automatic rollback to 0.4.4 still boots against a migrated database.

### Added
- **Uploads, resumable and verified.** Six endpoints under `/api/upload`. A client hashes its files
  first and asks `check` which ones the server already holds, so re-offering a whole camera roll next
  month transfers only what is new. Each remaining file then goes `init` → `chunk` → `finish`, with
  every chunk carrying the offset it belongs at: a dropped connection resumes from
  `GET /api/upload/:id` rather than starting the file again.

  The verification is the point. The server recomputes SHA-256 over the bytes it received and
  discards anything that does not match what was promised — the case that matters is not a liar but
  a truncated transfer, which would otherwise become a permanently broken photo with the evidence
  gone. Filenames are stripped to their last component and checked against the extensions this app
  can actually decode, so `../../etc/evil.jpg` lands in `source/` as `evil.jpg` and `payload.sh` is
  refused before a single byte moves.

  Scratch files live under `<data_dir>/uploads/`, never in the media root: a half-written file there
  cannot be scanned, indexed, thumbnailed or served, which one sitting in `source/` could be. They
  are cleared at startup and swept hourly.

- **Per-user quotas and a disk-space floor.** `EASYCLEAN_QUOTAS=lily=20GB,zack=50GB` sets ceilings;
  `EASYCLEAN_DISK_FLOOR` (default 5 GiB) refuses uploads before the disk can reach zero, because a
  full disk is not a smaller version of a working one — SQLite cannot write, and the one action that
  would recover the situation, deleting things, is also the one that stops working.

  Quotas are read live, database first and environment second, and **seeding never writes them**.
  That is deliberate: account seeding re-applies `.env` on every boot, so a quota stored the same way
  would be reverted by the next restart — the trap documented in `.claude/auth-redesign-brief.md`.
  The admin UI can therefore write quotas today, before that brief's work lands, and a restart will
  not undo it. A user cannot raise their own ceiling: `settings_put` still accepts exactly one field,
  and there is a test that says so.

- **Uploads process themselves.** A finished upload is indexed immediately, then a debounced pass
  five seconds after the *last* file in a batch runs thumbnails, blur scoring and duplicate hashing —
  one pass per batch, not one per photo. Semantic search follows automatically where the CLIP weights
  are already present; the first ~150 MB download stays an explicit choice in Settings rather than
  something a photo upload triggers on a home connection.

### Changed
- `media_file` gains `content_hash` (migration `0009`), written only by uploads. Deliberately not
  `phash`: that column is perceptual and exists to match photos that merely *look* alike, which is
  the opposite of what "have I already received these exact bytes?" needs.
- `core::Error`'s upload variants map to real status codes rather than a blanket 500 — 413 for a full
  quota with the numbers attached, 409 for a misaimed chunk carrying the offset to resume from, 415,
  422, 404. The 5xx scrubbing rule is untouched: a full disk answers 507 and lets the status code
  carry the meaning.

## [0.4.4] — 2026-09-20

Two pieces of daily use that had gone sour: the vault was one flat grid however much you hid in it,
and video in the swipe deck was close to unusable on the phone it is meant for.

Patch: no breaking API change and nothing to configure. One new migration (0008 — a table, a column,
an index), applied on start; it is additive, so a rollback to 0.4.3 still boots.

### Added
- **Folders in the vault.** Hidden photos can be filed. A row of cards above the grid — All, Unfiled,
  one per folder, New folder — filters it, a folder's own bar offers Rename and Delete, and selection
  gains Move: to a folder, to Unfiled, or into a new one named on the spot. `+` inside a folder hides
  straight into it.

  Folders group; they do not move anything. Files stay flat in `.vault/` and no name you type ever
  becomes part of a path. They live in their own `vault_folder` table rather than as albums, because
  `/api/albums` lists every album without asking for a PIN and a folder's name is as private as what
  is in it — for the same reason the column is deliberately absent from ordinary Gallery responses,
  where even a `null` would announce that a vault exists. Names are unique per user, ignoring case.

  **Deleting a folder never deletes a photo**: its contents stay hidden and become Unfiled, and the
  confirmation says so. Restoring a photo forgets its folder, so hiding it again lands it Unfiled.
- **A real player for video in the swipe deck.** The clip plays in place with one tap, with a play
  button while stopped, a spinner while it buffers, tap anywhere to pause and resume, a hairline of
  progress along the bottom edge, and the clip's length on the date line. It rewinds itself to the
  first frame when it ends.

### Fixed
- **Video actually starts, and nothing sits on top of it.** Play mounted a native-controls `<video>`
  and left it to `autoPlay`; Safari on iOS only starts a clip with sound from a `play()` made inside
  the tap, so it sat paused under Safari's own player — whose buttons landed on the heart and on our
  Play button, with a scrubber across half the photo you were judging. The card now owns its player,
  and there are no native controls left to collide with.
- **Originals stream in ranges.** `/raw` and the vault's `/raw` read the whole file into memory and
  answered every request with all of it. Safari opens a video with `Range: bytes=0-1` and expects a
  206; given a 200 it downloads the entire clip before the first frame and cannot seek — so Play on a
  24 MB clip meant a 24 MB wait, and every one of those sat whole in a 2 GB container. Both now serve
  from disk through `ServeFile`, with byte ranges and conditional requests.
- **A toast no longer re-locks the vault.** Every action that showed one dropped you back on the PIN
  pad: the locked-or-unset check re-ran whenever `onClose` changed identity, `App` passes a fresh
  inline one on each render, and every toast re-renders `App`. The check now runs once, on mount.
- **Disabled pills look disabled.** The vault's Hide pill was indistinguishable from a live one.

## [0.4.3] — 2026-09-17

Five fixes from a day of real use on a phone. The first is a correctness bug — a deleted photo could
still be sitting in an album, with no way to get it out of one — and the other four are the places
the UI made a phone harder to use than it needed to be.

Patch: no API change and nothing to configure. One new migration (an index), applied on start.

### Fixed
- **A binned photo leaves its albums.** The Gallery filters on status and the trash lists what you
  deleted, but the album grid joined `album_file` with no filter at all — so a photo you had already
  deleted kept its tile, kept its place in the count, and could even be the album's cover. There was
  no way out of that state from inside the album either: the viewer's Delete answers *"already in the
  trash"* for a file that is. Album contents, counts and covers now all read through one visibility
  rule that excludes trashed and vaulted files, and every route that changes a file's status
  recomputes the albums holding it.

  Membership itself survives a deletion on purpose, so **putting a photo back from the trash puts it
  back on the shelves it was filed on** — including a manual album, which nothing would ever rebuild.
  A permanent purge is what finally drops the row.

  Existing databases are reconciled at startup: counts and covers were computed under the old rule,
  so one statement at boot corrects every album rather than waiting for each to be touched.
- **Video previews show the video.** The detail viewer mounted a `<video>` with no `poster`, so a
  clip was a blank grey stage with a play button on it until you pressed play — the one thing you
  could not judge from it being whether it was worth keeping. It now shows the first frame ffmpeg
  already extracted at scan time, and loads metadata only, rather than pulling a 24 MB clip down
  because you paged past it.

### Added
- **Play a video from the swipe deck.** A video's card was a single still with no way to see the
  rest of the clip. There is now a Play button in the top-right corner — opposite the heart, framing
  the card rather than crowding one corner — that plays it inline with its own controls, without
  leaving the deck. Nothing streams until you ask: mounting a player per card would have the server
  serving originals as fast as you can swipe past them.
- **Swipe in from the left edge to go back**, in an album's contents. The screen follows your finger
  and springs back if you let go early, so the gesture says what it will do before you commit to it.
  Touch only, and only from within 28px of the edge — further in is an ordinary scroll.

### Changed
- **Sub-screen headers stay put.** `.page-header` was already pinned on the tabs, but the header an
  album's contents and the vault share scrolled away with the grid, so getting back meant flinging a
  few hundred photos all the way home first. It is sticky now, with the same hairline the tab headers
  grow once something passes under them.
- A photo deleted from inside an album leaves that grid immediately, and the item count in the header
  follows it down.

### Notes
- Migration `0007` adds an index on `album_file(file_id)`. "Which albums is this file in" now runs on
  every swipe, bin and restore, and that table's primary key only answers the opposite question — at
  100K files an unindexed lookup would be a full scan per swipe.

## [0.4.2] — 2026-09-17

Containment for the part of the pipeline Rust cannot vouch for. Every photo indexed is a file this
server did not write, and two of the things that read it — libheif for HEIC, ffmpeg for video — are C
libraries.

Patch: no API change, and the one new setting is optional. **The container hardening does not arrive
with the image**, though; see Notes.

### Changed
- **The container is confined.** Read-only root filesystem with a 256 MB `tmpfs` for `/tmp`, all
  Linux capabilities dropped, `no-new-privileges`, a 2 GB memory ceiling (`EASYCLEAN_MEM_LIMIT`, so
  you can raise it) and a 512-process limit — in both the production and the from-source compose
  files, because a local run that behaves differently from production is not much of a rehearsal.
  The image already ran as a non-root user; this is about what that user can reach if a malformed
  photo ever gets libheif or ffmpeg to misbehave. Verified end to end under every one of these
  constraints: boot, login, scan, blur scoring, thumbnail, preview, and `secret.key` generated
  `0600` on a read-only root.
- **Decode bounds are stated rather than inherited.** `open_image` goes through `ImageReader` and
  sets an explicit maximum edge of 65,535 px alongside the allocation cap. Two tests come with it:
  one pinning the bounds, and one proving the reader actually honours limits on this path — which is
  the property that would quietly disappear if it ever went back to `image::open`.

### Notes
- **Correction to the 0.4.1 notes.** Those said no decode limits were set and that a decompression
  bomb could still cost memory. That was wrong. `image::open` already applies `Limits::default()`,
  which caps one decode at 512 MiB, and `decode()` enforces it by calling `limits.reserve(...)` with
  the decoder's declared size *before* allocating. libheif's hook implements `image::ImageDecoder`,
  so HEIC was covered by that check too. The allocation cap was doing the work all along; this
  release makes the bounds explicit and adds dimension limits, which is a smaller claim than the one
  0.4.1 made.
- **The hardening needs one manual step.** The rollout job rewrites `.env` and runs
  `docker compose up -d`; it does not replace `/opt/easyclean/docker-compose.yml`. Upgrading the
  image alone will not confine the container. Re-run `deploy/install.sh` on the server — it
  overwrites `docker-compose.yml` and deliberately leaves an existing `.env` alone — or copy
  `deploy/docker-compose.yml` across by hand and `docker compose up -d`.
- The allocation cap stays at 512 MiB on purpose. A 48-megapixel phone photo needs about 144 MB
  decoded, so a tighter cap would start refusing real photographs. Total memory is bounded at the
  container instead, where it also covers what libheif and ffmpeg allocate out of Rust's reach.
- `.claude/auth-redesign-brief.md` records the account-management work this release does not do:
  self-service passcode change, token revocation, and making `.env` a bootstrap credential rather
  than a permanent override. The third blocks the first — 0.4.0 made the environment re-apply on
  every boot, so an in-app change would be overwritten on restart.

## [0.4.1] — 2026-09-17

The four surfaces the 0.4.0 audit did not reach: a sweep of every route for authorization, error
responses, the dependency tree, and what happens when a malformed photo is scanned. Three of the four
came back clean; this releases the one fix and three dependency bumps.

Patch rather than minor: no API changes, no configuration changes, nothing to do before upgrading.

### Security
- **Internal errors no longer reach the client.** `From<easyclean_core::Error>` turned any failure
  into a 500 whose body was the error's own text — which could carry `sqlx` messages naming columns
  and SQL, an `io::Error`, a `<data_dir>` path, or whatever `EASYCLEAN_CLIP_BASE_URL` points at. The
  reachable-without-logging-in case was `login`, which queries the database before it has
  authenticated anyone. The cause is now logged and a fixed sentence is returned instead. Scrubbing
  happens both where a 500 is built and again in `into_response`, so it holds for any 5xx however it
  was constructed; messages written by hand for a 4xx are untouched and still explain themselves.
- **rustls 0.23.44 → 0.23.45** (RUSTSEC-2026-0285, medium): TLS 1.3 handshake messages accepted
  across encryption level boundaries. This is the TLS client that fetches the CLIP weights.
- **crossbeam-epoch 0.9.18 → 0.9.21** (RUSTSEC-2026-0204): invalid pointer dereference in the
  `fmt::Pointer` impl for `Atomic`/`Shared`. Reached through `rayon`, by way of `image`.
- **anyhow 1.0.102 → 1.0.104** (RUSTSEC-2026-0190): unsoundness in `Error::downcast_mut()`.

### Notes
- `cargo audit` reports one remaining advisory, `rsa` (RUSTSEC-2023-0071, the Marvin timing attack),
  for which no fixed version exists. It is not reachable here: `cargo tree -i rsa -e normal` finds no
  path to it, so no RSA code is compiled into the server. The three remaining warnings — `paste`
  unmaintained, `event-listener` unsound, `spin` yanked — are transitive with no reachable path.
- `npm audit` reports two advisories in `browserslist` and `baseline-browser-mapping`. Both are
  build-time development dependencies and both are denial-of-service class; neither ships to a
  browser.
- **The authorization sweep found nothing.** All 44 routes were enumerated and matched to their
  handlers: only `/api/health` and `/api/auth/login` are unauthenticated, both deliberately. Twelve
  database functions take an id without a user parameter, and every call site of all twelve is
  guarded — by `owned_media`, by ids that came from a user-scoped query, or by an explicit ownership
  check. Album access is scoped in SQL.
- **Malformed media handling came back clean too.** The EXIF GPS parser bounds-checks before
  indexing, `decode.rs` propagates every failure rather than unwrapping, and — the part worth
  recording — no attacker-controlled EXIF *string* is ever stored. Only a timestamp and two
  coordinates are read out of a photo's metadata, which is what keeps a crafted file from reaching
  the database as text. Two residual risks stay open by design: no decode limits are set, so a
  decompression bomb can still cost memory; and libheif and ffmpeg are C libraries, where Rust's
  guarantees stop.

## [0.4.0] — 2026-09-16

A security release, from an audit of the whole system: every route, the path and SQL layers, the
token design, the service worker, the model download, the image and the CI/CD pipeline.

Minor rather than patch, because two of the fixes change how an existing install starts. **A server
with no `EASYCLEAN_PASSCODE` set, or an `EASYCLEAN_SECRET` shorter than 32 characters, now refuses
to boot** — see Notes before upgrading. Nothing in the database changes and there is no migration.

Much of what the audit looked at held up and is unchanged: `UserRoot::safe_join` correctly refuses
absolute paths and `..`, every one of the 65 SQL statements binds its parameters, passcodes and PINs
are bcrypt, HS256 is pinned on verification, the image runs as a non-root user with no secrets in
any layer, and `ci.yml` runs fork pull requests on GitHub's runners rather than the self-hosted one.

### Security
- **A login could take a photo back out of the vault.** Every per-file route reaches its row through
  `owned_media`, which refuses a hidden one — but `swipe` and `duplicates/resolve` take a file id
  straight from the request body and wrote status without it. `{"file_id":N,"action":"undo"}` moved a
  vaulted row back to `pending`, and `/raw` then served the photo to someone who never entered the
  PIN. Both statements now carry the `AND status <> 'vault'` guard that `vault_add` and
  `vault_restore` already spelled out. The vault defends against exactly one person — somebody
  holding an unlocked phone — and that was the person it let in.
- **No default passcode, anywhere.** `easyclean` was the fallback in `Config::default`, in
  `from_env` and in `docker-compose.yml`, which made it a credential published in this repository and
  reachable by anyone on the network. It is gone, and the server refuses to create a first account
  without an explicit one rather than inventing a weak one.
- **Changing the passcode works.** Accounts were seeded only when the `user` table was empty, so
  editing `EASYCLEAN_PASSCODE` afterwards silently did nothing while the operator believed they had
  rotated it. The environment is now re-applied on every boot; nothing else writes that table.
- **The service worker cached full-resolution originals as the app shell.** The navigation branch was
  tested before the `/api/` network-only guard, so opening a photo in a new tab — Share, and the
  vault's own open — routed those bytes through the handler that stores whatever it fetched under the
  fixed key `/index.html`. A vault photo, the one copy the server takes care to delete everywhere
  else, ended up in Cache Storage readable with no token. The API is now decided first, and the cache
  names are bumped to `v4` so a browser holding one drops it on activation.
- **PIN guessing is throttled**, on unlock and on the change-PIN route's `current_pin`, which was the
  same oracle. Four digits is 10,000 tries; the throttle is what makes that cost something. Keyed on
  the account rather than the IP, because whoever is guessing has already signed in as that user.
- **The login token is refused from a query string.** `<img src>` and `EventSource` cannot set a
  header, so those requests carry their credential in the URL — where it reaches access logs and, if
  ever navigated to, the address bar and history. That credential used to be the login itself: good
  for a week, good for everything. `AuthUser` is now header-only, and a new `MediaUser` extractor
  accepts only a **media token** from the query — read-only, an hour, and rejected everywhere else.
- **Opening an original no longer navigates to an API URL.** Share and the vault fetch the bytes and
  open a `blob:` URL instead, so no token reaches the address bar or history. The vault's fetch sends
  its token as a header, so `vault_token` appears in no URL at all.
- **The CLIP weights are pinned and verified.** They were fetched from `resolve/main` — whatever the
  repository owner last pushed — with no integrity check, and an `.onnx` is a compute graph handed
  straight to ONNX Runtime in this process. The download is now pinned to an upstream commit, each
  file is checked against its SHA-256 while it streams, and a file that does not match is deleted
  before it can be renamed into place. Files already on disk are re-hashed rather than trusted.
- **A Content-Security-Policy on the shell**, plus `X-Frame-Options`, `X-Content-Type-Options` and
  `Referrer-Policy: no-referrer` on every response including the API. The session token lives in
  `localStorage`, so any script that runs on this origin owns the account rather than the session.
- **`EASYCLEAN_SECRET` must be at least 32 characters.** HS256 accepts a one-character key, and a
  short one is recoverable offline from any token its holder already has.
- **Cached thumbnails are dropped when a photo is vaulted, and on logout.** They are keyed on the
  path with the token stripped, so a thumbnail cached while a photo was ordinary outlived both the
  session that fetched it and the photo being hidden.
- **`EASYCLEAN_USERS` no longer mangles a passcode containing a colon.** `lily:pa:ss` seeded the
  passcode `pa` under the meaningless role `ss` — a shorter credential than the operator set — and
  `lily:a:b:c` matched no arm at all and dropped the account without a word, which for a single-entry
  spec meant a server with no users.
- **Every third-party GitHub Action is pinned to a commit SHA.** `@v4` is a pointer its owner can
  repoint, `dtolnay/rust-toolchain@stable` was a branch, and `deploy.yml`'s runner is the home server.
- **The deploy step no longer splices the image tag into a shell string.** `$TAG` was interpolated
  into the `sh -c` script that backs up the index, so a tag containing `$(…)` executed inside a
  container with the data volume mounted. It is passed as an environment variable now.
- **The server address is validated before anything is sent to it.** It is where the passcode goes,
  and any string that parsed was accepted: `http://real.local@evil.example` reads as one host and
  resolves to another. Non-`http(s)` schemes, embedded credentials and trailing paths are refused,
  and a value already stored by an older build is re-checked on load.

### Added
- `POST /api/auth/media-token` — trades a header-authenticated login for a one-hour, read-only token
  for the routes that cannot send a header. It reads thumbnails, previews, originals and scan
  progress, and is rejected as a login everywhere else.

### Changed
- `EASYCLEAN_PASSCODE` is **required**, and is re-applied on every boot.
- `EASYCLEAN_SECRET`, when set, must be at least 32 characters.
- `deploy/install.sh` stops rather than warns when the passcode is still the published placeholder.
  The warning scrolled past on a screen that then printed a working URL, so the install looked done.

### Notes
- **Before upgrading**, make sure `/opt/easyclean/.env` has a non-empty `EASYCLEAN_PASSCODE`, and
  that `EASYCLEAN_SECRET` is either empty (the server generates and persists one) or at least 32
  characters — `openssl rand -hex 32`. A server that fails these refuses to start, and the rollout
  health check will roll it back.
- **An installed PWA shows broken thumbnails until it picks up this build**, because the old bundle
  puts the login token in image URLs and the server now refuses that. The shell is `no-cache` and the
  worker is network-first, so this resolves on the next launch rather than needing a reinstall.
- **The CLIP weights are re-checked once.** An install that already downloaded them has no
  verification marker, so they are re-hashed on first use; matching files are kept, and only
  mismatched ones are fetched again. Those bytes were never verified, which is the point.
- 12 regression tests were added, covering each of the fixes above that has a reachable attack path.

## [0.3.0] — 2026-09-16

A vault for photos nobody else should see, search that says what it found, and one header
shared by every screen you can navigate into.

Minor rather than patch: the vault is a genuine feature with its own auth boundary, and
`media_file.status` gains a value that existing installs have never seen. Every new endpoint is
additive, none that existed changed shape, and `0006` only adds two nullable columns and one
table.

### Added
- **A hidden vault.** Three fast taps on the profile chip in Setting opens it; nothing else in the
  app refers to it. Photos moved in leave the Gallery, albums, search, the stats and the timeline,
  and their file, thumbnail, cached preview and CLIP embedding all move or are deleted with them —
  so nothing readable survives outside `.vault/`. Guarded by its own 4–8 digit PIN, hashed the way
  the login passcode is and deliberately separate from it: the only person a hidden vault defends
  against is someone who can already open the app. Unlocking mints a 15-minute token that leaving
  the screen discards.
- **Media Types in Albums**, replacing Suggested Collections: Videos plus one album per image
  format — JPEG, PNG, HEIC, GIF, WebP, TIFF, BMP, RAW. Screenshots moves to Collections, where it
  belongs; a screenshot is a PNG like any other, so listing it as a format would say the wrong
  thing.
- **A full search overlay** with suggestions and recent searches, in place of the inline field that
  expanded inside the Gallery header. Results are headed by their count and a chip naming the query,
  which is also how you clear it.
- **Delete inside the vault.** Permanent, and it says so: the trash screen needs no PIN, so binning
  a hidden photo would stand it on a shelf anyone can browse.

### Changed
- **Semantic search returns up to 300 photos, not 60.** The relevance filter keeps roughly the top
  sixth of a library for any query, so on a large one *every* search truncated to exactly 60 and the
  result count said nothing. Responses now also carry `semantic_total` — how many cleared the filter
  before the cap — so the count can be honest about what it is showing.
- **One header for every full-screen sub-view.** The swipe decks, Duplicates, Trash, the vault and
  an opened album now share the same back button, centred title and insets.

### Fixed
- **Backdrop blur did nothing in Chromium.** Where the stylesheet hand-wrote a `-webkit-` twin
  beside the standard property, Lightning CSS read the pair as one prefixed declaration and emitted
  the prefix alone — which Chromium does not support. Every frosted surface in the app rendered as a
  flat tinted slab there while looking correct on iOS Safari, which does accept the prefixed form.
  All seven hand-written prefixes removed so the build generates them, and `build.cssTarget` now
  names the baseline instead of leaving it inferred.
- **Sub-screen titles were never centred.** "On this day", "Duplicates" and the rest sat wherever
  the back button and UNDO left room. Measured 32px off centre even after a first attempt with a
  three-column grid, because `1fr` is a minimum of auto and a wide action slot simply grows past its
  share.
- **No header allowed for the notch except the photo viewer's**, so on a notched phone every other
  back button and title sat under the status bar.
- **Page headers were inset 20px while the content below them was inset 16px**, leaving every large
  title four pixels out of line with its own cards.
- **Modals with a text field opened where the keyboard does.** New album and search now open against
  the top edge, under the title nav.
- **The page scrolled behind open modals**, and the tab bar rode up with the layout when the
  keyboard appeared instead of staying at the bottom edge.

## [0.2.2] — 2026-09-15

Three rounds of phone testing: bulk selection, a Share button that works, albums that mean
what they say, and a dashboard that knows what day it is.

Patch rather than minor: every new endpoint is additive, none that existed changed shape, and
the one migration only removes album rows that duplicated others.

### Added
- **Select mode in the Gallery.** A Select button beside Search turns the grid into a picker: tap
  photos to choose them, and the tab bar is replaced by Share · *N* selected · Album · Delete. Select
  becomes Cancel, so the way in and the way out are one control. Ticks use the accent colour, like
  every other chosen state in the app.
- **A share sheet that offers what actually works.** Save to Photos (opens the original so a
  long-press can add it to the camera roll — the route that works on iOS), Download, and the
  device's own share sheet when the connection is secure enough to allow it. When it isn't, the
  sheet says so instead of leaving a gap.
- **Swipe up on a photo opens its details**, the counterpart to pulling the drawer back down.
- **The Albums "+" creates an album.** It previously called `albums/refresh`, which recomputes
  counts and covers — invisible work that looked exactly like a broken button. New endpoint
  `POST /api/albums`, since an album could only be created as a side effect of filing a photo.
- **Details lists the albums a photo is in**, as chips, fetched per photo when the panel opens
  (`GET /api/media/:id/albums`) rather than joined into every Gallery row.

### Changed
- **The Timeline Checklist hides months you've finished.** It's a to-do list, and a cleaned month is
  done — leaving them in place pushed the months that still need work below the fold, which on a
  well-reviewed library is most of them. A **Show cleaned (N)** toggle on the section header brings
  them back, and when there's nothing left at all the list says so outright instead of rendering
  empty.
- **Empty album sections say how they fill.** "Nothing here yet" under Collections *and* Suggested
  Collections told you nothing you could act on, twice over, and read as two things that had failed
  to load. Each empty section now carries one line naming the action that fills it — heart a photo,
  keep a video or screenshot, finish a session — on a dashed card that reads as a waiting slot
  rather than a broken one.

- **Hearting a photo fills Favourites immediately.** Album membership was decided only by
  `categorize` at session finish, so a photo you hearted from the Gallery might never reach the
  Favourites album at all. The heart is the entire interface for that album, so it is now the thing
  that fills it — and un-hearting takes the photo back out.
- **Memories lists years, not years *and* months.** Every photo was filed into both, so Memories
  showed "Nov 2023" beside "2023" — the same pictures under two headings. Migration `0005` removes
  the month albums already in the database. The Clean tab's month checklist is unaffected: it reads
  `created_at` directly and never used albums.
- **Automatic albums are read-only.** Years, Videos, Screenshots and Favourites describe what a
  photo *is*, so hand-filing into one would state something untrue — a 2021 photo cannot belong to
  2023 — and the next rebuild would undo it anyway. The viewer's album picker now offers only albums
  you made yourself, and the server refuses the rest.
- **The viewer's drawers no longer cover the photo.** Details and Add to album were absolutely
  positioned over the bottom of the image, so you read a photo's details while looking at less and
  less of it. They are flex siblings now: the stage gives up the height and the photo shrinks to fit
  above them, whole and visible the entire time.
- **The drawers close by gesture, not by a button.** The `✕` is gone. Tap the photo, tap the toolbar
  button that opened it, or pull the drawer down — and a tap *inside* a drawer no longer dismisses
  it, so picking an album or reading a value can't close it by accident.
- **Add to album keeps its title and New album field fixed**, with only the album list scrolling
  beneath them, so the way to create an album can't scroll out of reach behind a long list.

### Removed
- **The Language row.** It only ever read "English" and had nothing behind it — a control that
  can't do anything is worse than no control. It comes back when there's a second language.

### Fixed
- **Share dumped you on a blank "Open in Preview" page.** `navigator.share` needs a secure context,
  which plain HTTP on a LAN is not, so the button always fell through to an `<a download>` — and iOS
  Safari answers that with a file-info page rather than a save. Replaced by the share sheet above.
- **The dashboard's weekday row was wrong six days out of seven.** It read Mon…Sat + Today, hardcoded,
  which is only correct if today is a Sunday. The labels are derived from the date now.
- **Finished days and cleaned months weren't accent-coloured.** Today's ring was filled with the
  accent while a day you had actually cleaned got a grey outline, so done and untouched looked
  nearly alike. Both now fill with the accent; today-with-nothing-swiped keeps an unfilled ring,
  because filling it would claim work that hasn't happened.
- **The viewer's drawers appeared and vanished instantly.** They slide up on open and back down on
  close, continuing from wherever a drag left them rather than snapping.
- **An album you created never appeared.** `Add to album` files into a `manual` album, but the
  Albums tab bucketed only `favorites`/`place`, `videos`/`screenshots` and `month`/`year` — so a
  manual album existed, held its photos, and was rendered nowhere. Collections shows them now.
- **Settings rows promised screens that don't exist.** Indexed media, Library size, Flagged blurry
  and Version all carried a `›` chevron and a pointer cursor, but none of them is clickable — they
  are readouts. The chevron, the cursor and the press feedback now appear only on a row that
  actually does something.
- **Theme and Accent Color had each other's icons.** Theme carried a paint palette — which means
  *colour*, not light-and-dark — and Accent Color carried a water droplet, which means nothing here.
  Theme now uses a half-filled circle, the standard light/dark mark, and the palette moved to the row
  that is actually about colour. The palette itself was redrawn with its thumb-hole notch, because
  the old one collapsed into an indistinct ring of dots at the 15px these render at.
- **The "Default" accent swatch showed the colour you had chosen.** It painted itself from
  `--accent`, which `applyAccent()` overrides inline the moment you pick a colour — so picking
  Orange turned the Default swatch orange too, showing the palette's first and fifth chips
  identical and hiding what Default actually means. It reads `--text` now: white on dark, black on
  light, never overridden.
- **A long filename broke out of the delete confirmation.** Camera and messenger names run past 70
  characters with no spaces to wrap on, so inside the sentence they pushed the dialog past the
  screen edge. The name is now its own line, truncated with an ellipsis and carrying the full text
  as a tooltip. Checked at 320px with a 75-character name.

## [0.2.1] — 2026-09-15

Phone-testing fixes: duplicates you can actually compare, date decks that filter by date, and an
installed PWA that picks up a deploy.

Patch rather than minor: nothing changes shape for existing data — no migration, no schema change,
and every endpoint that existed still answers the same way.

### Added
- **Duplicates get their own compare screen.** Reviewing copies in the one-card swipe deck asked
  "keep this?" about a photo whose only problem was that another copy existed — and never showed you
  the other copy. Now a whole cluster is on screen at once: tap the copies you mean, and one button
  settles the group. Selected go one way, the rest go the other, which covers every case in a single
  gesture — keep both, delete both, or split a pair. The largest copy is pre-selected and marked
  BEST (more bytes means less re-compression, which is how the original differs from the one that
  came back through a chat app); an eye button enlarges either copy when the difference is in the
  detail. Undo takes back a whole group, and Skip defers one.

  New endpoints: `GET /api/media/duplicates` returns whole clusters (never a group of one), and
  `POST /api/media/duplicates/resolve` settles or undoes one in a single call — a half-decided
  cluster would simply come back on the next pass.
- **Confirmation before anything is destroyed.** Delete in the photo viewer, Delete forever in the
  trash, and Empty trash each ask first, showing the photo in question and saying plainly whether it
  is recoverable. Empty trash was a two-tap EMPTY → SURE? in the same spot, which is no protection
  against the mis-tap it was meant to catch; Cancel now takes the focus, so the keyboard's easy path
  is the safe one too.
- **An update path for the installed PWA.** The app checks for a new build on launch, whenever it is
  resumed, and hourly, and offers a banner plus a Setting › About row when one is ready. Setting also
  shows the app's version beside the one the server reports, so "did my deploy reach this phone?" is
  answerable without reinstalling.

### Fixed
- **On This Day showed recent photos, not photos from this day.** It and Recents both fell through to
  the unfiltered `timeline` query, so the two cards showed the same deck as each other and as
  everything else. They are now real batch modes: `onthisday` matches this calendar day in any year,
  `recents` the last seven days.
- **The dashboard's On This Day badge was the day of the month.** It read as a count and never agreed
  with the deck behind it — 15 on the card, 200 in the session. Every Clean card now badges the
  number of photos actually waiting, and Duplicates reads "N groups · M files".
- **The Gallery's scope tabs slid under the title when scrolled.** They stuck to a hardcoded header
  height that was shorter than the real header. Header and tabs now pin as one block, so nothing has
  to know anyone's height.
- **The photo viewer was dark in light mode.** Its chrome, sheets and ground are theme-aware.
- **Albums had no subtitle**, so "Collections" — the first section heading — read as the page's
  summary. It now says "N collections", matching the Gallery's "N items".
- **A deploy never reached an installed PWA.** Three separate causes: the server sent no cache
  headers, so the browser applied its own freshness to `index.html` and `sw.js` (now `no-cache`,
  with content-hashed `/assets/` marked immutable for a year); the service worker called
  `skipWaiting()` during install, swapping itself under a page still running the old JavaScript and
  leaving no waiting worker for the app to notice; and nothing ever called `registration.update()`,
  which an app that is resumed rather than reloaded needs. Deleting and reinstalling the app was the
  only way out.

## [0.2.0] — 2026-09-15

Semantic search, a real photo viewer, and a Gallery that holds the whole library.

Minor rather than patch: this adds a database migration and changes what the Gallery shows.

### Added
- **Semantic photo search.** Type what a photo *shows* — "a crane at a harbour", "green rolling
  hills" — and find it regardless of its filename. CLIP ViT-B/32 (int8 ONNX) embeds every photo and
  your query into the same 512-dimension space; the closest vectors are the results.

  One search box, two kinds of answer: exact filename/album matches first (that search always works),
  then photos that merely look like the query, de-duplicated against the first group.

  Design notes, since each was a real decision:
  - **No vector database.** Benchmarked on this stack: an exact scan of 100K vectors is 15ms (int8),
    less than the 23ms the text encoder spends on the query itself. Qdrant or Milvus would add a
    second service and a sync problem to optimise the cheap half, and would break the locked
    "one core, one shell" architecture. Vectors are int8 BLOBs in the existing SQLite.
  - **Embeddings come from the cached thumbnail**, not the original — CLIP downsamples to 224x224
    regardless, so re-decoding a 12MB HEIC would be pure waste. Indexing measured 27.6ms per photo.
  - **Weights download on first use** (~149MB) into `<data_dir>/models/`, so the image grew only
    48MB (ONNX Runtime, statically linked) and the Docker build stays offline and reproducible.
    `EASYCLEAN_CLIP_BASE_URL` retargets it at a mirror.
  - **The relevance cut-off is relative.** CLIP similarities bunch in 0.15-0.30 whatever you ask, so
    an absolute threshold is meaningless. Results are kept by z-score against the library's own
    distribution for that query: subjects actually present measured z = 2.3-2.7, while gibberish and
    absent-but-valid subjects ("a giraffe in a spacesuit") measured 1.5-1.6. Below z=2 results are
    still shown, labelled "closest guesses" — a ranking that silently returns nothing is worse than
    one that admits it is unsure.

  New endpoints: `GET /api/search`, `GET /api/search/status`, `POST /api/search/index`. Settings
  gains a Photo Search panel with the download/index progress.
- **Pinch-to-zoom and swipe navigation in the photo viewer.** Two fingers zoom (up to 6x, double-tap
  toggles), one finger pans when zoomed, swipes left/right between photos when not, and swipes down
  to dismiss with the backdrop fading as it goes. The `<` `>` arrows are gone; arrow keys still work.
  Only the photo transforms — the top bar and action bar stay pixel-identical at full zoom, and the
  browser's own pinch stays disabled.
- **Viewer action bar**, shaped like the tab bar it replaces while the viewer is open: Share, Info,
  Album, Delete. In the trash it becomes Put back, Info, Delete forever, and the favourite button is
  hidden — favouriting is a review signal that means nothing for a binned file. Share uses the Web
  Share API where the browser offers it and otherwise downloads the original; on plain-HTTP LAN the
  API is unavailable (it needs a secure context), so the download is what actually runs today.
- **Gallery search runs on the server, across the whole library** (`q=`), matching the filename and
  the path a photo is filed under — so an album name finds its contents. It replaces a browser-side
  filter that could only see the pages already scrolled through: on a 213-photo fixture with 120
  loaded, searching now finds a file sitting at row ~213. Typing is debounced, so a six-letter word
  is one request. `%` and `_` are escaped rather than left to act as LIKE wildcards — searching
  `IMG_2203` no longer also matches `IMG-2203`.
- **Photos / Videos filter** (`type=`) beside the Gallery tabs, toggling off when tapped again. An
  unknown type is a 400 rather than a silent empty result.
- **Gallery tabs — All / Original / Cleaned** (`mode=library|original|cleaned`), so un-reviewed
  photos and filed ones can be told apart at a glance.
- **Manual album filing** (`POST /api/albums/add`): file a photo under an existing album or a new one
  by name, alongside the automatic categorisation that runs at session finish.
- **Bin a photo straight from the Gallery or viewer** (`POST /api/media/:id/trash`) — a swipe only
  records a decision for session finish, and there is no session to finish here. Still trash-first.
- **Days-left countdown in the trash**, turning red in the last three days, with `trash_retention_days`
  now exposed on `/api/settings`.

### Changed
- **Page headers are sticky.** Clean, Gallery, Albums, Setting and album detail keep their title bar
  pinned while the content scrolls, with a hairline appearing once something has scrolled under it.
- **The trash is ordered by when each file was binned**, not when the photo was taken — a 2019 photo
  deleted a minute ago belongs at the top, and `created_at` buried it. Rows open the viewer.

### Fixed
- **The Gallery stopped at 500 photos.** `/api/media/batch` had no pagination, so the Gallery asked
  for the server's maximum page and showed only that. It now pages (`page`, 1-based) and loads more as
  you scroll. The ordering gained an `id` tie-break: without it two photos sharing a timestamp were
  free to swap places between pages, so one could appear twice and another never.
- **The page scrolled behind the open photo viewer.** The lock set `body { overflow: hidden }`, but
  the scrolling element is `.screen` — the app is a flex column that owns its own overflow — so it
  did nothing. A class on `<body>` now freezes whichever `.screen` is mounted.
- **A pinch behaved like a one-finger drag.** `setPointerCapture` ran before the pointer was
  registered and throws `NotFoundError` for a pointer the browser no longer considers active, which
  aborted the handler before the two-finger state was set up. It now runs last, inside a try/catch.
- **The pre-rollout index backup could not write.** The snapshot step added in 0.1.1 ran `mkdir
  data/backups` as the deploy user, but the data dir belongs to uid 10001 (the container's user) and
  that user only owns `.env` — so the rollout aborted with EPERM before touching anything. It now
  runs inside a container as that uid, and is best-effort: a failed backup warns instead of blocking
  the deployment. Caught on the v0.1.1 rollout; the server was left untouched on 0.1.0 because the
  failure happened before the image swap.

## [0.1.1] — 2026-09-13

Review quality, detail views, and settings that actually do something.

### Added
- **Full-quality previews** (`GET /api/media/:id/preview`) — a 1600px, quality-88 JPEG rendered from
  the original and cached under `<data_dir>/previews/`. The swipe deck and the new detail viewer use
  it instead of the 480px thumbnail, which was far too soft to judge a photo by. The thumbnail is
  still painted first as an instant placeholder and cross-fades out. Videos fall back to their
  first-frame thumbnail, and an undecodable file falls back rather than breaking the card.
- **Photo detail viewer** — full-screen view opened from the Gallery grid or from inside an album:
  arrow-key and on-screen paging across the whole set, Escape to close, favourite toggle, and
  filename/date/size/flags. Videos play inline.
- **Album detail view** — album cards open their contents as a grid (paged, 60 at a time), and any
  photo opens in the viewer.
- **Real "Storage Saved" chart** (`GET /api/stats/storage?days=N`) — bytes and file counts binned per
  day, zero-filled across the window. The Settings chart previously drew a hardcoded array of bars
  and a fake "+12% vs last 30 days".
- **Per-user settings** (`GET`/`POST /api/settings`, `user_setting` table) — the first key is
  `duplicate_distance`. `EASYCLEAN_*` env vars remain the fallback for anything never set.
- **Gallery filename search** — the search button in the Gallery header now filters; it previously
  did nothing. Semantic search remains a later phase.
- **Designed icon set.** The supplied Broom, Pictures Folder, Settings, Fire and Trophy artwork now
  drives Clean/Gallery/Setting in the nav, the streak badges, and the Settings Library rows. The
  files are Figma exports that wrap a 90x90 white-on-transparent PNG inside an `.svg`, so they are
  used as CSS alpha *masks* over `currentColor` rather than as images — that is what lets raster
  artwork follow the theme and the accent. Each is inlined by Vite, so there are no extra requests.

### Fixed
- **Duplicate Sensitivity now works.** The slider was inert — no `onChange`, no persistence, no
  effect on detection. It now maps Low/Medium/High to pHash Hamming distances 4/10/16, saves per
  user, and re-clusters the library immediately via `detect::regroup_duplicates`, which reuses the
  stored hashes so changing the setting never re-decodes anything.
- **Accent Color now works.** `--accent` was defined in both themes but referenced by no rule, so
  picking a swatch changed nothing. It now drives the active nav/tab item, today's streak ring, the
  storage bars, the selected theme chip, the slider thumb, progress rings and focus outlines. Swatch
  0 is "Default", meaning *follow the theme* — white on dark, black on light — rather than the old
  hardcoded `#ffffff`, which was invisible in light mode.
- **Rollback survives a migration.** sqlx aborts by default when a database
  carries a migration the binary doesn't know about — exactly the state after deploying a release
  that adds one. An automatic rollback would then crash-loop instead of restoring service, silently
  defeating the safety net in `deploy.yml`. Migrations now open with `ignore_missing`, so an older
  image boots normally against a newer database and ignores tables it doesn't use. `deploy.yml` also
  snapshots the index into `data/backups/` before every rollout (last 10 kept), and the runbook
  documents recovery. **Rolling back from 0.1.1 to 0.1.0 still needs that DB restore** — 0.1.0 was
  built before this fix; from 0.1.1 onwards rollback is clean.
- **The Clean icon no longer looks like a pen** — it is the supplied broom glyph.
- **Icon style is consistent per row.** Adopting filled artwork left outline glyphs stranded beside
  it, so `album`, `calendar`, `clock`, `copy`, `blur` and `folder` were redrawn as filled glyphs.
  `Calendar.svg` and `Clock.svg` were not used: both are drawn in `#D9D9D9` with colours hardcoded,
  so they could not follow the theme. `Duplicate Images.svg` was not used either — it layers two
  photo glyphs around an opaque `#333` card, which flattens to a featureless blob as a mask.
- **The Gallery no longer hides photos you kept.** `get_batch` scoped every mode to
  `status = 'pending'`, which is right for a swipe deck and wrong for a library view, so a kept photo
  vanished from the Gallery. Added `mode=library` (everything except trash) and pointed the Gallery
  at it.
- **Progress rings are visible in light mode.** `ProgressRing` hardcoded `rgba(255,255,255,…)`,
  leaving near-invisible rings on the light theme's white cards; it now uses theme tokens.
- Purging a file now also deletes its cached thumbnail and preview, instead of leaving them behind.

### Removed
- Settings rows that told the user nothing actionable: **Server Status**, **Server URL**,
  **Folder Paths**, and the **Backend URL** input. `ServerSetup` is still the place to point the app
  at a different backend.
- **Zoom Sync** — a prototype leftover wired to nothing; there is no zoom feature for it to sync and
  no cross-device state channel. Its toggle CSS went with it.
- The Gallery's **Select** button, which had no multi-select or bulk action behind it.
- **Env-file clutter.** `.env`/`.env.example` are down to the five values you must actually set;
  anything that only repeated a server default (`EASYCLEAN_TRASH_RETENTION_DAYS`, `RUST_LOG`) is
  documented as an optional override instead of shipped pre-set. Dropped
  `EASYCLEAN_CORS_ORIGINS` from `.env.example`, which the local compose file never forwarded, so it
  silently did nothing. Removed `EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD` from the production compose
  and env example now that duplicate sensitivity is a per-user setting — the server still uses it as
  the first-run default, and the README now spells out which settings are operator-level (env) and
  which are per-user (Settings screen).

## [0.1.0] — 2026-09-13

First release deployed to the home server.

### Added
- **HEIC/HEIF decode** (`heic` cargo feature, `libheif-rs`) — iPhone photos now get thumbnails, blur
  scores, and duplicate hashes like any other format. Always enabled in the Docker image.
- **EXIF orientation** is applied on decode, so portrait phone photos no longer render sideways.
  All decoding now goes through one place, `core/src/decode.rs`.
- **Installable PWA** — web app manifest, generated icon set (192/512/maskable/apple-touch), and a
  service worker: offline app shell, cache-first hashed assets, cached thumbnails (capped), and
  never-cached API state.
- **CI** (`ci.yml`) — fmt, clippy `-D warnings`, tests, web lint/typecheck/build, PWA-asset check, and
  a Docker build whose container is smoke-tested (health, UI, auth enforcement, login, HEIC support).
- **CD** (`deploy.yml`) — pushing a `v*` tag promotes that commit to production: it re-runs the CI
  gate on the tagged commit, publishes `ghcr.io/<owner>/easyclean` at `:<version>`, `:<major>.<minor>`,
  `:sha-<commit>` and `:latest`, then rolls it out to the home server through a self-hosted runner,
  with a health check and automatic rollback to the previous tag. Pushes to `main` are gated but
  deploy nothing; a manual run with an existing tag rolls back without rebuilding. Replaces the
  separate `release.yml`, which built the same image on the same trigger.
- **`deploy/` production kit** for `/opt/easyclean`: GHCR-based compose file, `.env.example`,
  `install.sh`, and a runbook covering runner setup, rollback, backup, and security.
- **Scale benchmark** (`core/examples/bench.rs`) — generates a synthetic library and times scan,
  detection, per-screen queries, and incremental rescan.

### Changed
- **Brand mark** — the isometric "e" logo replaces the placeholder broom glyph and the generated teal
  sparkle icon. Traced from the source artwork to a single outline in `web/src/brand.ts`, which the
  React icon and `scripts/generate-icons.mjs` both read, so the in-app logo, favicon and PWA icons
  can't drift apart. In-app it's coloured by its container and inverts with the theme; the app icons
  are a white glyph on a true-black tile.
- **Login screen redesign** — ambient gradient backdrop, logo tile, large heading, labeled fields, a
  passcode reveal toggle, and a footer note, in both themes. `ServerSetup` shares the same shell.
- **Friendlier auth errors.** `ApiError` now carries the HTTP status and unwraps the server's
  `{"error": …}` body, so a rate-limited login shows "Too many attempts. Wait 15 minutes and try
  again." instead of raw JSON, and a bad passcode reads "Incorrect username or passcode."
- **Detection now runs across all cores.** It awaited each blocking decode before starting the next,
  leaving one core busy and the rest idle; it now keeps `available_parallelism()` files in flight and
  applies results to SQLite as they land. 10 000 images: **168.8s → 60.6s (2.8×)**, identical output.
- MIT `LICENSE`.
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

### Fixed
- **The web app now talks to the server that served it.** The API base URL defaulted to a hardcoded
  `http://localhost:8080` and was persisted into every browser — on a phone that address is the phone,
  so opening the app from a LAN device always landed on the "enter your server address" screen. It now
  defaults to same-origin, with `Settings → Backend URL` as the override and a store migration that
  clears the stale value.
- Service-worker thumbnail cache keys no longer include the `?token=` query, so a JWT isn't persisted
  in Cache Storage and cache hits survive a new token.
- **The Docker image never actually built.** The Dockerfile synthesized a stub workspace manifest that
  omitted `[workspace.dependencies]`, so cargo failed with "error inheriting `serde` from workspace
  root". It now edits the real manifest instead. The base also moved from Debian bookworm to trixie,
  because `libheif-rs` needs libheif ≥ 1.17 and bookworm ships 1.15.
- Web lint errors: a component declared during render in `SettingsTab` (remounted on every render,
  losing state), a `setState` called synchronously inside an effect in `SwipeScreen` (which also
  raced — a slow batch could overwrite a newer deck), and a stale-closure effect dependency in
  `TrashScreen`.
- All `cargo clippy` warnings; `cargo fmt` is clean. `npm audit` reports 0 vulnerabilities.

### Removed
- **The Tauri desktop shell.** The `desktop/` crate, its workspace member, its `release.yml` bundle job,
  and the `--exclude easyclean-desktop` flags in CI are gone; `Cargo.lock` drops 312 packages
  (660 → 348). EasyClean now ships one way — the Docker image — and the backend is only ever built
  inside it. The code remains in git history at `da8f7f5`.
- The superseded prototypes: `frontend/index.html` (single-file React app, now ported to `web/`) and
  the Python FastAPI `backend/`. Both remain in git history at `2763e16`.

### Security
- **No shared default JWT secret.** The forgeable `dev-insecure-secret-change-me` fallback is gone.
  The secret comes from `EASYCLEAN_SECRET`, else a persisted `<data_dir>/secret.key` (mode 0600), else
  a freshly generated random key — resolved and logged on *every* boot, not just the first.
- **CORS is closed by default.** `CorsLayer::permissive()` is replaced by same-origin-only, opt-in via
  `EASYCLEAN_CORS_ORIGINS`. Previously any page on the LAN could read the API with a stolen token.
- Media is mounted read-write (the app moves files); README documents the LAN-only posture, passcode
  requirements, and trash-first deletion.

### Notes
- CLIP semantic search (M6) and a native mobile client (M7) remain post-MVP; MVP ships web + PWA.

# Brief — account management: passcode change, token revocation, env as bootstrap

> Written 2026-09-17, after the security audit that shipped in 0.4.0 and 0.4.1.
> Read [CLAUDE.md](CLAUDE.md) first — this brief assumes it. Current version: **0.4.1**.

## What this is

Three gaps in how accounts work. None of them is a vulnerability on its own; together they mean a
user cannot manage their own credential and a lost phone cannot be logged out. They are one piece of
work because the third blocks the first.

**Do not treat this as "add an auth framework".** The existing design is sound and stays. See
*Non-goals* before proposing anything larger.

## What exists today (verified, with locations)

| Piece | Where | State |
|---|---|---|
| Passcode hashing | [core/src/auth.rs:15](../core/src/auth.rs#L15) | bcrypt, `DEFAULT_COST`, self-salting. Fine. |
| Token minting/verify | [core/src/auth.rs:51](../core/src/auth.rs#L51), [:67](../core/src/auth.rs#L67) | HS256, algorithm pinned in `Validation::new(Algorithm::HS256)`, `exp` validated. Fine. |
| Claims | [core/src/auth.rs:8](../core/src/auth.rs#L8) | `{ sub, role, exp }` — **no version/jti**, which is why revocation is impossible today. |
| Three token roles | `role` claim | login (`admin`/`user`), `vault` ([:29](../core/src/auth.rs#L29)), `media` ([:33](../core/src/auth.rs#L33)). Checked at three extractors in [server/src/auth.rs](../server/src/auth.rs). |
| Login | [server/src/routes.rs](../server/src/routes.rs) `login` | rate-limited 5/15min per IP. Fine. |
| Seeding | [server/src/main.rs:46](../server/src/main.rs#L46) | **re-applies `EASYCLEAN_PASSCODE` / `EASYCLEAN_USERS` on every boot** — see the constraint below. |
| `user` table | [core/migrations/0001_init.sql](../core/migrations/0001_init.sql) | `username TEXT PK, pass_hash TEXT, role TEXT`. No version column. |
| Logout | [web/src/store/session.ts](../web/src/store/session.ts) | **client-side only** — clears `localStorage`, the JWT stays valid for its full 7 days. |

## The three gaps

1. **No passcode change.** There is no endpoint. `settings_put` accepts exactly one field,
   `duplicate_distance`. The only way to change a passcode is the operator editing `.env` and
   restarting — so in a multi-user install (`EASYCLEAN_USERS=saiz:s3cret:admin,lily:her-passcode`)
   the admin holds every user's passcode in plaintext and a user cannot rotate their own.

2. **No revocation.** Nothing can invalidate an issued token. A lost phone keeps working for up to
   `EASYCLEAN_JWT_EXPIRE_DAYS` (default 7). The only lever is rotating `EASYCLEAN_SECRET`, which
   logs *everyone* out.

3. **`.env` is permanently authoritative.** ← **read this before writing any code.**
   0.4.0 changed seeding from "only when the `user` table is empty" to "re-apply on every boot",
   because the old behaviour meant editing `EASYCLEAN_PASSCODE` silently did nothing. That fix was
   correct, but it means **an in-app passcode change would be overwritten on the next restart.**
   Gap 1 cannot ship until this is redesigned. Do this part first.

## Proposed design

Adjust if you find something better, but keep the properties: `.env` still works for bootstrap and
for recovery, an in-app change survives restart, and a changed passcode invalidates old sessions.

**A. `.env` becomes bootstrap, not override.**
Store a fingerprint of the last-applied env credential (e.g. SHA-256 of the passcode) in
`user_setting` or a new `server_state` table. On boot, per user: if no row exists → seed; if the
fingerprint differs from stored → the operator edited `.env`, so re-apply and update the fingerprint;
otherwise leave the database alone. That keeps `.env` as the recovery path (edit it, restart, you are
back in) without it clobbering a user's own change.

**B. `POST /api/auth/passcode`** — `{ current_passcode, new_passcode }`, `AuthUser` extractor.
Verify current with `verify_passcode`, enforce a minimum length, `hash_passcode`, write, bump
`token_version`. Rate-limit it: reuse `LoginRateLimiter` keyed on the account, exactly as
`vault_unlock` does (see the `vault_attempt_key` pattern in routes.rs) — `current_passcode` is a
guessing oracle otherwise. This is the same mistake the vault PIN had before 0.4.0; do not repeat it.

**C. `token_version` for revocation.**
Add `token_version INTEGER NOT NULL DEFAULT 1` to `user` (new migration). Put it in `Claims`, check
it in the `AuthUser` extractor against the stored value, reject on mismatch. Bump on passcode change
and on an explicit "log out everywhere". Decide deliberately whether vault and media tokens also
carry it — they should, or a media token outlives the revocation by up to an hour.

Note this adds a DB read per request to `AuthUser`. Measure it; if it matters, cache per username
with a short TTL, but do not add a cache before you have a number.

**D. Frontend.** A passcode row in Settings (match [ui.md](ui.md), do not redesign), and make logout
call the server so it actually revokes.

**E. Optional, if it is cheap:** `EASYCLEAN_PASSCODE_FILE` alongside the env var, read at startup.
Env vars are visible in `docker inspect`, `docker compose config` and `/proc/<pid>/environ`, so a
file is a real improvement for the bootstrap credential. Not required for the rest to work.

## Non-goals — do not build these

- **No external identity provider.** No OIDC, Auth0, Google sign-in. This app is LAN-first and must
  work when the internet is down; an external IdP makes the photo library unreachable during an
  outage and sends auth metadata to a third party.
- **No passkeys/WebAuthn.** They need a secure context. The deployment is plain HTTP on the LAN, so
  this is blocked by the transport, not by the auth design.
- **No MFA**, no session-listing UI, no account self-registration. Household photo cleaner.
- **No new dependencies** unless something genuinely cannot be done without one.

## Constraints

- **Migration required** (`core/migrations/`). CLAUDE.md: schema changes → migrate or wipe. Existing
  installs have real data; the migration must be additive and must not log anyone out on upgrade
  beyond what is intended.
- **Do not weaken 0.4.0's guarantees.** The server must still refuse to boot with no passcode
  configured and no existing user, and `EASYCLEAN_SECRET` must still be rejected below 32 chars.
  There are tests for both in `core/src/config.rs`; they must keep passing.
- **Tests are the deliverable, not an extra.** Every fix in 0.4.0/0.4.1 shipped with a regression
  test. At minimum: current-passcode required; wrong current rejected; change is rate-limited; old
  token rejected after a change; `.env` edit still recovers an account; an in-app change survives a
  restart.

## How to verify (no Rust toolchain on this Mac — by design)

```
docker run --rm -m 6g -v "$PWD":/src -w /src \
  -v easyclean-cargo-registry:/usr/local/cargo/registry \
  -v easyclean-target:/src/target \
  rust:1-trixie \
  sh -c "rustup component add rustfmt clippy >/dev/null 2>&1; \
         cargo fmt --all && cargo clippy --workspace --lib --bins --tests -- -D warnings && \
         cargo test --workspace --lib --bins"
```

Use `--lib --bins`: `cargo test --workspace` also links `core/examples/bench.rs`, which OOM-kills the
linker in a container. Web: `cd web && npx tsc -b && npx eslint src public/sw.js && npm run build`.
Then a real `docker build` + container smoke test for anything touching startup.

## Process

- Branch, PR, squash-free rebase merge (history is linear — no merge commits).
- **Ask which version number to use before pushing any tag.** Pushing a `v*` tag deploys to
  production via a self-hosted runner on the home server. The user has asked to be consulted on the
  number every time; "ship it" authorises the action, not the version.
- Changelog entry in the project's voice: explain *why*, not just what. See the 0.4.0 and 0.4.1
  entries.

## Suggested opening move

Read this brief, `CLAUDE.md`, `core/src/auth.rs`, `server/src/auth.rs`, `server/src/main.rs` (the
seeding block) and `core/migrations/`, then come back with the migration plan and the
`token_version` checking strategy **before** writing code. Part A is the one with a real design
decision in it; get that agreed first.

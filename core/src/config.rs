use std::path::PathBuf;

use crate::error::{Error, Result};

/// Filename of the generated JWT secret inside the data dir.
const SECRET_FILE: &str = "secret.key";

/// Shortest `EASYCLEAN_SECRET` accepted. Matches what [`random_hex_32`]
/// produces, so an operator-supplied key is held to the generated one's bar.
const MIN_SECRET_LEN: usize = 32;

/// Where the JWT signing secret came from. Surfaced at startup so an operator
/// can tell a configured secret from a generated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOrigin {
    /// Supplied via `EASYCLEAN_SECRET`.
    Env,
    /// Read from `<data_dir>/secret.key` (generated on an earlier run).
    Loaded,
    /// Freshly generated and persisted to `<data_dir>/secret.key`.
    Generated,
}

/// A user to seed into the DB on first run.
#[derive(Debug, Clone)]
pub struct UserSeed {
    pub username: String,
    pub passcode: String,
    pub role: String,
}

/// Runtime configuration, loaded from `EASYCLEAN_*` environment variables with
/// sensible defaults. Built once at startup and shared.
#[derive(Debug, Clone)]
pub struct Config {
    /// Root that contains per-user folders (`<media_root>/<user>/source|clean|.trash`).
    pub media_root: PathBuf,
    /// Where the SQLite DB and caches live.
    pub data_dir: PathBuf,
    /// HTTP port the server binds (on 0.0.0.0).
    pub port: u16,
    /// Laplacian-variance cutoff: below this ⇒ blurry.
    pub blur_threshold: f64,
    /// Max pHash Hamming distance for two images to count as duplicates.
    pub duplicate_distance_threshold: u32,
    /// Days a file stays in `.trash` before auto-purge.
    pub trash_retention_days: u32,
    /// User served by single-user dev runs / the default seed.
    pub default_user: String,
    /// JWT signing secret. Empty until [`Config::resolve_secret`] runs — there
    /// is deliberately no shared fallback constant (it would be forgeable).
    pub secret: String,
    /// JWT validity in days.
    pub jwt_expire_days: i64,
    /// Extra browser origins allowed to call the API. Empty ⇒ same-origin only
    /// (the UI is served by this server, so that is the norm). `*` ⇒ any origin.
    pub cors_origins: Vec<String>,
    /// Users seeded into the DB when it's empty.
    pub users: Vec<UserSeed>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            media_root: PathBuf::from("./media"),
            data_dir: PathBuf::from("./data"),
            port: 8080,
            blur_threshold: 100.0,
            duplicate_distance_threshold: 10,
            trash_retention_days: 30,
            default_user: "default".into(),
            secret: String::new(),
            jwt_expire_days: 7,
            cors_origins: Vec::new(),
            // Deliberately empty. A default passcode here is a published
            // credential: the value ships in the repo, the compose file and the
            // image, so every install that never changed it answers to the same
            // word — and anyone on the Wi-Fi can read it in this file. The
            // server refuses to seed a first user without an explicit
            // `EASYCLEAN_PASSCODE` instead (see `main`).
            users: Vec::new(),
        }
    }
}

impl Config {
    /// Build config from `EASYCLEAN_*` env vars, falling back to defaults.
    pub fn from_env() -> Self {
        let d = Config::default();
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());

        let default_user = var("EASYCLEAN_USER").unwrap_or(d.default_user);
        // No passcode ⇒ no seed. `from_env` stays infallible and simply reports
        // that there is nobody to create; refusing to start is `main`'s call,
        // made once, where the database can say whether a user already exists.
        let users = match var("EASYCLEAN_USERS") {
            Some(spec) => parse_users(&spec),
            None => var("EASYCLEAN_PASSCODE")
                .map(|passcode| {
                    vec![UserSeed {
                        username: default_user.clone(),
                        passcode,
                        role: "admin".into(),
                    }]
                })
                .unwrap_or_default(),
        };

        Config {
            media_root: var("EASYCLEAN_MEDIA_ROOT")
                .map(PathBuf::from)
                .unwrap_or(d.media_root),
            data_dir: var("EASYCLEAN_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(d.data_dir),
            port: var("EASYCLEAN_PORT")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.port),
            blur_threshold: var("EASYCLEAN_BLUR_THRESHOLD")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.blur_threshold),
            duplicate_distance_threshold: var("EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.duplicate_distance_threshold),
            trash_retention_days: var("EASYCLEAN_TRASH_RETENTION_DAYS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.trash_retention_days),
            default_user,
            secret: var("EASYCLEAN_SECRET").unwrap_or(d.secret),
            jwt_expire_days: var("EASYCLEAN_JWT_EXPIRE_DAYS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.jwt_expire_days),
            cors_origins: var("EASYCLEAN_CORS_ORIGINS")
                .map(|v| parse_origins(&v))
                .unwrap_or(d.cors_origins),
            users,
        }
    }

    /// Full path to the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("easyclean.db")
    }

    /// Path of the persisted generated secret.
    pub fn secret_path(&self) -> PathBuf {
        self.data_dir.join(SECRET_FILE)
    }

    /// Make sure `self.secret` holds a real signing key.
    ///
    /// Order: `EASYCLEAN_SECRET` (already loaded by [`Config::from_env`]) →
    /// `<data_dir>/secret.key` from a previous run → a fresh 32-byte random key
    /// persisted with `0600`. Anyone's photo library sits behind these tokens,
    /// so there is no shared default to fall back to.
    pub fn resolve_secret(&mut self) -> Result<SecretOrigin> {
        let supplied = self.secret.trim().to_string();
        if !supplied.is_empty() {
            // A generated key is 32 random bytes; one typed by hand should be
            // worth as much. HS256 accepts a one-character key perfectly
            // happily, and a short one is guessable offline from any token the
            // holder already has — at which point every session is forgeable.
            if supplied.len() < MIN_SECRET_LEN {
                return Err(Error::Auth(format!(
                    "EASYCLEAN_SECRET is {} characters; it must be at least {MIN_SECRET_LEN}. \
                     Generate one with: openssl rand -hex 32",
                    supplied.len()
                )));
            }
            self.secret = supplied;
            return Ok(SecretOrigin::Env);
        }

        let path = self.secret_path();
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                self.secret = existing;
                return Ok(SecretOrigin::Loaded);
            }
        }

        std::fs::create_dir_all(&self.data_dir)?;
        let secret = random_hex_32()?;
        write_secret(&path, &secret)?;
        self.secret = secret;
        Ok(SecretOrigin::Generated)
    }
}

/// 32 random bytes, hex-encoded (64 chars).
fn random_hex_32() -> Result<String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| Error::Auth(format!("no system randomness: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write the signing key owner-readable only.
///
/// Created with `0600` rather than written and then chmod'ed: the gap between
/// the two would leave the key briefly world-readable under a permissive umask.
#[cfg(unix)]
fn write_secret(path: &std::path::Path, secret: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(secret.as_bytes())?;
    // `mode` only applies when the file is created, so tighten an existing one.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secret(path: &std::path::Path, secret: &str) -> Result<()> {
    std::fs::write(path, secret)?;
    Ok(())
}

/// Parse a comma-separated origin list. `*` anywhere ⇒ allow any origin.
fn parse_origins(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The only roles a seed may carry. Anything else in the role position is
/// read as part of the passcode rather than quietly accepted.
const ROLES: [&str; 2] = ["admin", "user"];

/// Parse `user:pass[:role],user2:pass2` into seeds.
///
/// Split off the username, then look for a role only at the *end* — a passcode
/// is the one field here that may legitimately contain a colon, and splitting
/// on every colon mis-read it twice over. `lily:pa:ss` used to seed the passcode
/// `pa` with the role `ss`: a shorter credential than the operator set, under a
/// role that means nothing. `lily:a:b:c` matched no arm at all and the account
/// was dropped without a word — and if that was the only entry, the server came
/// up with no users whatsoever.
///
/// So: everything between the first colon and an optional trailing `:admin` /
/// `:user` is the passcode, colons and all.
fn parse_users(spec: &str) -> Vec<UserSeed> {
    spec.split(',')
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| {
            let Some((username, rest)) = entry.split_once(':') else {
                tracing::warn!(
                    "EASYCLEAN_USERS entry {entry:?} has no passcode — expected user:passcode[:role]"
                );
                return None;
            };
            let username = username.trim().to_string();

            let (passcode, role) = match rest.rsplit_once(':') {
                Some((pass, maybe_role)) if ROLES.contains(&maybe_role.trim()) => {
                    (pass.to_string(), maybe_role.trim().to_string())
                }
                // No trailing role, or a word that isn't one: the whole tail is
                // the passcode. Better to treat an unknown role as part of the
                // secret than to hand out a privilege nobody asked for.
                _ => (rest.to_string(), "user".to_string()),
            };

            if username.is_empty() || passcode.is_empty() {
                tracing::warn!("EASYCLEAN_USERS entry {entry:?} has an empty username or passcode");
                return None;
            }
            Some(UserSeed {
                username,
                passcode,
                role,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.port, 8080);
        assert_eq!(c.trash_retention_days, 30);
        assert_eq!(c.db_path(), PathBuf::from("./data/easyclean.db"));
    }

    #[test]
    fn secret_is_generated_then_reused() {
        let dir = std::env::temp_dir().join(format!("ec-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut a = Config {
            data_dir: dir.clone(),
            ..Config::default()
        };
        assert_eq!(a.resolve_secret().unwrap(), SecretOrigin::Generated);
        assert_eq!(a.secret.len(), 64, "32 random bytes, hex-encoded");

        // A second boot must reuse it, or every restart invalidates sessions.
        let mut b = Config {
            data_dir: dir.clone(),
            ..Config::default()
        };
        assert_eq!(b.resolve_secret().unwrap(), SecretOrigin::Loaded);
        assert_eq!(a.secret, b.secret);

        // An explicit secret always wins.
        let supplied = "s".repeat(MIN_SECRET_LEN);
        let mut c = Config {
            data_dir: dir.clone(),
            secret: supplied.clone(),
            ..Config::default()
        };
        assert_eq!(c.resolve_secret().unwrap(), SecretOrigin::Env);
        assert_eq!(c.secret, supplied);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_short_secret_is_refused() {
        let dir = std::env::temp_dir().join(format!("ec-shortsecret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut c = Config {
            data_dir: dir.clone(),
            secret: "too-short".into(),
            ..Config::default()
        };
        // Must not fall through to generating one: silently replacing a weak
        // key with a good one would log "generated" and leave the operator
        // believing the value they set is in use.
        assert!(c.resolve_secret().is_err());
        assert!(!dir.join(SECRET_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_default_credential_is_shipped() {
        assert!(
            Config::default().users.is_empty(),
            "a default passcode would be a published credential"
        );
    }

    #[test]
    fn no_shared_default_secret() {
        assert!(Config::default().secret.is_empty());
    }

    #[test]
    fn parses_origin_lists() {
        assert_eq!(
            parse_origins("http://a.local:5173/, http://b.local"),
            vec![
                "http://a.local:5173".to_string(),
                "http://b.local".to_string()
            ]
        );
        assert!(parse_origins(" , ").is_empty());
    }

    #[test]
    fn parses_user_specs() {
        let u = parse_users("lily:pw1,zack:pw2:admin");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].username, "lily");
        assert_eq!(u[0].passcode, "pw1");
        assert_eq!(u[0].role, "user");
        assert_eq!(u[1].username, "zack");
        assert_eq!(u[1].passcode, "pw2");
        assert_eq!(u[1].role, "admin");
    }

    /// A passcode is the one field that may contain a colon, and both of these
    /// used to go wrong quietly — the first by seeding `pa` instead of `pa:ss`,
    /// the second by dropping the account and seeding nobody.
    #[test]
    fn a_colon_in_a_passcode_survives() {
        let u = parse_users("lily:pa:ss");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].passcode, "pa:ss", "the passcode must not be truncated");
        assert_eq!(u[0].role, "user", "`ss` is not a role");

        let u = parse_users("lily:a:b:c");
        assert_eq!(u.len(), 1, "the account must not vanish");
        assert_eq!(u[0].passcode, "a:b:c");

        // A real trailing role is still read as one.
        let u = parse_users("zack:pa:ss:admin");
        assert_eq!(u[0].passcode, "pa:ss");
        assert_eq!(u[0].role, "admin");
    }

    #[test]
    fn malformed_user_entries_are_dropped_not_guessed() {
        assert!(parse_users("nopasscode").is_empty());
        assert!(parse_users("lily:").is_empty());
        assert!(parse_users(":pw").is_empty());
        assert!(parse_users(" , ").is_empty());
    }
}

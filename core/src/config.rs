use std::path::PathBuf;

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
    /// JWT signing secret.
    pub secret: String,
    /// JWT validity in days.
    pub jwt_expire_days: i64,
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
            secret: "dev-insecure-secret-change-me".into(),
            jwt_expire_days: 7,
            users: vec![UserSeed {
                username: "default".into(),
                passcode: "easyclean".into(),
                role: "admin".into(),
            }],
        }
    }
}

impl Config {
    /// Build config from `EASYCLEAN_*` env vars, falling back to defaults.
    pub fn from_env() -> Self {
        let d = Config::default();
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());

        let default_user = var("EASYCLEAN_USER").unwrap_or(d.default_user);
        let users = match var("EASYCLEAN_USERS") {
            Some(spec) => parse_users(&spec),
            None => vec![UserSeed {
                username: default_user.clone(),
                passcode: var("EASYCLEAN_PASSCODE").unwrap_or_else(|| "easyclean".into()),
                role: "admin".into(),
            }],
        };

        Config {
            media_root: var("EASYCLEAN_MEDIA_ROOT").map(PathBuf::from).unwrap_or(d.media_root),
            data_dir: var("EASYCLEAN_DATA_DIR").map(PathBuf::from).unwrap_or(d.data_dir),
            port: var("EASYCLEAN_PORT").and_then(|v| v.parse().ok()).unwrap_or(d.port),
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
            users,
        }
    }

    /// Full path to the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("easyclean.db")
    }
}

/// Parse `user:pass[:role],user2:pass2` into seeds.
fn parse_users(spec: &str) -> Vec<UserSeed> {
    spec.split(',')
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(':').collect();
            match parts.as_slice() {
                [u, p] => Some(UserSeed {
                    username: u.trim().to_string(),
                    passcode: (*p).to_string(),
                    role: "user".to_string(),
                }),
                [u, p, r] => Some(UserSeed {
                    username: u.trim().to_string(),
                    passcode: (*p).to_string(),
                    role: r.trim().to_string(),
                }),
                _ => None,
            }
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
    fn parses_user_specs() {
        let u = parse_users("lily:pw1,zack:pw2:admin");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].username, "lily");
        assert_eq!(u[0].role, "user");
        assert_eq!(u[1].username, "zack");
        assert_eq!(u[1].role, "admin");
    }
}

mod auth;
mod error;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use easyclean_core::config::SecretOrigin;
use easyclean_core::Config;

use crate::routes::LoginRateLimiter;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut cfg = Config::from_env();
    tracing::info!(media_root = ?cfg.media_root, data_dir = ?cfg.data_dir, "starting EasyClean");

    // Resolve the JWT signing key on *every* boot — env var, else the key
    // persisted in the data dir, else a fresh random one. Never a shared
    // constant: tokens guard someone's whole photo library.
    match cfg.resolve_secret()? {
        SecretOrigin::Env => tracing::info!("JWT secret: from EASYCLEAN_SECRET"),
        SecretOrigin::Loaded => {
            tracing::info!("JWT secret: loaded from {}", cfg.secret_path().display())
        }
        SecretOrigin::Generated => tracing::warn!(
            "JWT secret: generated a new random key at {} — set EASYCLEAN_SECRET to manage it \
             yourself (keep the data dir to survive a container rebuild)",
            cfg.secret_path().display()
        ),
    }

    // Registers libheif when built with `--features heic`; logs which it is.
    easyclean_core::decode::init_decoders();

    let db = easyclean_core::db::init(&cfg.db_path()).await?;

    // Accounts come from the environment on *every* boot, not just the first.
    //
    // Seeding only when the table was empty meant the passcode was whatever the
    // very first boot happened to use, permanently: editing EASYCLEAN_PASSCODE
    // afterwards changed nothing, silently, while the operator believed they
    // had rotated it. Nothing else writes `user` — the app has no
    // change-passcode screen — so the env var is the single source of truth and
    // re-applying it is what makes editing it mean something.
    let existing = easyclean_core::db::count_users(&db).await?;
    if cfg.users.is_empty() {
        if existing == 0 {
            anyhow::bail!(
                "no account to create: set EASYCLEAN_PASSCODE (or EASYCLEAN_USERS) and start \
                 again.\nThere is deliberately no default passcode — this server binds every \
                 interface, so a built-in one would be a credential anybody on the network \
                 already knows."
            );
        }
        tracing::info!("no EASYCLEAN_PASSCODE/EASYCLEAN_USERS set — keeping the {existing} existing account(s)");
    } else {
        for u in &cfg.users {
            let hash = easyclean_core::auth::hash_passcode(&u.passcode)?;
            easyclean_core::db::upsert_user(&db, &u.username, &hash, &u.role).await?;
        }
        let names: Vec<&String> = cfg.users.iter().map(|u| &u.username).collect();
        let verb = if existing == 0 {
            "seeded"
        } else {
            "reconciled"
        };
        tracing::info!(
            "{verb} {} account(s) from the environment: {:?}",
            cfg.users.len(),
            names
        );
    }

    // Reconcile the album shelves with what they are now allowed to show.
    //
    // Counts and covers are stored columns, and this release stops them
    // counting binned and hidden files — so every database that predates it is
    // carrying numbers computed under the old rule. One statement at boot is
    // what makes the correction arrive with the upgrade rather than whenever
    // each album next happens to be touched.
    match easyclean_core::db::refresh_all_album_meta(&db).await {
        Ok(n) => tracing::info!("albums: recomputed counts and covers for {n} album(s)"),
        // Not fatal: stale counts are a cosmetic wrong number, and refusing to
        // start over one would be the larger outage.
        Err(e) => tracing::warn!("albums: could not recompute counts and covers: {e}"),
    }

    let port = cfg.port;
    let state = AppState {
        db,
        cfg: Arc::new(cfg),
        scan: Default::default(),
        rate_limiter: LoginRateLimiter::default(),
        index: Default::default(),
        clip: Default::default(),
    };

    // Start the 30-day retention purge task (runs every hour).
    routes::spawn_retention_task(state.clone()).await;

    let app = routes::router(state).into_make_service_with_connect_info::<SocketAddr>();

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

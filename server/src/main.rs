mod auth;
mod error;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use easyclean_core::Config;

use crate::routes::LoginRateLimiter;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::from_env();
    tracing::info!(media_root = ?cfg.media_root, data_dir = ?cfg.data_dir, "starting EasyClean");

    let db = easyclean_core::db::init(&cfg.db_path()).await?;

    // Seed users on first run.
    if easyclean_core::db::count_users(&db).await? == 0 {
        for u in &cfg.users {
            let hash = easyclean_core::auth::hash_passcode(&u.passcode)?;
            easyclean_core::db::upsert_user(&db, &u.username, &hash, &u.role).await?;
        }
        let names: Vec<&String> = cfg.users.iter().map(|u| &u.username).collect();
        tracing::info!("seeded {} user(s): {:?}", cfg.users.len(), names);
        if cfg.secret == "dev-insecure-secret-change-me" {
            tracing::warn!("using the default dev JWT secret — set EASYCLEAN_SECRET in production");
        }
    }

    let port = cfg.port;
    let state = AppState {
        db,
        cfg: Arc::new(cfg),
        scan: Default::default(),
        rate_limiter: LoginRateLimiter::default(),
    };

    // Start the 30-day retention purge task (runs every hour).
    routes::spawn_retention_task(state.clone()).await;

    let app = routes::router(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

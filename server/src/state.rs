use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};

use easyclean_core::db::Db;
use easyclean_core::Config;

use crate::routes::LoginRateLimiter;

/// Shared application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub scan: ScanProgress,
    pub rate_limiter: LoginRateLimiter,
}

/// Live progress of a background scan+detection run.
#[derive(Clone, Default)]
pub struct ScanProgress {
    pub running: Arc<AtomicBool>,
    pub added: Arc<AtomicUsize>,
    pub processed: Arc<AtomicUsize>,
    pub blurry: Arc<AtomicUsize>,
    pub dup_groups: Arc<AtomicUsize>,
    pub thumbs: Arc<AtomicUsize>,
    pub error: Arc<Mutex<Option<String>>>,
}

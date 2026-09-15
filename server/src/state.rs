use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};

use easyclean_core::db::Db;
use easyclean_core::embed::Clip;
use easyclean_core::Config;

use crate::routes::LoginRateLimiter;

/// Shared application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub scan: ScanProgress,
    pub rate_limiter: LoginRateLimiter,
    /// Semantic-search index build (download + embed).
    pub index: IndexProgress,
    /// The CLIP pair, loaded on first use.
    ///
    /// Lazy because the weights are ~150MB on disk and about a second to load,
    /// and an install that never searches semantically should never pay either.
    pub clip: Arc<tokio::sync::OnceCell<Arc<Clip>>>,
}

/// Live progress of a semantic-index build.
///
/// Two phases share one struct because the UI shows one bar: first the model
/// download (bytes), then the embedding pass (files).
#[derive(Clone, Default)]
pub struct IndexProgress {
    pub running: Arc<AtomicBool>,
    /// "download" | "embed" | "" when idle.
    pub phase: Arc<Mutex<String>>,
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub error: Arc<Mutex<Option<String>>>,
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

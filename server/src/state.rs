use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};

use easyclean_core::db::Db;
use easyclean_core::embed::Clip;
use easyclean_core::upload::UploadManager;
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
    /// In-flight chunked uploads.
    pub uploads: UploadManager,
    /// Debounces the post-upload processing pass.
    ///
    /// A batch of 200 photos finishes 200 times; running detection and
    /// embedding on each would re-walk the library 200 times over. The token
    /// here is bumped on every `finish`, and the pass only runs if it is still
    /// the newest one when the wait expires — so one batch costs one pass, and
    /// a user who keeps adding files keeps pushing it back rather than
    /// stacking up work.
    pub upload_settle: UploadSettle,
    /// The CLIP pair, loaded on first use.
    ///
    /// Lazy because the weights are ~150MB on disk and about a second to load,
    /// and an install that never searches semantically should never pay either.
    pub clip: Arc<tokio::sync::OnceCell<Arc<Clip>>>,
}

/// Per-user generation counter for the debounced post-upload pass.
#[derive(Clone, Default)]
pub struct UploadSettle {
    pub generation: Arc<Mutex<std::collections::HashMap<String, u64>>>,
}

impl UploadSettle {
    /// Record another finished upload for `user`, returning the token this
    /// waiter must still hold when it wakes.
    pub fn bump(&self, user: &str) -> u64 {
        let mut map = self.generation.lock().unwrap();
        let slot = map.entry(user.to_string()).or_insert(0);
        *slot += 1;
        *slot
    }

    /// True when no further upload finished while this waiter was asleep.
    pub fn is_current(&self, user: &str, token: u64) -> bool {
        self.generation
            .lock()
            .unwrap()
            .get(user)
            .copied()
            .unwrap_or(0)
            == token
    }
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

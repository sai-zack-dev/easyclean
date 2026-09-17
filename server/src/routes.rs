use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::path::Path as FsPath;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{json, Value};

use easyclean_core::auth as core_auth;
use easyclean_core::categorize;
use easyclean_core::fileops;
use easyclean_core::models::MediaFile;
use easyclean_core::storage::UserRoot;
use easyclean_core::upload;
use easyclean_core::{db, detect, embed, scan};

use crate::auth::{AuthUser, MediaUser, VaultUser};
use crate::error::ApiError;
use crate::state::AppState;

/// Build the full API router.
pub fn router(state: AppState) -> Router {
    let web_dist = std::env::var("EASYCLEAN_WEB_DIST").unwrap_or_else(|_| "web/dist".to_string());

    let static_files = tower_http::services::ServeDir::new(&web_dist).not_found_service(
        tower_http::services::ServeFile::new(format!("{web_dist}/index.html")),
    );

    Router::new()
        .route("/api/health", get(health))
        // auth
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(me))
        .route("/api/auth/media-token", post(media_token))
        // scan
        .route("/api/scan", post(scan_start))
        .route("/api/scan/status", get(scan_status_sse))
        // upload
        .route("/api/upload/check", post(upload_check))
        .route("/api/upload/init", post(upload_init))
        .route("/api/upload/quota", get(upload_quota))
        .route("/api/upload/:id", get(upload_status).delete(upload_abort))
        .route("/api/upload/:id/finish", post(upload_finish))
        // The one route allowed a large body, and only just large enough for
        // one chunk. Everything else in this router keeps axum's 2 MB default —
        // raising the limit globally would mean any endpoint could be made to
        // buffer megabytes, which is a denial of service with extra steps.
        .route(
            "/api/upload/:id/chunk",
            put(upload_chunk).layer(DefaultBodyLimit::max(chunk_body_limit(&state.cfg))),
        )
        // media
        .route("/api/media/batch", get(batch))
        .route("/api/media/duplicates", get(duplicate_groups))
        .route("/api/media/duplicates/resolve", post(duplicates_resolve))
        .route("/api/media/swipe", post(swipe))
        .route("/api/media/session/finish", post(session_finish))
        .route("/api/media/:id/thumb", get(thumb))
        .route("/api/media/:id/preview", get(preview))
        .route("/api/media/:id/raw", get(raw))
        .route("/api/media/:id/favorite", post(favorite))
        .route("/api/media/:id/albums", get(media_albums))
        .route("/api/media/:id/trash", post(trash_one))
        // albums
        .route("/api/albums", get(albums_list))
        .route("/api/albums", post(album_create))
        .route("/api/albums/refresh", post(albums_refresh))
        .route("/api/albums/:id/files", get(album_files))
        .route("/api/albums/add", post(album_add))
        // stats
        .route("/api/stats/summary", get(stats))
        .route("/api/stats/streak", get(streak))
        .route("/api/stats/timeline", get(timeline))
        .route("/api/stats/storage", get(storage))
        // semantic search
        .route("/api/search", get(search))
        .route("/api/search/status", get(search_status))
        .route("/api/search/index", post(search_index_start))
        // settings
        .route("/api/settings", get(settings_get))
        .route("/api/settings", post(settings_put))
        // the hidden vault
        .route("/api/vault/status", get(vault_status))
        .route("/api/vault/pin", post(vault_set_pin))
        .route("/api/vault/unlock", post(vault_unlock))
        .route("/api/vault/files", get(vault_list))
        .route("/api/vault/folders", get(vault_folders_list))
        .route("/api/vault/folders", post(vault_folder_create))
        .route(
            "/api/vault/folders/:id",
            patch(vault_folder_rename).delete(vault_folder_delete),
        )
        .route("/api/vault/move", post(vault_move))
        .route("/api/vault/add", post(vault_add))
        .route("/api/vault/restore", post(vault_restore))
        .route("/api/vault/delete", post(vault_delete))
        .route("/api/vault/:id/thumb", get(vault_thumb))
        .route("/api/vault/:id/raw", get(vault_raw))
        // trash
        .route("/api/trash", get(trash_list))
        .route("/api/trash/restore/:id", post(trash_restore))
        .route("/api/trash/purge/:id", delete(trash_purge_one))
        .route("/api/trash/purge-all", delete(trash_purge_all))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors_layer(&state.cfg))
        .with_state(state)
        .fallback_service(static_files)
        .layer(axum::middleware::from_fn(static_cache_control))
}

/// Cache policy for the static shell.
///
/// Without this the browser applies its own heuristic freshness to
/// `index.html` and `sw.js`, and an installed PWA goes on serving the build it
/// first saw — the update never arrives, and reinstalling the app is the only
/// way out. Two rules fix that:
///
///   - `/assets/*` is content-hashed by Vite, so it is immutable and cached
///     for a year; a new build means new filenames.
///   - everything else (the HTML shell, `sw.js`, the manifest, icons) is
///     `no-cache`: still cached, but revalidated every time, so a deploy is
///     picked up on the next launch.
///
/// API responses are left alone — they set their own headers, and the service
/// worker never caches them.
async fn static_cache_control(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;

    // Security headers go on everything, API responses included — they cost
    // nothing and the API is the half worth framing or sniffing.
    let h = res.headers_mut();
    // `frame-ancestors 'none'`: the destructive controls here are one tap and
    // no confirmation beyond the app's own dialog, so a page that can frame
    // this one can trick someone into emptying their trash for good.
    // `X-Frame-Options` as well, for anything that predates CSP level 2.
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // The token rides in query strings for `<img>` and `EventSource`, so a
    // referrer would carry it to any host the page links out to.
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );

    if path.starts_with("/api/") {
        return res;
    }

    // CSP on the shell only. The session JWT lives in localStorage, which means
    // any script that runs on this origin is a permanent account takeover
    // rather than a session one — this is what keeps an injected <script> from
    // having anywhere to load from. `img-src` allows blob: for the share sheet,
    // which builds object URLs from fetched originals.
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; \
             img-src 'self' blob: data:; \
             media-src 'self' blob:; \
             style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
             font-src 'self' https://fonts.gstatic.com; \
             connect-src 'self'; \
             object-src 'none'; \
             base-uri 'none'; \
             form-action 'none'; \
             frame-ancestors 'none'",
        ),
    );

    let policy = if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    res
}

/// CORS policy. The UI is served by this same server, so the default is
/// same-origin only: an empty `CorsLayer` emits no `Access-Control-Allow-*`,
/// which keeps any other page on the LAN from reading the API with a stolen
/// token. `EASYCLEAN_CORS_ORIGINS` opens it up for split dev setups.
fn cors_layer(cfg: &easyclean_core::Config) -> tower_http::cors::CorsLayer {
    use tower_http::cors::{Any, CorsLayer};

    if cfg.cors_origins.is_empty() {
        return CorsLayer::new();
    }
    if cfg.cors_origins.iter().any(|o| o == "*") {
        tracing::warn!("EASYCLEAN_CORS_ORIGINS=* — any website may call this API");
        return CorsLayer::permissive();
    }

    let origins: Vec<HeaderValue> = cfg
        .cors_origins
        .iter()
        .filter_map(|o| match HeaderValue::from_str(o) {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::warn!("ignoring malformed CORS origin {o:?}");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any)
}

async fn health() -> Json<Value> {
    // The build the server is actually running. The PWA shows this in Setting
    // and compares it with its own compiled-in version, so "did my deploy
    // reach this phone?" is answerable without reinstalling the app.
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

// ── Login rate-limiter ────────────────────────────────────────────────────

/// Simple in-process per-IP rate limiter: max 5 attempts / 15 min.
#[derive(Clone, Default)]
pub struct LoginRateLimiter(Arc<Mutex<HashMap<String, (u32, Instant)>>>);

impl LoginRateLimiter {
    const MAX: u32 = 5;
    const WINDOW: Duration = Duration::from_secs(15 * 60);

    pub fn check(&self, ip: &str) -> bool {
        let mut map = self.0.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= Self::WINDOW {
            *entry = (0, now);
        }
        entry.0 < Self::MAX
    }

    pub fn record(&self, ip: &str) {
        let mut map = self.0.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= Self::WINDOW {
            *entry = (1, now);
        } else {
            entry.0 += 1;
        }
    }
}

// ── Auth ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    passcode: String,
}

async fn login(
    State(st): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(b): Json<LoginReq>,
) -> Result<Json<Value>, ApiError> {
    let ip = addr.ip().to_string();
    if !st.rate_limiter.check(&ip) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts".into(),
        ));
    }
    let result = db::get_user(&st.db, &b.username).await?;
    let (username, _hash, role) = match result {
        Some(r) if core_auth::verify_passcode(&b.passcode, &r.1) => r,
        _ => {
            st.rate_limiter.record(&ip);
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "invalid credentials".into(),
            ));
        }
    };
    let token = core_auth::make_token(&username, &role, &st.cfg.secret, st.cfg.jwt_expire_days)?;
    Ok(Json(json!({
        "access_token": token,
        "token_type": "Bearer",
        "user": { "username": username, "role": role }
    })))
}

async fn me(user: AuthUser) -> Json<Value> {
    Json(json!({ "username": user.username, "role": user.role }))
}

/// How long a media token is good for. An hour: long enough to browse without
/// the client thinking about it, short enough that one copied out of a log or a
/// proxy is usually already dead.
const MEDIA_TOKEN_MINUTES: i64 = 60;

/// Trade the login for a read-only token that is safe to put in a URL.
///
/// Proved with the `Authorization` header — the login itself never travels in a
/// query string — and what comes back can only read this user's thumbnails,
/// previews, originals and scan progress.
async fn media_token(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let token = core_auth::make_media_token(&user.username, &st.cfg.secret, MEDIA_TOKEN_MINUTES)
        .map_err(ApiError::internal)?;
    Ok(Json(json!({
        "media_token": token,
        "expires_in": MEDIA_TOKEN_MINUTES * 60,
    })))
}

// ── Scan + SSE ────────────────────────────────────────────────────────────

async fn scan_start(State(st): State<AppState>, user: AuthUser) -> Json<Value> {
    if st.scan.running.swap(true, Ordering::SeqCst) {
        return Json(json!({ "started": false, "reason": "already running" }));
    }
    for c in [
        &st.scan.added,
        &st.scan.processed,
        &st.scan.blurry,
        &st.scan.dup_groups,
        &st.scan.thumbs,
    ] {
        c.store(0, Ordering::Relaxed);
    }
    *st.scan.error.lock().unwrap() = None;
    tokio::spawn(run_scan(st.clone(), user.username));
    Json(json!({ "started": true }))
}

async fn run_scan(st: AppState, user: String) {
    let root = match UserRoot::new(&st.cfg.media_root, &user) {
        Ok(r) => {
            let _ = r.ensure();
            r
        }
        Err(e) => {
            *st.scan.error.lock().unwrap() = Some(e.to_string());
            st.scan.running.store(false, Ordering::SeqCst);
            return;
        }
    };
    let thumb_dir = st.cfg.data_dir.join("thumbs");
    let sp = st.scan.clone();
    if let Err(e) = scan::scan_folder(&st.db, &user, root.root(), move |n| {
        sp.added.store(n, Ordering::Relaxed);
    })
    .await
    {
        *st.scan.error.lock().unwrap() = Some(format!("scan: {e}"));
        st.scan.running.store(false, Ordering::SeqCst);
        return;
    }
    let sp = st.scan.clone();
    let dup_distance = resolve_dup_distance(&st, &user).await;
    match detect::run_detection(
        &st.db,
        &user,
        st.cfg.blur_threshold,
        &thumb_dir,
        dup_distance,
        move |n| sp.processed.store(n, Ordering::Relaxed),
    )
    .await
    {
        Ok(d) => {
            st.scan.blurry.store(d.blurry, Ordering::Relaxed);
            st.scan
                .dup_groups
                .store(d.duplicate_groups, Ordering::Relaxed);
            st.scan.thumbs.store(d.thumbs, Ordering::Relaxed);
        }
        Err(e) => *st.scan.error.lock().unwrap() = Some(format!("detect: {e}")),
    }
    st.scan.running.store(false, Ordering::SeqCst);
}

/// SSE endpoint: streams a `progress` event every second while a scan runs,
/// then a final `done` event. `EventSource` can't set headers, so clients
/// authenticate with `new EventSource('/api/scan/status?token=<jwt>')`.
async fn scan_status_sse(
    State(st): State<AppState>,
    _user: MediaUser,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let s = stream::unfold((st, false), |(st, done)| async move {
        if done {
            return None;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let sp = &st.scan;
        let running = sp.running.load(Ordering::Relaxed);
        let data = json!({
            "running": running,
            "added":   sp.added.load(Ordering::Relaxed),
            "processed": sp.processed.load(Ordering::Relaxed),
            "blurry":  sp.blurry.load(Ordering::Relaxed),
            "duplicate_groups": sp.dup_groups.load(Ordering::Relaxed),
            "thumbs":  sp.thumbs.load(Ordering::Relaxed),
            "error":   sp.error.lock().unwrap().clone(),
        });
        let event_type = if running { "progress" } else { "done" };
        let ev = Event::default().event(event_type).data(data.to_string());
        Some((Ok(ev), (st, !running)))
    });

    Sse::new(s).keep_alive(KeepAlive::default())
}

// ── Upload ────────────────────────────────────────────────────────────────

/// Most hashes one `check` may ask about.
///
/// A phone with 40,000 photos must not put 40,000 hashes in one request: the
/// body alone would be 2.6 MB and the reply would hold the lot in memory twice
/// over. Clients page through their library instead, and the limit is what
/// makes them.
const MAX_CHECK_HASHES: usize = 5_000;

/// How long after the last finished upload the processing pass waits.
///
/// Long enough that a batch arriving back-to-back settles into one pass, short
/// enough that a single photo is thumbnailed while the user is still looking at
/// the screen they uploaded it from.
const UPLOAD_SETTLE_SECS: u64 = 5;

#[derive(Deserialize)]
struct CheckReq {
    hashes: Vec<String>,
}

/// Which of these files does the server already have?
///
/// The first call of any upload batch, and the one that makes re-offering a
/// whole camera roll cheap: the client sends hashes, and sends bytes only for
/// what comes back in `need`.
async fn upload_check(
    State(st): State<AppState>,
    user: AuthUser,
    Json(b): Json<CheckReq>,
) -> Result<Json<Value>, ApiError> {
    if b.hashes.len() > MAX_CHECK_HASHES {
        return Err(ApiError::bad(&format!(
            "too many hashes in one request (max {MAX_CHECK_HASHES})"
        )));
    }
    // Normalize up front so a client sending uppercase digests gets an answer
    // rather than a silent "you have none of these" — the failure mode of
    // comparing unnormalized hex against a lowercase column.
    let hashes = b
        .hashes
        .iter()
        .map(|h| upload::normalize_hash(h))
        .collect::<Result<Vec<_>, _>>()?;

    let have = db::hashes_present(&st.db, &user.username, &hashes).await?;
    let need: Vec<&String> = hashes.iter().filter(|h| !have.contains(*h)).collect();
    Ok(Json(json!({
        "have": have.iter().collect::<Vec<_>>(),
        "need": need,
    })))
}

#[derive(Deserialize)]
struct InitReq {
    filename: String,
    size: u64,
    hash: String,
}

/// Open a session: reserves quota and disk, returns where to start writing.
async fn upload_init(
    State(st): State<AppState>,
    user: AuthUser,
    Json(b): Json<InitReq>,
) -> Result<Json<Value>, ApiError> {
    let init = st
        .uploads
        .init(
            &st.cfg,
            &st.db,
            &user.username,
            &b.filename,
            b.size,
            &b.hash,
        )
        .await?;
    Ok(Json(json!({
        "id": init.id,
        "offset": init.offset,
        "chunk_size": init.chunk_size,
        "filename": init.filename,
    })))
}

/// Where a resuming client should continue from.
async fn upload_status(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let offset = st.uploads.offset(&id, &user.username).await?;
    Ok(Json(json!({ "id": id, "offset": offset })))
}

#[derive(Deserialize)]
struct ChunkQuery {
    offset: u64,
}

/// Append one chunk at an explicit offset.
///
/// The body arrives whole — the route's own body limit is what bounds it — so
/// either the entire chunk is here or the extractor rejected it before this
/// function ran. That is the point: there is no path that writes half a chunk
/// and leaves the session's offset describing a file that no longer matches it.
async fn upload_chunk(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<ChunkQuery>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    let offset = st
        .uploads
        .write_chunk(&id, &user.username, q.offset, &body)
        .await?;
    Ok(Json(json!({ "id": id, "offset": offset })))
}

/// Verify and file the completed upload, then nudge the processing pass.
async fn upload_finish(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let done = st
        .uploads
        .finish(&st.cfg, &st.db, &id, &user.username)
        .await?;

    let token = st.upload_settle.bump(&user.username);
    tokio::spawn(settle_after_uploads(st.clone(), user.username, token));

    Ok(Json(json!({
        "file_id": done.file_id,
        "filename": done.filename,
        "size": done.size,
    })))
}

/// Abandon a session and delete what it had written.
async fn upload_abort(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    st.uploads.abort(&id, &user.username).await?;
    Ok(Json(json!({ "aborted": true })))
}

/// How much room this user has, for the UI to show before a batch starts.
async fn upload_quota(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let used = db::bytes_used(&st.db, &user.username).await?;
    let limit = upload::quota_bytes(&st.db, &st.cfg, &user.username).await?;
    Ok(Json(json!({
        "used": used,
        // Null rather than a huge number: "unlimited" is a different thing from
        // "a very big quota", and a UI drawing a progress bar needs to know
        // which it is looking at.
        "limit": limit,
        "chunk_size": st.cfg.upload_chunk_size,
    })))
}

/// Thumbnail, score and index whatever the last batch brought in.
///
/// Debounced: each finished upload bumps a token, and this only proceeds if no
/// newer upload arrived while it slept. Two hundred photos therefore cost one
/// detection pass rather than two hundred.
async fn settle_after_uploads(st: AppState, user: String, token: u64) {
    tokio::time::sleep(Duration::from_secs(UPLOAD_SETTLE_SECS)).await;
    if !st.upload_settle.is_current(&user, token) {
        return; // a later upload owns the pass now
    }
    // A scan already running will pick these files up itself — detection works
    // from "rows without a perceptual hash", not from a list this function
    // holds — so stepping aside costs nothing and avoids two passes competing
    // for the same rows.
    if st.scan.running.swap(true, Ordering::SeqCst) {
        return;
    }
    *st.scan.error.lock().unwrap() = None;

    let thumb_dir = st.cfg.data_dir.join("thumbs");
    let sp = st.scan.clone();
    let dup_distance = resolve_dup_distance(&st, &user).await;
    match detect::run_detection(
        &st.db,
        &user,
        st.cfg.blur_threshold,
        &thumb_dir,
        dup_distance,
        move |n| sp.processed.store(n, Ordering::Relaxed),
    )
    .await
    {
        Ok(d) => {
            st.scan.blurry.store(d.blurry, Ordering::Relaxed);
            st.scan
                .dup_groups
                .store(d.duplicate_groups, Ordering::Relaxed);
            st.scan.thumbs.store(d.thumbs, Ordering::Relaxed);
        }
        Err(e) => *st.scan.error.lock().unwrap() = Some(format!("detect: {e}")),
    }
    st.scan.running.store(false, Ordering::SeqCst);

    // Semantic search too, but only once the weights are on disk. Pulling
    // ~150 MB because somebody uploaded a photo is not a decision to make on
    // their behalf over a home connection — the first download stays an
    // explicit choice in Settings, and every upload after it is automatic.
    if embed::is_downloaded(&st.cfg.data_dir) && !st.index.running.swap(true, Ordering::SeqCst) {
        st.index.done.store(0, Ordering::Relaxed);
        st.index.total.store(0, Ordering::Relaxed);
        *st.index.error.lock().unwrap() = None;
        tokio::spawn(run_index(st.clone(), user));
    }
}

/// Reclaim sessions whose client walked away, hourly.
pub async fn spawn_upload_sweeper(state: AppState) {
    // Anything on disk from before this process started belongs to sessions
    // that no longer exist, so it can never be resumed — only take up the free
    // space the disk floor is there to protect.
    if let Err(e) = easyclean_core::upload::purge_orphans(&state.cfg) {
        tracing::warn!("could not clear stale upload scratch files: {e}");
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
            let n = state.uploads.sweep(upload::SESSION_IDLE_SECS).await;
            if n > 0 {
                tracing::info!("reclaimed {n} abandoned upload session(s)");
            }
        }
    });
}

// ── Media ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BatchQuery {
    mode: Option<String>,
    month: Option<String>,
    size: Option<i64>,
    /// 1-based page. The Gallery pages through a library of any size rather
    /// than asking for all of it at once.
    page: Option<i64>,
    /// Free text, matched against filename and library path. Searching here
    /// rather than in the browser is the point: the client only ever holds the
    /// pages it has scrolled through, so filtering there missed the rest.
    q: Option<String>,
    /// `image` or `video`.
    #[serde(rename = "type")]
    media_type: Option<String>,
}

async fn batch(
    State(st): State<AppState>,
    user: AuthUser,
    Query(q): Query<BatchQuery>,
) -> Result<Json<Value>, ApiError> {
    let size = q.size.unwrap_or(50).clamp(1, 500);
    let page = q.page.unwrap_or(1).max(1);
    // Reject anything that isn't a media type we index, rather than quietly
    // returning nothing for a typo.
    let media_type = match q.media_type.as_deref() {
        Some("image") | Some("video") => q.media_type.as_deref(),
        Some(other) if !other.is_empty() => {
            return Err(ApiError::bad("type must be 'image' or 'video'"))
        }
        _ => None,
    };
    let query = db::BatchQuery {
        mode: q.mode.as_deref().unwrap_or("timeline"),
        month: q.month.as_deref(),
        q: q.q.as_deref(),
        media_type,
    };
    let files = db::get_batch(&st.db, &user.username, &query, size, page).await?;
    // `total` is every match, not just this page — the review screen shows it
    // as a progress denominator and the Gallery as a result count.
    let total = db::count_batch(&st.db, &user.username, &query).await?;
    Ok(Json(json!({ "files": enrich(files), "total": total })))
}

/// Every un-reviewed duplicate cluster, whole.
///
/// The compare screen shows a group at a time and asks which copies to keep, so
/// it needs the members together — a paged flat batch could split a pair across
/// two pages and ask about each copy in isolation, which is what made the old
/// duplicate deck no better than an ordinary swipe.
async fn duplicate_groups(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let files = db::duplicate_clusters(&st.db, &user.username).await?;

    // The query already returns a group's members contiguously, so one pass
    // folds them into groups without a hash map or a second sort.
    let total_files = files.len();
    let mut groups: Vec<Value> = Vec::new();
    let mut gid = String::new();
    let mut members: Vec<MediaFile> = Vec::new();
    for f in files {
        let g = f.group_id.clone().unwrap_or_default();
        if g != gid && !members.is_empty() {
            let done = std::mem::take(&mut members);
            groups.push(json!({ "group_id": gid, "files": enrich(done) }));
        }
        gid = g;
        members.push(f);
    }
    if !members.is_empty() {
        groups.push(json!({ "group_id": gid, "files": enrich(members) }));
    }

    let total_groups = groups.len();
    Ok(Json(
        json!({ "groups": groups, "total_groups": total_groups, "total_files": total_files }),
    ))
}

/// Resolve one duplicate cluster in a single call.
///
/// `keep` and `delete` together cover the whole group — the screen never sends
/// a partial decision, because a copy left `pending` would make the same group
/// come back on the next pass. `undo` is the group-level counterpart: one tap
/// put several files away, so one tap has to bring them all back.
#[derive(Deserialize)]
struct DupResolveReq {
    #[serde(default)]
    keep: Vec<i64>,
    #[serde(default)]
    delete: Vec<i64>,
    #[serde(default)]
    undo: Vec<i64>,
}

async fn duplicates_resolve(
    State(st): State<AppState>,
    user: AuthUser,
    Json(r): Json<DupResolveReq>,
) -> Result<Json<Value>, ApiError> {
    if r.keep.is_empty() && r.delete.is_empty() && r.undo.is_empty() {
        return Err(ApiError::bad("nothing to resolve"));
    }
    // Undo is its own request shape; mixing it with a decision would mean two
    // opposite intents racing over the same rows.
    if !r.undo.is_empty() && !(r.keep.is_empty() && r.delete.is_empty()) {
        return Err(ApiError::bad("send 'undo' on its own"));
    }
    let now = now_secs();

    if !r.undo.is_empty() {
        let restored =
            db::set_status_many(&st.db, &user.username, &r.undo, "pending", None).await?;
        for _ in 0..restored {
            db::unlog_last_swipe(&st.db, &user.username).await?;
        }
        refresh_albums_for(&st, &r.undo).await?;
        return Ok(Json(json!({ "restored": restored })));
    }

    let kept = db::set_status_many(&st.db, &user.username, &r.keep, "keep", None).await?;
    let deleted =
        db::set_status_many(&st.db, &user.username, &r.delete, "trashed", Some(now)).await?;
    // Each copy decided is a swipe, so the streak counts the work honestly
    // whether it was done one card at a time or a group at a time.
    db::log_swipes(&st.db, &user.username, (kept + deleted) as usize, now).await?;
    let touched: Vec<i64> = r.keep.iter().chain(r.delete.iter()).copied().collect();
    refresh_albums_for(&st, &touched).await?;
    Ok(Json(json!({ "kept": kept, "deleted": deleted })))
}

#[derive(Deserialize)]
struct SwipeReq {
    file_id: i64,
    action: String,
}

async fn swipe(
    State(st): State<AppState>,
    user: AuthUser,
    Json(s): Json<SwipeReq>,
) -> Result<Json<Value>, ApiError> {
    let now = now_secs();

    // `undo` puts the file back in the deck and drops the swipe from the
    // streak log. Safe before a session finish; afterwards the file has
    // already moved, and it's restored from the trash screen instead.
    if s.action == "undo" {
        db::set_status(&st.db, &user.username, s.file_id, "pending", None).await?;
        db::unlog_last_swipe(&st.db, &user.username).await?;
        // Undoing a delete puts the photo back on the shelves it was on.
        refresh_albums_for(&st, &[s.file_id]).await?;
        return Ok(Json(json!({ "id": s.file_id, "status": "pending" })));
    }

    let status = match s.action.as_str() {
        "keep" => "keep",
        "delete" => "trashed",
        _ => return Err(ApiError::bad("action must be 'keep', 'delete' or 'undo'")),
    };
    let trashed_at = (status == "trashed").then_some(now);
    db::set_status(&st.db, &user.username, s.file_id, status, trashed_at).await?;
    db::log_swipes(&st.db, &user.username, 1, now).await?;
    // A swiped-away photo leaves its albums in the same breath, rather than
    // sitting in one until the session is finished — or, for an album no
    // rebuild ever touches, for good.
    refresh_albums_for(&st, &[s.file_id]).await?;
    Ok(Json(json!({ "id": s.file_id, "status": status })))
}

/// Finish a review session: physically move kept files to `clean/<album>/`
/// and run album categorization.
async fn session_finish(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    root.ensure().map_err(ApiError::internal)?;

    let kept = db::kept_files(&st.db, &user.username).await?;
    let mut moved = 0usize;
    let mut albums_updated: std::collections::HashSet<i64> = Default::default();
    let now = now_secs();
    let clean_dir = root.clean();
    let clean_dir = clean_dir.as_path();

    for mf in &kept {
        let cats = categorize::categories_for(mf);
        let primary = cats.first().cloned().unwrap_or_else(|| "other".to_string());
        let album_name = categorize::album_name(&primary);
        let cat_type = categorize::cat_type(&primary).to_string();

        // Physical move source → clean/<album_name>/. Files an earlier finish
        // already filed are left alone — re-moving them inside clean/ would
        // rename each one (`x.jpg` → `x_1.jpg`) on every run.
        let src = FsPath::new(&mf.path);
        if src.exists() && !src.starts_with(clean_dir) {
            if let Ok(dest) = fileops::to_clean(&root, src, &album_name) {
                db::update_path(
                    &st.db,
                    mf.id,
                    &dest.to_string_lossy(),
                    &rel_to_root(&root, &dest),
                )
                .await?;
                moved += 1;
            }
        }

        // Add to every applicable album.
        for cat_key in &cats {
            let aname = categorize::album_name(cat_key);
            let atype = categorize::cat_type(cat_key).to_string();
            let aid = db::get_or_create_album(&st.db, &user.username, &aname, &atype, now).await?;
            db::add_to_album(&st.db, aid, mf.id).await?;
            albums_updated.insert(aid);
        }

        // Drop the per-file album created above if it differs.
        let _ = cat_type; // suppress lint
        let _ = album_name;
    }

    // Refresh counts for every touched album.
    for aid in albums_updated {
        db::refresh_album_meta(&st.db, aid).await?;
    }

    // Trash-first: deleted files leave source/ for .trash/ in the same pass,
    // where they sit until an explicit purge or the retention sweep.
    let trash_dir = root.trash();
    let mut trashed = 0usize;
    for mf in db::list_status(&st.db, &user.username, "trashed").await? {
        let src = FsPath::new(&mf.path);
        // Skip anything already moved by an earlier finish.
        if !src.exists() || src.starts_with(&trash_dir) {
            continue;
        }
        match fileops::to_trash(&root, src) {
            Ok(dest) => {
                db::update_path(
                    &st.db,
                    mf.id,
                    &dest.to_string_lossy(),
                    &rel_to_root(&root, &dest),
                )
                .await?;
                trashed += 1;
            }
            Err(e) => tracing::warn!("finish: trashing {}: {e}", mf.path),
        }
    }

    Ok(Json(
        json!({ "moved": moved, "trashed": trashed, "categorized": kept.len() }),
    ))
}

/// Toggle the heart, and move the photo in or out of the Favourites album in
/// the same breath.
///
/// Album membership used to be decided only by `categorize` at session finish,
/// so hearting a photo did nothing you could see — Favourites stayed empty
/// until a review session ended, which for a photo you hearted from the Gallery
/// might be never. The heart is the whole interface for that album, so it has
/// to be the thing that fills it.
async fn favorite(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let _ = owned_media(&st, &user.username, id).await?;
    let fav = db::toggle_favorite(&st.db, &user.username, id).await?;

    let name = categorize::album_name("favorites:favorites");
    let album_id = db::get_or_create_album(
        &st.db,
        &user.username,
        &name,
        categorize::cat::FAVORITES,
        now_secs(),
    )
    .await?;
    if fav {
        db::add_to_album(&st.db, album_id, id).await?;
    } else {
        db::remove_from_album(&st.db, album_id, id).await?;
    }
    db::refresh_album_meta(&st.db, album_id).await?;

    Ok(Json(json!({ "id": id, "is_favorite": fav })))
}

/// Which albums a file is filed under. Read by the viewer's Details panel, so
/// it is fetched per photo on demand rather than joined into every batch row.
async fn media_albums(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let _ = owned_media(&st, &user.username, id).await?;
    let albums = db::albums_for_file(&st.db, &user.username, id).await?;
    Ok(Json(json!({ "albums": albums })))
}

async fn thumb(
    State(st): State<AppState>,
    user: MediaUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;
    let tp = m
        .thumb_path
        .ok_or_else(|| ApiError::not_found("no thumbnail yet"))?;
    serve_file(FsPath::new(&tp)).await
}

/// A full-screen-quality JPEG of one file — what the swipe deck and the detail
/// viewer show.
///
/// Neither the thumbnail nor the original will do: a 480px thumb is too soft to
/// judge sharpness by, and the original can be a 12 MB HEIC no browser decodes.
/// Previews are rendered once on first request and cached under
/// `<data_dir>/previews/<id>.jpg`; the pixels never change, so the cache is
/// valid even after the file moves between `source`, `clean` and `.trash`.
async fn preview(
    State(st): State<AppState>,
    user: MediaUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;

    // Video has no still to enlarge — its ffmpeg first-frame thumbnail is the
    // best preview available.
    if m.media_type != "image" {
        let tp = m
            .thumb_path
            .ok_or_else(|| ApiError::not_found("no preview for this video"))?;
        return serve_file(FsPath::new(&tp)).await;
    }

    let out = preview_path(&st, id);
    if !out.exists() {
        let src = m.path.clone();
        let dest = out.clone();
        // Decoding a full-size photo is CPU-heavy and blocking; keep it off the
        // async runtime or one preview stalls every other in-flight request.
        let ok = tokio::task::spawn_blocking(move || {
            easyclean_core::thumbs::ensure_image_preview(FsPath::new(&src), &dest)
        })
        .await
        .unwrap_or(false);

        if !ok {
            // Undecodable (corrupt, or HEIC without libheif): fall back to the
            // thumbnail so the card shows something rather than breaking.
            let tp = m
                .thumb_path
                .ok_or_else(|| ApiError::not_found("could not render a preview"))?;
            return serve_file(FsPath::new(&tp)).await;
        }
    }
    serve_file(&out).await
}

async fn raw(
    State(st): State<AppState>,
    user: MediaUser,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;
    let p = FsPath::new(&m.path);
    if !p.starts_with(&st.cfg.media_root) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "path outside media root".into(),
        ));
    }
    if !p.exists() {
        return Err(ApiError::not_found("file missing on disk"));
    }
    serve_original(p, &headers).await
}

/// Bin one file straight from the Gallery or the detail viewer.
///
/// Unlike a swipe — which only marks a decision, to be carried out when the
/// session is finished — this moves the file to `.trash/` now, because there is
/// no session to finish here and the user expects it gone. Still trash-first:
/// the bytes survive until an explicit purge or retention cleanup.
async fn trash_one(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;
    if m.status == "trashed" {
        return Err(ApiError::bad("file is already in the trash"));
    }
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    root.ensure().map_err(ApiError::internal)?;

    let src = FsPath::new(&m.path);
    if src.exists() && !src.starts_with(root.trash()) {
        let dest = fileops::to_trash(&root, src).map_err(ApiError::internal)?;
        db::update_path(
            &st.db,
            id,
            &dest.to_string_lossy(),
            &rel_to_root(&root, &dest),
        )
        .await?;
    }
    let now = now_secs();
    db::set_status(&st.db, &user.username, id, "trashed", Some(now)).await?;
    // The bin is the last place this photo is listed: the Gallery already
    // filters by status, and this is what takes it off the album shelves too.
    refresh_albums_for(&st, &[id]).await?;
    Ok(Json(
        json!({ "id": id, "status": "trashed", "trashed_at": now }),
    ))
}

// ── Albums ────────────────────────────────────────────────────────────────

async fn albums_list(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let albums = db::list_albums(&st.db, &user.username).await?;
    Ok(Json(json!({ "albums": albums })))
}

#[derive(Deserialize)]
struct AlbumFilesQuery {
    page: Option<i64>,
    size: Option<i64>,
}

async fn album_files(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Query(q): Query<AlbumFilesQuery>,
) -> Result<Json<Value>, ApiError> {
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(50).clamp(1, 200);
    let files = db::album_files(&st.db, &user.username, id, page, size).await?;
    Ok(Json(json!({ "files": enrich(files) })))
}

/// Rebuild all album metadata (counts + covers) for the user.
async fn albums_refresh(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let albums = db::list_albums(&st.db, &user.username).await?;
    for a in &albums {
        db::refresh_album_meta(&st.db, a.id).await?;
    }
    Ok(Json(json!({ "refreshed": albums.len() })))
}

/// `category_type` for an album the user made themselves, as opposed to one
/// `categorize` derives. Only these accept a manual add.
const MANUAL_ALBUM: &str = "manual";

#[derive(Deserialize)]
struct AlbumCreateReq {
    name: String,
}

/// Make an empty album.
///
/// `albums/add` can only create one as a side effect of filing a photo, which
/// is no use to the "+" in the Albums tab — there you are making the shelf
/// before you have anything to put on it.
async fn album_create(
    State(st): State<AppState>,
    user: AuthUser,
    Json(b): Json<AlbumCreateReq>,
) -> Result<Json<Value>, ApiError> {
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("an album needs a name"));
    }
    if name.chars().count() > 80 {
        return Err(ApiError::bad("that name is too long"));
    }
    let id =
        db::get_or_create_album(&st.db, &user.username, name, MANUAL_ALBUM, now_secs()).await?;
    Ok(Json(json!({ "album_id": id, "name": name })))
}

#[derive(Deserialize)]
struct AlbumAddReq {
    file_id: i64,
    /// Existing album to file it under, or a name to create one.
    album_id: Option<i64>,
    name: Option<String>,
}

/// Put one file in an album, creating the album if only a name was given.
///
/// Albums are otherwise built by `categorize` at session finish; this is the
/// manual path, for filing a photo somewhere the rules would never put it.
/// Category type `manual` keeps those out of the auto-rebuild's way.
async fn album_add(
    State(st): State<AppState>,
    user: AuthUser,
    Json(b): Json<AlbumAddReq>,
) -> Result<Json<Value>, ApiError> {
    // Ownership check: never let one user file another's photo.
    let _ = owned_media(&st, &user.username, b.file_id).await?;

    let album_id = match (b.album_id, b.name.as_deref().map(str::trim)) {
        (Some(id), _) => {
            let album = db::get_album(&st.db, &user.username, id)
                .await?
                .ok_or_else(|| ApiError::not_found("album not found"))?;
            // Auto-built albums describe what a photo *is* — its year, that it
            // is a video, that you hearted it — so hand-filing into one would
            // state something untrue and the next rebuild would undo it
            // anyway. A 2021 photo cannot be put in 2023.
            if album.category_type != MANUAL_ALBUM {
                return Err(ApiError::bad(
                    "that album is built automatically — add photos to one of your own instead",
                ));
            }
            id
        }
        (None, Some(name)) if !name.is_empty() => {
            db::get_or_create_album(&st.db, &user.username, name, MANUAL_ALBUM, now_secs()).await?
        }
        _ => return Err(ApiError::bad("need an album_id or a name")),
    };

    db::add_to_album(&st.db, album_id, b.file_id).await?;
    db::refresh_album_meta(&st.db, album_id).await?;
    Ok(Json(json!({ "album_id": album_id, "file_id": b.file_id })))
}

// ── Stats ─────────────────────────────────────────────────────────────────

async fn stats(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let s = db::stats_summary(&st.db, &user.username).await?;
    Ok(Json(serde_json::to_value(s).unwrap_or(Value::Null)))
}

async fn timeline(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let months = db::timeline_months(&st.db, &user.username).await?;
    Ok(Json(json!({ "months": months })))
}

async fn streak(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let s = db::streak_stats(&st.db, &user.username, now_secs()).await?;
    Ok(Json(serde_json::to_value(s).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct StorageQuery {
    days: Option<i64>,
}

/// Daily "storage saved" series behind the Settings chart.
async fn storage(
    State(st): State<AppState>,
    user: AuthUser,
    Query(q): Query<StorageQuery>,
) -> Result<Json<Value>, ApiError> {
    let days = q.days.unwrap_or(30);
    let series = db::storage_by_day(&st.db, &user.username, days, now_secs()).await?;
    let total: i64 = series.iter().map(|d| d.bytes).sum();
    let files: i64 = series.iter().map(|d| d.files).sum();
    Ok(Json(
        json!({ "days": series, "total_bytes": total, "total_files": files }),
    ))
}

// ── Semantic search ───────────────────────────────────────────────────────

/// How many nearest photos a semantic query returns.
///
/// 60 was too low to be honest about: the z-filter below keeps roughly the top
/// sixth of the library for any query, so on a large library *every* search
/// truncated to exactly 60 and the result count told you nothing. The response
/// now also carries `semantic_total` — how many passed the filter before this
/// cut — so the UI can say "300 of 1,240" instead of implying 60 is all there
/// is. Still capped, because the payload is whole file rows and the grid pages
/// through them.
const SEMANTIC_LIMIT: usize = 300;

/// Keep candidates standing this many standard deviations above the mean score.
///
/// CLIP similarities bunch between roughly 0.15 and 0.30 no matter what you ask,
/// so an absolute cut-off is meaningless — 0.22 is a strong match for one query
/// and noise for another. What separates a hit from a miss is how far the best
/// score stands out *from the rest of this library, for this query*.
const Z_KEEP: f32 = 1.0;

/// Below this, the top result doesn't stand out enough to call it a match.
///
/// Measured: subjects actually present scored z = 2.3 to 2.7, while gibberish
/// and absent-but-valid subjects ("a giraffe in a spacesuit") scored 1.5 to 1.6.
/// Results below this are still returned — labelled as guesses rather than
/// hidden, because a ranking that silently returns nothing is worse than one
/// that admits it is unsure.
const Z_CONFIDENT: f32 = 2.0;

/// Get the model, loading it once on first use.
async fn clip_model(st: &AppState) -> Result<Arc<embed::Clip>, ApiError> {
    let data_dir = st.cfg.data_dir.clone();
    let cell = st.clip.clone();
    cell.get_or_try_init(|| async move {
        tokio::task::spawn_blocking(move || embed::Clip::load(&data_dir))
            .await
            .map_err(|e| ApiError::internal(format!("model load panicked: {e}")))?
            .map(Arc::new)
            .map_err(ApiError::internal)
    })
    .await
    .cloned()
}

async fn search_status(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let (indexed, total) = db::embedding_stats(&st.db, &user.username, embed::MODEL_ID).await?;
    let running = st.index.running.load(Ordering::Relaxed);
    Ok(Json(json!({
        "model_ready": embed::is_downloaded(&st.cfg.data_dir),
        "download_bytes": embed::DOWNLOAD_BYTES,
        "indexed": indexed,
        "indexable": total,
        "running": running,
        "phase": st.index.phase.lock().ok().map(|p| p.clone()).unwrap_or_default(),
        "done": st.index.done.load(Ordering::Relaxed),
        "total": st.index.total.load(Ordering::Relaxed),
        "error": st.index.error.lock().ok().and_then(|e| e.clone()),
    })))
}

/// Kick off "download the model if needed, then embed everything un-embedded".
async fn search_index_start(State(st): State<AppState>, user: AuthUser) -> Json<Value> {
    if st.index.running.swap(true, Ordering::SeqCst) {
        return Json(json!({ "started": false, "reason": "already running" }));
    }
    st.index.done.store(0, Ordering::Relaxed);
    st.index.total.store(0, Ordering::Relaxed);
    *st.index.error.lock().unwrap() = None;
    tokio::spawn(run_index(st.clone(), user.username));
    Json(json!({ "started": true }))
}

async fn run_index(st: AppState, user: String) {
    let set_phase = |p: &str| {
        if let Ok(mut g) = st.index.phase.lock() {
            *g = p.to_string();
        }
    };
    let fail = |msg: String| {
        if let Ok(mut g) = st.index.error.lock() {
            *g = Some(msg);
        }
    };

    // ── 1. Weights
    if !embed::is_downloaded(&st.cfg.data_dir) {
        set_phase("download");
        let data_dir = st.cfg.data_dir.clone();
        let (done, total) = (st.index.done.clone(), st.index.total.clone());
        let res = tokio::task::spawn_blocking(move || {
            embed::download(&data_dir, |d, t| {
                done.store(d as usize, Ordering::Relaxed);
                total.store(t as usize, Ordering::Relaxed);
            })
        })
        .await;
        match res {
            Ok(Ok(())) => tracing::info!("CLIP weights downloaded"),
            Ok(Err(e)) => {
                fail(format!("download: {e}"));
                set_phase("");
                st.index.running.store(false, Ordering::SeqCst);
                return;
            }
            Err(e) => {
                fail(format!("download panicked: {e}"));
                set_phase("");
                st.index.running.store(false, Ordering::SeqCst);
                return;
            }
        }
    }

    // ── 2. Embed
    set_phase("embed");
    let clip = match clip_model(&st).await {
        Ok(c) => c,
        Err(e) => {
            fail(format!("loading model: {}", e.1));
            set_phase("");
            st.index.running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let pending = match db::files_needing_embedding(&st.db, &user, embed::MODEL_ID).await {
        Ok(p) => p,
        Err(e) => {
            fail(format!("listing files: {e}"));
            set_phase("");
            st.index.running.store(false, Ordering::SeqCst);
            return;
        }
    };
    st.index.done.store(0, Ordering::Relaxed);
    st.index.total.store(pending.len(), Ordering::Relaxed);

    let mut n = 0usize;
    for (id, thumb) in pending {
        let clip = clip.clone();
        // Embed from the cached thumbnail: CLIP downsamples to 224 anyway, so
        // re-decoding the original would cost far more for identical pixels.
        let embedded = tokio::task::spawn_blocking(move || {
            embed::embed_image_at(&clip, std::path::Path::new(&thumb))
        })
        .await;

        match embedded {
            Ok(Ok(vec)) => {
                let (bytes, scale) = embed::quantize(&vec);
                if let Err(e) = db::upsert_embedding(
                    &st.db,
                    id,
                    embed::MODEL_ID,
                    vec.len() as i64,
                    scale as f64,
                    &bytes,
                    now_secs(),
                )
                .await
                {
                    tracing::warn!("storing embedding for {id}: {e}");
                }
            }
            // One unreadable thumbnail must not abandon the whole pass.
            Ok(Err(e)) => tracing::warn!("embedding {id}: {e}"),
            Err(e) => tracing::warn!("embedding {id} panicked: {e}"),
        }
        n += 1;
        st.index.done.store(n, Ordering::Relaxed);
    }

    set_phase("");
    st.index.running.store(false, Ordering::SeqCst);
    tracing::info!("semantic index: embedded {n} file(s)");
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    mode: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
}

/// One search box, two kinds of answer.
///
/// Exact name/path matches come first — that search always works and is what
/// you want when you know the filename. Photos that merely *look* like the
/// query follow, ranked, and only when an index exists. Anything already listed
/// as a name match is not repeated.
async fn search(
    State(st): State<AppState>,
    user: AuthUser,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let text = q.q.trim();
    if text.is_empty() {
        return Err(ApiError::bad("q must not be empty"));
    }
    let media_type = match q.media_type.as_deref() {
        Some("image") | Some("video") => q.media_type.as_deref(),
        Some(other) if !other.is_empty() => {
            return Err(ApiError::bad("type must be 'image' or 'video'"))
        }
        _ => None,
    };
    let mode = q.mode.as_deref().unwrap_or("library");

    // ── exact matches
    let name_hits = db::get_batch(
        &st.db,
        &user.username,
        &db::BatchQuery {
            mode,
            q: Some(text),
            media_type,
            ..Default::default()
        },
        200,
        1,
    )
    .await?;
    let name_ids: std::collections::HashSet<i64> = name_hits.iter().map(|m| m.id).collect();

    // ── semantic matches
    let mut semantic: Vec<Value> = Vec::new();
    let mut semantic_total = 0usize;
    let mut searched = false;
    let mut confident = false;
    if embed::is_downloaded(&st.cfg.data_dir) {
        let rows = db::all_embeddings(&st.db, &user.username, embed::MODEL_ID).await?;
        if !rows.is_empty() {
            searched = true;
            let clip = clip_model(&st).await?;
            let phrase = text.to_string();
            let qvec = tokio::task::spawn_blocking(move || clip.embed_text(&phrase))
                .await
                .map_err(|e| ApiError::internal(format!("query encode panicked: {e}")))?
                .map_err(ApiError::internal)?;

            let mut scored: Vec<(f32, i64)> = rows
                .iter()
                .filter(|(id, _, _)| !name_ids.contains(id))
                .map(|(id, scale, v)| (embed::similarity(&qvec, v, *scale as f32), *id))
                .collect();
            // How far the best score stands out, in standard deviations across
            // the whole candidate set — computed before truncation, so the
            // baseline is the library rather than the shortlist.
            let n = scored.len().max(1) as f32;
            let mean = scored.iter().map(|(s, _)| *s).sum::<f32>() / n;
            let sd = (scored.iter().map(|(s, _)| (s - mean).powi(2)).sum::<f32>() / n)
                .sqrt()
                .max(1e-6);
            let top = scored.iter().map(|(s, _)| *s).fold(f32::MIN, f32::max);
            confident = (top - mean) / sd >= Z_CONFIDENT;

            scored.retain(|(s, _)| (s - mean) / sd >= Z_KEEP);
            scored.sort_unstable_by(|a, b| {
                b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            // Counted before truncating: this is the honest size of the result
            // set, which is what the header reports.
            semantic_total = scored.len();
            scored.truncate(SEMANTIC_LIMIT);

            let ids: Vec<i64> = scored.iter().map(|(_, id)| *id).collect();
            let files = db::media_by_ids(&st.db, &user.username, &ids).await?;
            let scores: std::collections::HashMap<i64, f32> =
                scored.iter().map(|(s, id)| (*id, *s)).collect();
            semantic = enrich(files)
                .into_iter()
                .map(|mut v| {
                    let id = v.get("id").and_then(|i| i.as_i64());
                    if let (Value::Object(map), Some(id)) = (&mut v, id) {
                        map.insert(
                            "score".into(),
                            json!(scores.get(&id).copied().unwrap_or(0.0)),
                        );
                    }
                    v
                })
                .collect();
        }
    }

    Ok(Json(json!({
        "name_matches": enrich(name_hits),
        "semantic_matches": semantic,
        // How many cleared the relevance filter, before the cap above. Equal to
        // `semantic_matches.len()` until a query genuinely has more.
        "semantic_total": semantic_total,
        "semantic_available": searched,
        // False when nothing stood out — the UI then says "closest guesses"
        // rather than claiming a match.
        "semantic_confident": confident,
    })))
}

// ── Settings ──────────────────────────────────────────────────────────────

/// Setting key for the duplicate-detection Hamming distance.
const KEY_DUP_DISTANCE: &str = "duplicate_distance";

/// Widest Hamming distance the UI may ask for. Two 64-bit dHashes that differ
/// in more than ~20 bits are not the same photo by any reading, and letting the
/// value climb further would sweep unrelated shots into one group.
const DUP_DISTANCE_MAX: u32 = 20;

/// The user's duplicate distance, falling back to
/// `EASYCLEAN_DUPLICATE_DISTANCE_THRESHOLD` when they have never set one.
async fn resolve_dup_distance(st: &AppState, user: &str) -> u32 {
    match db::get_setting(&st.db, user, KEY_DUP_DISTANCE).await {
        Ok(Some(v)) => v
            .parse::<u32>()
            .map(|d| d.min(DUP_DISTANCE_MAX))
            .unwrap_or(st.cfg.duplicate_distance_threshold),
        _ => st.cfg.duplicate_distance_threshold,
    }
}

async fn settings_get(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "duplicate_distance": resolve_dup_distance(&st, &user.username).await,
        "duplicate_distance_max": DUP_DISTANCE_MAX,
        // Operator policy, not a preference — the trash screen needs it to
        // count down how long each file has left.
        "trash_retention_days": st.cfg.trash_retention_days,
    })))
}

#[derive(Deserialize)]
struct SettingsReq {
    duplicate_distance: Option<u32>,
}

/// Save settings. Changing the duplicate distance re-clusters immediately —
/// a setting that only took effect after the next full scan would look broken.
async fn settings_put(
    State(st): State<AppState>,
    user: AuthUser,
    Json(b): Json<SettingsReq>,
) -> Result<Json<Value>, ApiError> {
    let mut groups: Option<usize> = None;

    if let Some(raw) = b.duplicate_distance {
        if raw > DUP_DISTANCE_MAX {
            return Err(ApiError::bad(&format!(
                "duplicate_distance must be 0..={DUP_DISTANCE_MAX}"
            )));
        }
        db::set_setting(&st.db, &user.username, KEY_DUP_DISTANCE, &raw.to_string()).await?;
        groups = Some(detect::regroup_duplicates(&st.db, &user.username, raw).await?);
    }

    Ok(Json(json!({
        "duplicate_distance": resolve_dup_distance(&st, &user.username).await,
        "duplicate_groups": groups,
    })))
}

// ── Trash ─────────────────────────────────────────────────────────────────

async fn trash_list(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let files = db::list_status(&st.db, &user.username, "trashed").await?;
    Ok(Json(json!({ "files": enrich(files) })))
}

async fn trash_restore(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;
    if m.status != "trashed" {
        return Err(ApiError::bad("file is not in trash"));
    }
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    root.ensure().map_err(ApiError::internal)?;

    // Only files that made it into .trash/ need moving back; one deleted but
    // never finished is still sitting in source/.
    let src = FsPath::new(&m.path);
    if src.exists() && src.starts_with(root.trash()) {
        let dest = fileops::restore(src, &root.source()).map_err(ApiError::internal)?;
        db::update_path(
            &st.db,
            id,
            &dest.to_string_lossy(),
            &rel_to_root(&root, &dest),
        )
        .await?;
    }
    db::set_status(&st.db, &user.username, id, "pending", None).await?;
    // Membership survived the deletion, so putting the file back puts it back
    // in the albums it was filed under — including manual ones, which nothing
    // would ever rebuild.
    refresh_albums_for(&st, &[id]).await?;
    Ok(Json(json!({ "id": id, "status": "pending" })))
}

async fn trash_purge_one(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let m = owned_media(&st, &user.username, id).await?;
    if m.status != "trashed" {
        return Err(ApiError::bad("file is not in trash"));
    }
    // Read the membership before `delete_media_row` takes those rows with it.
    let touched = albums_holding(&st, &[id]).await?;
    fileops::purge(FsPath::new(&m.path)).map_err(ApiError::internal)?;
    db::delete_media_row(&st.db, id).await?;
    drop_derived(&st, &m);
    refresh_albums(&st, touched).await?;
    Ok(Json(json!({ "purged": id })))
}

async fn trash_purge_all(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let files = db::list_status(&st.db, &user.username, "trashed").await?;
    let ids: Vec<i64> = files.iter().map(|f| f.id).collect();
    let touched = albums_holding(&st, &ids).await?;
    let mut purged = 0usize;
    for f in files {
        fileops::purge(FsPath::new(&f.path)).map_err(ApiError::internal)?;
        db::delete_media_row(&st.db, f.id).await?;
        drop_derived(&st, &f);
        purged += 1;
    }
    refresh_albums(&st, touched).await?;
    Ok(Json(json!({ "purged": purged })))
}

// ── Retention background task ─────────────────────────────────────────────

/// Spawn a task that purges files trashed longer than `retention_days`, running
/// every hour. Call once at startup.
pub async fn spawn_retention_task(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            run_retention(&state).await;
        }
    });
}

async fn run_retention(st: &AppState) {
    let retention_secs = st.cfg.trash_retention_days as i64 * 86_400;
    let users = match db::users_with_trash(&st.db).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("retention: list users: {e}");
            return;
        }
    };
    for user in users {
        match db::expired_trash(&st.db, &user, retention_secs).await {
            Ok(expired) => {
                // Album membership outlives a deletion so a restore can undo
                // it, which means a purge is where those rows finally go — and
                // the counts they fed have to be recomputed afterwards. Read
                // straight from `db` rather than through the request helpers:
                // nothing here has a response to put an `ApiError` in, and a
                // sweep must not abandon the rest of the user's expired files
                // because one lookup failed.
                let mut touched: HashSet<i64> = HashSet::new();
                for f in &expired {
                    match db::album_ids_for_file(&st.db, f.id).await {
                        Ok(ids) => touched.extend(ids),
                        Err(e) => tracing::warn!("retention: albums for {}: {e}", f.id),
                    }
                }
                for f in expired {
                    if let Err(e) = fileops::purge(FsPath::new(&f.path)) {
                        tracing::warn!("retention purge {}: {e}", f.path);
                    } else if let Err(e) = db::delete_media_row(&st.db, f.id).await {
                        tracing::warn!("retention db delete {}: {e}", f.id);
                    } else {
                        drop_derived(st, &f);
                        tracing::info!("retention: purged {}", f.filename);
                    }
                }
                for aid in touched {
                    if let Err(e) = db::refresh_album_meta(&st.db, aid).await {
                        tracing::warn!("retention: refreshing album {aid}: {e}");
                    }
                }
            }
            Err(e) => tracing::error!("retention: expired_trash for {user}: {e}"),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Body ceiling for one chunk: the advertised chunk size plus a little slack.
///
/// The slack absorbs a client that rounds its chunk size up slightly or adds
/// framing, so an honest uploader is never rejected for being a few hundred
/// bytes over. It is deliberately small — this number is also the worst-case
/// memory one request can make the server allocate.
fn chunk_body_limit(cfg: &easyclean_core::Config) -> usize {
    const SLACK: u64 = 64 * 1024;
    cfg.upload_chunk_size.saturating_add(SLACK) as usize
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn owned_media(st: &AppState, username: &str, id: i64) -> Result<MediaFile, ApiError> {
    let m = db::get_media(&st.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("media not found"))?;
    if m.user != username {
        return Err(ApiError::not_found("media not found"));
    }
    // Hidden files are not reachable with an ordinary login, and every
    // per-file route — thumb, preview, raw, favourite, trash, albums — comes
    // through here. Guarding the one function is what makes that true of all
    // of them rather than of the ones somebody remembered.
    //
    // `not_found`, not `forbidden`: a 403 would confirm there is something at
    // that id to hide.
    if m.status == db::VAULT {
        return Err(ApiError::not_found("media not found"));
    }
    Ok(m)
}

/// Every album the given files are filed under.
///
/// Collected *before* a purge, because the membership rows that name those
/// albums are deleted along with the file's row.
async fn albums_holding(st: &AppState, file_ids: &[i64]) -> Result<HashSet<i64>, ApiError> {
    let mut ids = HashSet::new();
    for id in file_ids {
        ids.extend(db::album_ids_for_file(&st.db, *id).await?);
    }
    Ok(ids)
}

/// Recompute count and cover for every album in `album_ids`.
///
/// An album's count and cover are stored columns, not live queries — the Albums
/// tab reads every album at once, and one subquery per card per request is not
/// a trade worth making. The cost is that nothing notices a file being binned,
/// restored or purged until its albums are recomputed, which is why this runs
/// after every status change rather than only when a photo is filed.
async fn refresh_albums(st: &AppState, album_ids: HashSet<i64>) -> Result<(), ApiError> {
    for aid in album_ids {
        db::refresh_album_meta(&st.db, aid).await?;
    }
    Ok(())
}

/// [`albums_holding`] then [`refresh_albums`], for the common case where the
/// files survive the change and their membership rows are still there to read.
async fn refresh_albums_for(st: &AppState, file_ids: &[i64]) -> Result<(), ApiError> {
    let ids = albums_holding(st, file_ids).await?;
    refresh_albums(st, ids).await
}

/// Where the cached preview for `id` lives.
fn preview_path(st: &AppState, id: i64) -> std::path::PathBuf {
    st.cfg.data_dir.join("previews").join(format!("{id}.jpg"))
}

/// Drop the derived files (thumbnail + preview) for a permanently purged id.
/// Best-effort: a caches miss costs a re-render, a failure here must never fail
/// the purge itself.
fn drop_derived(st: &AppState, m: &MediaFile) {
    if let Some(tp) = &m.thumb_path {
        let _ = std::fs::remove_file(tp);
    }
    let _ = std::fs::remove_file(preview_path(st, m.id));
}

/// Path relative to the user's root (`source/x.jpg`, `clean/Aug 2026/x.jpg`,
/// `.trash/x.jpg`) for display. Falls back to the file name if `path` somehow
/// sits outside the root.
fn rel_to_root(root: &UserRoot, path: &FsPath) -> String {
    path.strip_prefix(root.root())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

fn enrich(files: Vec<MediaFile>) -> Vec<Value> {
    files
        .into_iter()
        .map(|f| {
            let id = f.id;
            let mut v = serde_json::to_value(&f).unwrap_or(Value::Null);
            if let Value::Object(ref mut map) = v {
                map.insert("thumb_url".into(), json!(format!("/api/media/{id}/thumb")));
                map.insert(
                    "preview_url".into(),
                    json!(format!("/api/media/{id}/preview")),
                );
                map.insert("raw_url".into(), json!(format!("/api/media/{id}/raw")));
            }
            v
        })
        .collect()
}

/// Serve an original — streamed, and honouring `Range`.
///
/// `serve_file` reads the whole file into memory and answers every request
/// with all of it, which is fine for a 30 KB thumbnail and wrong for a video.
/// Safari on iOS opens a clip by asking for `bytes=0-1` and expects a `206`
/// back; given a `200` it falls back to downloading the entire file before the
/// first frame, so tapping Play on a 24 MB clip meant watching a spinner for
/// all 24 MB — and it could not seek. Every one of those downloads also sat in
/// the server's memory whole, under a container capped at 2 GB.
///
/// tower-http's `ServeFile` streams from disk and answers ranges and
/// conditional requests properly. Only the headers it acts on are forwarded:
/// the caller has already authenticated the request and resolved the path, so
/// nothing else from the client reaches it.
async fn serve_original(path: &FsPath, headers: &HeaderMap) -> Result<Response, ApiError> {
    use tower::ServiceExt;
    let mut req = axum::http::Request::new(Body::empty());
    for name in [
        header::RANGE,
        header::IF_RANGE,
        header::IF_MODIFIED_SINCE,
        header::IF_UNMODIFIED_SINCE,
    ] {
        if let Some(v) = headers.get(&name) {
            req.headers_mut().insert(name, v.clone());
        }
    }
    // ServeFile cannot fail — a missing file is its own 404 response — so its
    // error type is `Infallible` and this pattern is irrefutable.
    let Ok(resp) = tower_http::services::ServeFile::new(path)
        .oneshot(req)
        .await;
    Ok(resp.map(Body::new))
}

async fn serve_file(path: &FsPath) -> Result<Response, ApiError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| ApiError::not_found("file not found"))?;
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    Ok(resp)
}

// ── Tests ─────────────────────────────────────────────────────────────────

// ── The hidden vault ───────────────────────────────────────────────────────
//
// Reached by tapping the profile chip in Settings three times; nothing else in
// the UI mentions it. The PIN is its own secret, not the login passcode: the
// only person a hidden vault defends against is someone who can already open
// the app.

/// How long one PIN entry stays good for. Short on purpose — this is the window
/// in which hidden photos are reachable, and it should close on its own.
const VAULT_TOKEN_MINUTES: i64 = 15;

/// Digits only, 4 to 8 of them. Rejected server-side as well as in the keypad,
/// because the keypad is not what enforces it.
fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.chars().count()) && pin.chars().all(|c| c.is_ascii_digit())
}

/// Is a PIN set? The one thing worth knowing before the keypad is drawn, so it
/// can say "Set a PIN" instead of "Enter PIN".
async fn vault_status(State(st): State<AppState>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let configured = db::vault_pin_hash(&st.db, &user.username).await?.is_some();
    Ok(Json(json!({ "configured": configured })))
}

#[derive(Deserialize)]
struct VaultPinReq {
    pin: String,
    /// Required once a PIN exists. Changing it has to prove you know the old
    /// one, or anyone holding a logged-in phone could lock you out of your own
    /// vault and read it.
    current_pin: Option<String>,
}

async fn vault_set_pin(
    State(st): State<AppState>,
    user: AuthUser,
    Json(req): Json<VaultPinReq>,
) -> Result<Json<Value>, ApiError> {
    if !valid_pin(&req.pin) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a PIN is 4 to 8 digits".into(),
        ));
    }
    if let Some(existing) = db::vault_pin_hash(&st.db, &user.username).await? {
        // Same throttle as unlock, and for the same reason: `current_pin` is a
        // second PIN oracle, so leaving it open would just move the guessing
        // here.
        let key = vault_attempt_key(&user.username);
        if !st.rate_limiter.check(&key) {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too many PIN attempts — try again later".into(),
            ));
        }
        let ok = req
            .current_pin
            .as_deref()
            .map(|c| easyclean_core::auth::verify_passcode(c, &existing))
            .unwrap_or(false);
        if !ok {
            st.rate_limiter.record(&key);
            return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong PIN".into()));
        }
    }
    let hash = easyclean_core::auth::hash_passcode(&req.pin).map_err(ApiError::internal)?;
    db::set_vault_pin(&st.db, &user.username, &hash, now_secs()).await?;
    Ok(Json(json!({ "configured": true })))
}

#[derive(Deserialize)]
struct VaultUnlockReq {
    pin: String,
}

/// Throttle key for PIN guessing.
///
/// The account, not the IP. Login is throttled per-IP because the attacker
/// there is anonymous; whoever is guessing a PIN has already signed in as this
/// user, so they can move between addresses at will and the account name is the
/// only thing that stays put. Four digits is 10,000 guesses — cheap to walk at
/// full speed, and the vault's whole threat model is someone holding an
/// unlocked phone.
fn vault_attempt_key(username: &str) -> String {
    format!("vault:{username}")
}

/// Trade the PIN for a short-lived token.
async fn vault_unlock(
    State(st): State<AppState>,
    user: AuthUser,
    Json(req): Json<VaultUnlockReq>,
) -> Result<Json<Value>, ApiError> {
    let key = vault_attempt_key(&user.username);
    if !st.rate_limiter.check(&key) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many PIN attempts — try again later".into(),
        ));
    }
    let hash = db::vault_pin_hash(&st.db, &user.username)
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no PIN set".into()))?;
    if !easyclean_core::auth::verify_passcode(&req.pin, &hash) {
        st.rate_limiter.record(&key);
        return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong PIN".into()));
    }
    let token =
        easyclean_core::auth::make_vault_token(&user.username, &st.cfg.secret, VAULT_TOKEN_MINUTES)
            .map_err(ApiError::internal)?;
    Ok(Json(json!({
        "vault_token": token,
        "expires_in": VAULT_TOKEN_MINUTES * 60,
    })))
}

#[derive(Deserialize)]
struct VaultListQuery {
    /// A folder id, or `unfiled`. Absent means the whole vault.
    folder: Option<String>,
}

async fn vault_list(
    State(st): State<AppState>,
    user: VaultUser,
    Query(q): Query<VaultListQuery>,
) -> Result<Json<Value>, ApiError> {
    let scope = match q.folder.as_deref() {
        None | Some("") | Some("all") => db::VaultScope::All,
        Some("unfiled") => db::VaultScope::Unfiled,
        Some(raw) => {
            let id: i64 = raw
                .parse()
                .map_err(|_| ApiError::bad("folder must be an id, `unfiled` or `all`"))?;
            owned_vault_folder(&st, &user.username, id).await?;
            db::VaultScope::Folder(id)
        }
    };
    let files = db::vault_files(&st.db, &user.username, scope).await?;
    let total = files.len();
    Ok(Json(json!({ "files": files, "total": total })))
}

#[derive(Deserialize)]
struct VaultIdsReq {
    ids: Vec<i64>,
}

#[derive(Deserialize)]
struct VaultAddReq {
    ids: Vec<i64>,
    /// Hide straight into this folder — the one on screen when you tapped +.
    /// Absent or null leaves them Unfiled.
    #[serde(default)]
    folder_id: Option<i64>,
}

/// Hide files.
///
/// Four things leave with the photo, and every one of them is a readable copy
/// that would otherwise survive outside the vault:
///   * the file itself, into `.vault/`
///   * its thumbnail, into `.vault/.thumbs/` — not deleted, so the vault's own
///     grid still has something to draw
///   * its cached full-screen preview under `<data_dir>/previews/`, deleted
///   * its CLIP embedding, deleted, so semantic search cannot rank it
///
/// It also leaves every album it was filed into, because album counts are
/// visible without the PIN and a hole in one is a tell.
async fn vault_add(
    State(st): State<AppState>,
    user: VaultUser,
    Json(req): Json<VaultAddReq>,
) -> Result<Json<Value>, ApiError> {
    // Checked before anything moves, so a bad folder id refuses the whole
    // request instead of hiding the photos and then failing to file them.
    if let Some(fid) = req.folder_id {
        owned_vault_folder(&st, &user.username, fid).await?;
    }
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    root.ensure().map_err(ApiError::internal)?;
    let thumbs = root.vault_thumbs();
    std::fs::create_dir_all(&thumbs).map_err(ApiError::internal)?;

    let mut hidden = 0usize;
    let mut touched: std::collections::HashSet<i64> = Default::default();

    for id in req.ids {
        let Some(m) = db::get_media(&st.db, id).await? else {
            continue;
        };
        if m.user != user.username || m.status == db::VAULT {
            continue;
        }

        let src = FsPath::new(&m.path);
        if !src.starts_with(&st.cfg.media_root) {
            continue;
        }
        let dest = match fileops::to_vault(&root, src) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("vault: moving {}: {e}", m.path);
                continue;
            }
        };

        if let Some(tp) = m.thumb_path.as_deref() {
            let _ = fileops::move_into(FsPath::new(tp), &thumbs);
        }
        let _ = std::fs::remove_file(preview_path(&st, id));
        db::delete_embedding(&st.db, id).await?;
        for aid in db::remove_from_all_albums(&st.db, id).await? {
            touched.insert(aid);
        }

        let rel = rel_to_root(&root, &dest);
        if db::move_to_vault(
            &st.db,
            &user.username,
            id,
            &dest.to_string_lossy(),
            &rel,
            req.folder_id,
        )
        .await?
        {
            hidden += 1;
        }
    }

    for aid in touched {
        db::refresh_album_meta(&st.db, aid).await?;
    }
    Ok(Json(json!({ "hidden": hidden })))
}

/// Bring files back out, to the status and path they had before.
///
/// `phash` is cleared so the next detection pass regenerates the thumbnail into
/// the ordinary cache — the old one went into the vault with the file, and
/// guessing where it landed is worse than rebuilding it.
async fn vault_restore(
    State(st): State<AppState>,
    user: VaultUser,
    Json(req): Json<VaultIdsReq>,
) -> Result<Json<Value>, ApiError> {
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    let mut restored = 0usize;

    for id in req.ids {
        let Some(m) = db::vault_file(&st.db, &user.username, id).await? else {
            continue;
        };
        // Where it came from. Without it there is nowhere honest to put the
        // file back, so the restore is refused rather than guessed at.
        let Some(target) = db::vault_restore_target(&st.db, &user.username, id).await? else {
            continue;
        };
        let target = FsPath::new(&target);
        let Some(dir) = target.parent() else { continue };
        if !dir.starts_with(&st.cfg.media_root) {
            continue;
        }

        let dest = match fileops::restore(FsPath::new(&m.path), dir) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("vault restore: {}: {e}", m.path);
                continue;
            }
        };
        let rel = rel_to_root(&root, &dest);
        if db::commit_vault_restore(&st.db, &user.username, id, &dest.to_string_lossy(), &rel)
            .await?
        {
            restored += 1;
        }
    }
    Ok(Json(json!({ "restored": restored })))
}

/// Delete hidden files for good.
///
/// Not trash-first, and that is deliberate. The trash screen needs no PIN, so
/// binning a hidden photo would stand it on a shelf anyone holding the phone
/// can browse — the one thing the vault exists to prevent. Trash-first protects
/// against a mis-tap; here the protection is an explicit confirmation that says
/// plainly that this cannot be undone, which is the "explicit purge" the same
/// rule allows for.
async fn vault_delete(
    State(st): State<AppState>,
    user: VaultUser,
    Json(req): Json<VaultIdsReq>,
) -> Result<Json<Value>, ApiError> {
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    let mut purged = 0usize;

    for id in req.ids {
        let Some(m) = db::vault_file(&st.db, &user.username, id).await? else {
            continue;
        };
        let p = FsPath::new(&m.path);
        if !p.starts_with(&st.cfg.media_root) {
            continue;
        }
        if p.exists() {
            fileops::purge(p).map_err(ApiError::internal)?;
        }
        // The thumbnail moved in here with the file, so it has to go too —
        // otherwise deleting a hidden photo leaves a readable copy of it.
        let _ = std::fs::remove_file(root.vault_thumbs().join(format!("{id}.jpg")));
        db::delete_media_row(&st.db, id).await?;
        drop_derived(&st, &m);
        purged += 1;
    }
    Ok(Json(json!({ "purged": purged })))
}

// ── Vault folders ─────────────────────────────────────────────────────────
//
// Grouping only: the files stay flat in `.vault/` and a folder is a row in
// `vault_folder` plus a column on the photo. Nothing about a folder touches the
// disk, so no name the user types ever becomes part of a path.

/// Same bounds as an album name, for the same reasons: it has to fit on a card,
/// and an empty one is a folder you cannot tell apart from the next.
fn vault_folder_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ApiError::bad("a folder needs a name"));
    }
    if name.chars().count() > 80 {
        return Err(ApiError::bad("that name is too long"));
    }
    Ok(name.to_string())
}

/// 404 unless `id` is one of this user's folders — the same answer for "not
/// yours" and "not there", so another account's folder ids can't be probed.
async fn owned_vault_folder(st: &AppState, username: &str, id: i64) -> Result<(), ApiError> {
    if db::vault_folder_owned(&st.db, username, id).await? {
        Ok(())
    } else {
        Err(ApiError::not_found("no such folder"))
    }
}

/// Every folder with its count and cover, plus the two totals the vault's
/// folder strip needs: what is in no folder, and everything.
async fn vault_folders_list(
    State(st): State<AppState>,
    user: VaultUser,
) -> Result<Json<Value>, ApiError> {
    let folders = db::vault_folders(&st.db, &user.username).await?;
    let unfiled = db::vault_unfiled_count(&st.db, &user.username).await?;
    let total = unfiled + folders.iter().map(|f| f.file_count).sum::<i64>();
    Ok(Json(json!({
        "folders": folders,
        "unfiled": unfiled,
        "total": total,
    })))
}

#[derive(Deserialize)]
struct VaultFolderReq {
    name: String,
}

async fn vault_folder_create(
    State(st): State<AppState>,
    user: VaultUser,
    Json(b): Json<VaultFolderReq>,
) -> Result<Json<Value>, ApiError> {
    let name = vault_folder_name(&b.name)?;
    let id = db::create_vault_folder(&st.db, &user.username, &name, now_secs())
        .await?
        .ok_or_else(|| {
            ApiError(
                StatusCode::CONFLICT,
                "you already have a folder with that name".into(),
            )
        })?;
    Ok(Json(json!({ "id": id, "name": name })))
}

async fn vault_folder_rename(
    State(st): State<AppState>,
    user: VaultUser,
    Path(id): Path<i64>,
    Json(b): Json<VaultFolderReq>,
) -> Result<Json<Value>, ApiError> {
    let name = vault_folder_name(&b.name)?;
    match db::rename_vault_folder(&st.db, &user.username, id, &name).await? {
        db::RenameOutcome::Renamed => Ok(Json(json!({ "id": id, "name": name }))),
        db::RenameOutcome::Missing => Err(ApiError::not_found("no such folder")),
        db::RenameOutcome::Taken => Err(ApiError(
            StatusCode::CONFLICT,
            "you already have a folder with that name".into(),
        )),
    }
}

/// Delete a folder. Its photos stay hidden, in Unfiled.
async fn vault_folder_delete(
    State(st): State<AppState>,
    user: VaultUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let unfiled = db::delete_vault_folder(&st.db, &user.username, id)
        .await?
        .ok_or_else(|| ApiError::not_found("no such folder"))?;
    Ok(Json(json!({ "deleted": id, "unfiled": unfiled })))
}

#[derive(Deserialize)]
struct VaultMoveReq {
    ids: Vec<i64>,
    /// Destination folder; null moves them to Unfiled.
    folder_id: Option<i64>,
}

async fn vault_move(
    State(st): State<AppState>,
    user: VaultUser,
    Json(req): Json<VaultMoveReq>,
) -> Result<Json<Value>, ApiError> {
    if let Some(fid) = req.folder_id {
        owned_vault_folder(&st, &user.username, fid).await?;
    }
    let moved = db::move_vault_files(&st.db, &user.username, &req.ids, req.folder_id).await?;
    Ok(Json(json!({ "moved": moved })))
}

/// A hidden file's thumbnail, from inside the vault.
async fn vault_thumb(
    State(st): State<AppState>,
    user: VaultUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let m = db::vault_file(&st.db, &user.username, id)
        .await?
        .ok_or_else(|| ApiError::not_found("not in the vault"))?;
    let root = UserRoot::new(&st.cfg.media_root, &user.username).map_err(ApiError::internal)?;
    // Named by id, not by the photo's filename: thumbnails are `<id>.jpg` in
    // the shared cache and keep that name when they move in here. Looking for
    // `<filename>.jpg` missed every time and fell through to the original —
    // which happened to render for images and left videos as a broken tile.
    let tp = root.vault_thumbs().join(format!("{id}.jpg"));
    if tp.exists() {
        return serve_file(&tp).await;
    }
    // No cached thumbnail. An image can stand in for its own thumbnail; a video
    // cannot, and saying so beats serving bytes no <img> can decode.
    if m.media_type == "image" {
        return serve_file(FsPath::new(&m.path)).await;
    }
    Err(ApiError::not_found("no thumbnail yet"))
}

/// A hidden file itself, from inside the vault.
async fn vault_raw(
    State(st): State<AppState>,
    user: VaultUser,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let m = db::vault_file(&st.db, &user.username, id)
        .await?
        .ok_or_else(|| ApiError::not_found("not in the vault"))?;
    let p = FsPath::new(&m.path);
    if !p.starts_with(&st.cfg.media_root) || !p.exists() {
        return Err(ApiError::not_found("file missing on disk"));
    }
    serve_original(p, &headers).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use easyclean_core::models::NewMedia;
    use std::sync::Arc;
    use tower::ServiceExt;

    static TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    async fn test_state() -> AppState {
        let id = TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("easyclean_api_test_{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = easyclean_core::Config {
            data_dir: dir.clone(),
            media_root: dir.join("media"),
            default_user: "saiz".into(),
            secret: "test-secret".into(),
            ..easyclean_core::Config::default()
        };
        let db = db::init(&cfg.db_path()).await.unwrap();
        let hash = core_auth::hash_passcode("pw").unwrap();
        db::upsert_user(&db, "saiz", &hash, "admin").await.unwrap();
        db::insert_media(
            &db,
            &NewMedia {
                user: "saiz".into(),
                path: dir.join("media/saiz/x.jpg").to_string_lossy().into(),
                rel_path: None,
                filename: "x.jpg".into(),
                media_type: "image".into(),
                size_bytes: 10,
                created_at: 1_700_000_000,
                gps_lat: None,
                gps_lon: None,
                content_hash: None,
            },
        )
        .await
        .unwrap();
        AppState {
            db,
            cfg: Arc::new(cfg),
            scan: Default::default(),
            rate_limiter: Default::default(),
            index: Default::default(),
            uploads: Default::default(),
            upload_settle: Default::default(),
            clip: Default::default(),
        }
    }

    fn bearer(st: &AppState) -> String {
        format!(
            "Bearer {}",
            core_auth::make_token("saiz", "admin", &st.cfg.secret, 7).unwrap()
        )
    }

    // ── Upload helpers ────────────────────────────────────────────────────

    /// What an honest client would compute for these bytes.
    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn json_req(uri: &str, auth: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    /// init → one chunk → finish, returning the `finish` response.
    async fn upload_one(st: &AppState, auth: &str, filename: &str, data: &[u8]) -> Response {
        let hash = sha256_hex(data);
        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                auth,
                json!({ "filename": filename, "size": data.len(), "hash": hash }),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "init should succeed");
        let id = body_json(r).await["id"].as_str().unwrap().to_string();

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/upload/{id}/chunk?offset=0"))
                    .header("authorization", auth)
                    .body(Body::from(data.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "chunk should be accepted");

        app(st.clone())
            .oneshot(json_req(
                &format!("/api/upload/{id}/finish"),
                auth,
                json!({}),
            ))
            .await
            .unwrap()
    }

    /// The happy path, end to end: the bytes land in `source/` under the name
    /// the client asked for, and the row carries the hash the *server* computed.
    #[tokio::test]
    async fn an_upload_lands_in_the_library() {
        let st = test_state().await;
        let auth = bearer(&st);
        let data = b"not really a jpeg, but the indexer only reads the extension";

        let r = upload_one(&st, &auth, "IMG_0001.jpg", data).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = body_json(r).await;
        let file_id = body["file_id"].as_i64().unwrap();
        assert_eq!(body["filename"], "IMG_0001.jpg");

        let on_disk = st.cfg.media_root.join("saiz/source/IMG_0001.jpg");
        assert!(on_disk.exists(), "file should be in source/: {on_disk:?}");
        assert_eq!(std::fs::read(&on_disk).unwrap(), data);

        let row = db::get_media(&st.db, file_id).await.unwrap().unwrap();
        assert_eq!(row.content_hash.as_deref(), Some(sha256_hex(data).as_str()));
        assert_eq!(row.status, "pending", "an upload is waiting to be reviewed");
    }

    /// Nothing that fails the hash check may be filed.
    ///
    /// The case that matters is not a liar but a truncated transfer: bytes that
    /// arrive wrong and are indexed anyway become a permanently broken photo in
    /// someone's library, and the only evidence is gone.
    #[tokio::test]
    async fn a_hash_that_does_not_match_files_nothing() {
        let st = test_state().await;
        let auth = bearer(&st);
        let data = b"the bytes actually sent";
        let before = db::count_media(&st.db, "saiz").await.unwrap();

        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                &auth,
                // A digest for entirely different content.
                json!({
                    "filename": "liar.jpg",
                    "size": data.len(),
                    "hash": sha256_hex(b"something else"),
                }),
            ))
            .await
            .unwrap();
        let id = body_json(r).await["id"].as_str().unwrap().to_string();

        app(st.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/upload/{id}/chunk?offset=0"))
                    .header("authorization", &auth)
                    .body(Body::from(data.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let r = app(st.clone())
            .oneshot(json_req(
                &format!("/api/upload/{id}/finish"),
                &auth,
                json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);

        assert!(
            !st.cfg.media_root.join("saiz/source/liar.jpg").exists(),
            "a file that failed verification must not reach source/"
        );
        assert_eq!(
            db::count_media(&st.db, "saiz").await.unwrap(),
            before,
            "and must not be indexed"
        );
    }

    /// A chunk aimed at the wrong place is refused, and the reply says where
    /// the file really ends so the client can carry on rather than start over.
    #[tokio::test]
    async fn a_misaimed_chunk_is_refused_with_the_real_offset() {
        let st = test_state().await;
        let auth = bearer(&st);
        let data = b"0123456789";

        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                &auth,
                json!({ "filename": "a.jpg", "size": 10, "hash": sha256_hex(data) }),
            ))
            .await
            .unwrap();
        let id = body_json(r).await["id"].as_str().unwrap().to_string();

        // First four bytes land.
        app(st.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/upload/{id}/chunk?offset=0"))
                    .header("authorization", &auth)
                    .body(Body::from(&data[..4]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Then a chunk that claims to start at 9 — a gap.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/upload/{id}/chunk?offset=9"))
                    .header("authorization", &auth)
                    .body(Body::from(&data[9..]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert!(
            body_json(r).await["error"].as_str().unwrap().contains("4"),
            "the client needs to be told where to resume"
        );

        // And asking directly gives the same answer.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/upload/{id}"))
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(r).await["offset"], 4);
    }

    /// The call that makes re-offering a camera roll cheap.
    #[tokio::test]
    async fn check_reports_what_is_already_held() {
        let st = test_state().await;
        let auth = bearer(&st);
        let held = b"already uploaded once";
        upload_one(&st, &auth, "held.jpg", held).await;

        let known = sha256_hex(held);
        let unknown = sha256_hex(b"never seen");
        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/check",
                &auth,
                json!({ "hashes": [known, unknown] }),
            ))
            .await
            .unwrap();
        let body = body_json(r).await;
        assert_eq!(body["have"], json!([known]));
        assert_eq!(body["need"], json!([unknown]));
    }

    /// An upload belongs to the account that opened it. Anyone else gets the
    /// same answer as for an id that never existed.
    #[tokio::test]
    async fn another_account_cannot_touch_your_upload() {
        let st = test_state().await;
        let auth = bearer(&st);
        let lily = format!(
            "Bearer {}",
            core_auth::make_token("lily", "user", &st.cfg.secret, 7).unwrap()
        );

        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                &auth,
                json!({ "filename": "mine.jpg", "size": 4, "hash": sha256_hex(b"mine") }),
            ))
            .await
            .unwrap();
        let id = body_json(r).await["id"].as_str().unwrap().to_string();

        for req in [
            Request::builder()
                .method("PUT")
                .uri(format!("/api/upload/{id}/chunk?offset=0"))
                .header("authorization", &lily)
                .body(Body::from("mine"))
                .unwrap(),
            Request::builder()
                .uri(format!("/api/upload/{id}"))
                .header("authorization", &lily)
                .body(Body::empty())
                .unwrap(),
            json_req(&format!("/api/upload/{id}/finish"), &lily, json!({})),
        ] {
            let r = app(st.clone()).oneshot(req).await.unwrap();
            assert_eq!(
                r.status(),
                StatusCode::NOT_FOUND,
                "another user's session must look like no session at all"
            );
        }
    }

    /// Anything that is not a photo or video is refused before a single byte is
    /// sent, not after the upload completes.
    #[tokio::test]
    async fn non_media_is_refused_at_init() {
        let st = test_state().await;
        let auth = bearer(&st);
        for name in ["payload.sh", "index.html", "photo.jpg.exe", "noextension"] {
            let r = app(st.clone())
                .oneshot(json_req(
                    "/api/upload/init",
                    &auth,
                    json!({ "filename": name, "size": 10, "hash": sha256_hex(b"x") }),
                ))
                .await
                .unwrap();
            assert_eq!(
                r.status(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "{name} should not be accepted"
            );
        }
    }

    /// A filename carrying a path must not decide where the file lands.
    #[tokio::test]
    async fn a_traversing_filename_still_lands_in_source() {
        let st = test_state().await;
        let auth = bearer(&st);
        let data = b"payload";

        let r = upload_one(&st, &auth, "../../../../etc/evil.jpg", data).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["filename"], "evil.jpg");
        assert!(st.cfg.media_root.join("saiz/source/evil.jpg").exists());
    }

    /// Two files with the same name both survive, rather than one overwriting
    /// the other — `IMG_0001.jpg` is what every phone on earth calls a photo.
    #[tokio::test]
    async fn a_repeated_filename_does_not_overwrite() {
        let st = test_state().await;
        let auth = bearer(&st);
        upload_one(&st, &auth, "IMG_0001.jpg", b"first photo").await;
        let r = upload_one(&st, &auth, "IMG_0001.jpg", b"second photo").await;

        assert_eq!(body_json(r).await["filename"], "IMG_0001_1.jpg");
        let src = st.cfg.media_root.join("saiz/source");
        assert_eq!(
            std::fs::read(src.join("IMG_0001.jpg")).unwrap(),
            b"first photo"
        );
        assert_eq!(
            std::fs::read(src.join("IMG_0001_1.jpg")).unwrap(),
            b"second photo"
        );
    }

    /// A quota refuses the upload up front, with the numbers the UI needs.
    #[tokio::test]
    async fn a_full_quota_refuses_before_any_bytes_move() {
        let mut st = test_state().await;
        let cfg = easyclean_core::Config {
            quotas: [("saiz".to_string(), 16u64)].into_iter().collect(),
            ..(*st.cfg).clone()
        };
        st.cfg = Arc::new(cfg);
        let auth = bearer(&st);

        // Comfortably inside the ceiling.
        let r = upload_one(&st, &auth, "small.jpg", b"tiny").await;
        assert_eq!(r.status(), StatusCode::OK);

        // This one would cross it.
        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                &auth,
                json!({ "filename": "big.jpg", "size": 1_000, "hash": sha256_hex(b"big") }),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body_json(r).await["error"]
            .as_str()
            .unwrap()
            .contains("quota exceeded"));
    }

    /// The database beats the environment, so an admin's change is not undone
    /// by the next restart. This is the whole reason quotas are read live
    /// instead of seeded.
    #[tokio::test]
    async fn an_admin_set_quota_overrides_the_environment() {
        let mut st = test_state().await;
        let cfg = easyclean_core::Config {
            quotas: [("saiz".to_string(), 4u64)].into_iter().collect(),
            ..(*st.cfg).clone()
        };
        st.cfg = Arc::new(cfg);
        db::set_setting(&st.db, "saiz", upload::QUOTA_SETTING_KEY, "100000")
            .await
            .unwrap();
        let auth = bearer(&st);

        let r = upload_one(&st, &auth, "ok.jpg", b"larger than four bytes").await;
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "the stored quota, not EASYCLEAN_QUOTAS, decides"
        );

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/upload/quota")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(r).await["limit"], 100_000);
    }

    /// A user cannot raise their own ceiling through the settings endpoint.
    #[tokio::test]
    async fn settings_cannot_be_used_to_grant_yourself_quota() {
        let mut st = test_state().await;
        let cfg = easyclean_core::Config {
            quotas: [("saiz".to_string(), 8u64)].into_iter().collect(),
            ..(*st.cfg).clone()
        };
        st.cfg = Arc::new(cfg);
        let auth = bearer(&st);

        app(st.clone())
            .oneshot(json_req(
                "/api/settings",
                &auth,
                json!({ "quota_bytes": 999_999_999 }),
            ))
            .await
            .unwrap();

        let r = app(st.clone())
            .oneshot(json_req(
                "/api/upload/init",
                &auth,
                json!({ "filename": "big.jpg", "size": 1_000, "hash": sha256_hex(b"big") }),
            ))
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "settings_put must not write the quota key"
        );
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn app(st: AppState) -> axum::Router {
        // Use into_make_service_with_connect_info for ConnectInfo extractor in tests.
        router(st)
    }

    #[tokio::test]
    async fn health_ok() {
        let r = app(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    /// Default policy is same-origin: a page on another origin must not be
    /// handed `Access-Control-Allow-Origin`, or any site on the LAN could read
    /// the API with a token it scraped from localStorage.
    #[tokio::test]
    async fn cors_is_closed_by_default() {
        let st = test_state().await;
        let auth = bearer(&st);
        let r = app(st)
            .oneshot(
                Request::builder()
                    .uri("/api/stats/summary")
                    .header("origin", "http://evil.local")
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(
            r.headers().get("access-control-allow-origin").is_none(),
            "no cross-origin read access without EASYCLEAN_CORS_ORIGINS"
        );
    }

    /// ...and configuring an origin opens it for exactly that origin.
    #[tokio::test]
    async fn cors_allows_configured_origin() {
        let mut st = test_state().await;
        let cfg = easyclean_core::Config {
            cors_origins: vec!["http://dev.local:5173".into()],
            ..(*st.cfg).clone()
        };
        st.cfg = Arc::new(cfg);
        let auth = bearer(&st);
        let r = app(st)
            .oneshot(
                Request::builder()
                    .uri("/api/stats/summary")
                    .header("origin", "http://dev.local:5173")
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            r.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("http://dev.local:5173")
        );
    }

    #[tokio::test]
    async fn protected_requires_token() {
        let r = app(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/stats/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    /// A login token must stop working the moment it is put in a URL.
    ///
    /// `<img src>` cannot send a header, so image routes accept a credential
    /// from the query string — and that credential used to be the login itself,
    /// good for a week and good for everything. Any access log, proxy, or tab
    /// opened on such a URL then held the whole account. Now only the narrow,
    /// hour-long media token is accepted there, and it can do nothing but read.
    #[tokio::test]
    async fn a_login_token_is_refused_from_the_query_string() {
        let st = test_state().await;
        let login = core_auth::make_token("saiz", "admin", &st.cfg.secret, 7).unwrap();
        let media = core_auth::make_media_token("saiz", &st.cfg.secret, 60).unwrap();

        let get = |st: AppState, uri: String| async move {
            app(st)
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
        };

        for path in [
            "/api/media/1/thumb",
            "/api/media/1/preview",
            "/api/media/1/raw",
        ] {
            assert_eq!(
                get(st.clone(), format!("{path}?token={login}")).await,
                StatusCode::UNAUTHORIZED,
                "{path} accepted a login token from the query string"
            );
        }

        // The media token is what those URLs are supposed to carry. `thumb`
        // 404s here because this fixture has no thumbnail on disk — the point
        // is that it got *past* the extractor rather than being rejected.
        let s = get(st.clone(), format!("/api/media/1/thumb?token={media}")).await;
        assert_ne!(s, StatusCode::UNAUTHORIZED, "a media token was refused");

        // And it buys nothing else: not the API, not a mutation, not the vault.
        for (method, uri) in [
            ("GET", "/api/auth/me"),
            ("GET", "/api/stats/summary"),
            ("GET", "/api/trash"),
            ("GET", "/api/vault/files"),
        ] {
            let r = app(st.clone())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("authorization", format!("Bearer {media}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                r.status(),
                StatusCode::UNAUTHORIZED,
                "{uri} accepted a media token as a login"
            );
        }

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/swipe")
                    .header("authorization", format!("Bearer {media}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"action":"delete"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::UNAUTHORIZED,
            "a media token could delete a photo"
        );
    }

    /// Minting one needs the header, so the login never travels in a URL even
    /// to ask for its replacement.
    #[tokio::test]
    async fn media_tokens_are_minted_from_a_header_login() {
        let st = test_state().await;
        let auth = bearer(&st);

        let r = post_json(st.clone(), &auth, None, "/api/auth/media-token", "").await;
        assert_eq!(r.status(), StatusCode::OK);
        let j = body_json(r).await;
        let minted = j["media_token"].as_str().unwrap().to_string();
        assert!(j["expires_in"].as_i64().unwrap() > 0);

        // It works where it is meant to...
        let s = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/1/raw?token={minted}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status();
        assert_ne!(s, StatusCode::UNAUTHORIZED);

        // ...and asking for one is itself protected.
        let r = app(st)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/media-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn stats_swipe_trash_flow() {
        let st = test_state().await;
        let auth = bearer(&st);
        let a = app(st);

        // stats: 1 pending
        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/stats/summary")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["pending"], 1);

        // swipe delete
        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/media/swipe")
                        .header("authorization", &auth)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"file_id":1,"action":"delete"}"#))
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["status"], "trashed");

        // trash list has 1
        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/trash")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["files"].as_array().unwrap().len(), 1);

        // streak recorded
        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/stats/streak")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["total_swipes"], 1);
        assert_eq!(j["current_streak"], 1);
    }

    /// Undo must reach the server: the file goes back to `pending` and the
    /// swipe leaves the streak log.
    #[tokio::test]
    async fn undo_reverts_swipe_and_streak() {
        let st = test_state().await;
        let auth = bearer(&st);
        let a = app(st.clone());
        let post = |body: &'static str| {
            let (auth, a) = (auth.clone(), a.clone());
            async move {
                a.oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/media/swipe")
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        post(r#"{"file_id":1,"action":"delete"}"#).await;
        let s = db::stats_summary(&st.db, "saiz").await.unwrap();
        assert_eq!((s.trashed, s.pending), (1, 0));
        assert_eq!(
            db::streak_stats(&st.db, "saiz", now_secs())
                .await
                .unwrap()
                .total_swipes,
            1
        );

        let j = body_json(post(r#"{"file_id":1,"action":"undo"}"#).await).await;
        assert_eq!(j["status"], "pending");
        let s = db::stats_summary(&st.db, "saiz").await.unwrap();
        assert_eq!(
            (s.trashed, s.pending),
            (0, 1),
            "undo should return it to the deck"
        );
        assert_eq!(
            db::streak_stats(&st.db, "saiz", now_secs())
                .await
                .unwrap()
                .total_swipes,
            0,
            "undo should not leave a swipe on the streak"
        );
    }

    /// Trash-first: a delete swipe leaves the file alone until the session is
    /// finished, which moves it into `.trash/`; restore brings it back.
    #[tokio::test]
    async fn delete_finishes_into_trash_and_restores() {
        let st = test_state().await;
        let auth = bearer(&st);
        let root = UserRoot::new(&st.cfg.media_root, "saiz").unwrap();
        root.ensure().unwrap();

        // Put a real file in source/ and point the media row at it.
        let src = root.source().join("x.jpg");
        std::fs::write(&src, b"photo").unwrap();
        db::update_path(&st.db, 1, &src.to_string_lossy(), "source/x.jpg")
            .await
            .unwrap();

        let a = app(st.clone());
        let post = |uri: &str, body: &'static str| {
            let auth = auth.clone();
            let uri = uri.to_string();
            let a = a.clone();
            async move {
                a.oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        post("/api/media/swipe", r#"{"file_id":1,"action":"delete"}"#).await;
        assert!(src.exists(), "swipe alone must not move the file");

        let j = body_json(post("/api/media/session/finish", "").await).await;
        assert_eq!(j["trashed"], 1);
        assert!(!src.exists(), "finish should move it out of source/");
        let trashed = root.trash().join("x.jpg");
        assert!(trashed.exists(), "finish should move it into .trash/");

        let j = body_json(post("/api/trash/restore/1", "").await).await;
        assert_eq!(j["status"], "pending");
        assert!(!trashed.exists(), "restore should empty .trash/");
        assert!(src.exists(), "restore should return it to source/");
        assert_eq!(std::fs::read(&src).unwrap(), b"photo");
    }

    /// Finishing twice must be a no-op the second time — re-moving a file
    /// already inside clean/ used to rename it `x_1.jpg`, then `x_2.jpg`.
    #[tokio::test]
    async fn finishing_twice_leaves_filed_photos_alone() {
        let st = test_state().await;
        let auth = bearer(&st);
        let root = UserRoot::new(&st.cfg.media_root, "saiz").unwrap();
        root.ensure().unwrap();

        let src = root.source().join("x.jpg");
        std::fs::write(&src, b"photo").unwrap();
        db::update_path(&st.db, 1, &src.to_string_lossy(), "source/x.jpg")
            .await
            .unwrap();

        let a = app(st.clone());
        let post = |uri: &'static str, body: &'static str| {
            let (auth, a) = (auth.clone(), a.clone());
            async move {
                a.oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        post("/api/media/swipe", r#"{"file_id":1,"action":"keep"}"#).await;
        let j = body_json(post("/api/media/session/finish", "").await).await;
        assert_eq!(j["moved"], 1);

        let filed: Vec<_> = walk(&root.clean());
        assert_eq!(filed.len(), 1);
        let first = filed[0].clone();

        let j = body_json(post("/api/media/session/finish", "").await).await;
        assert_eq!(j["moved"], 0, "second finish should move nothing");
        let filed = walk(&root.clean());
        assert_eq!(filed, vec![first], "the filed photo should keep its name");
    }

    /// Every file under `dir`, recursively, sorted.
    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else {
                    out.push(p)
                }
            }
        }
        out.sort();
        out
    }

    /// Without an index, search still works — it just answers with names only
    /// and says so, rather than erroring or pretending to understand pictures.
    #[tokio::test]
    async fn search_without_an_index_falls_back_to_names() {
        let st = test_state().await;
        let auth = bearer(&st);

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=x.jpg")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        assert_eq!(
            v["name_matches"].as_array().unwrap().len(),
            1,
            "the seeded x.jpg"
        );
        assert_eq!(v["semantic_matches"].as_array().unwrap().len(), 0);
        assert_eq!(
            v["semantic_available"],
            json!(false),
            "no model, no vectors"
        );
        assert_eq!(v["semantic_confident"], json!(false));

        // An empty query is a bad request, not an expensive scan of everything.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=%20%20")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    /// The index status drives the Settings panel, so its shape matters even
    /// before a single photo has been embedded.
    #[tokio::test]
    async fn search_status_reports_an_empty_index() {
        let st = test_state().await;
        let auth = bearer(&st);
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/search/status")
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        assert_eq!(v["model_ready"], json!(false));
        assert_eq!(v["indexed"], json!(0));
        assert_eq!(v["running"], json!(false));
        // The seeded file has no thumbnail, so it isn't yet indexable.
        assert!(v["indexable"].as_i64().is_some());
    }

    /// Embeddings are derived data keyed by file id. Purging a photo has to
    /// take its vector with it, or a recycled id inherits a stranger's meaning.
    #[tokio::test]
    async fn purging_a_file_drops_its_embedding() {
        let st = test_state().await;
        let vec = vec![7u8; easyclean_core::embed::DIM];
        db::upsert_embedding(
            &st.db,
            1,
            easyclean_core::embed::MODEL_ID,
            512,
            0.01,
            &vec,
            0,
        )
        .await
        .unwrap();
        assert_eq!(
            db::all_embeddings(&st.db, "saiz", easyclean_core::embed::MODEL_ID)
                .await
                .unwrap()
                .len(),
            1
        );

        db::delete_media_row(&st.db, 1).await.unwrap();
        assert!(
            db::all_embeddings(&st.db, "saiz", easyclean_core::embed::MODEL_ID)
                .await
                .unwrap()
                .is_empty(),
            "the vector must not outlive the photo"
        );
    }

    /// Search ranks by similarity, so the fetch that turns ids back into rows
    /// must preserve that order rather than SQLite's.
    #[tokio::test]
    async fn media_by_ids_keeps_the_callers_ranking() {
        let st = test_state().await;
        let mut ids = vec![1i64];
        for i in 0..4 {
            ids.push(
                db::insert_media(
                    &st.db,
                    &NewMedia {
                        user: "saiz".into(),
                        path: format!("/media/saiz/r{i}.jpg"),
                        rel_path: None,
                        filename: format!("r{i}.jpg"),
                        media_type: "image".into(),
                        size_bytes: 1,
                        created_at: 1_700_000_000 + i,
                        gps_lat: None,
                        gps_lon: None,
                        content_hash: None,
                    },
                )
                .await
                .unwrap(),
            );
        }
        ids.reverse();
        let rows = db::media_by_ids(&st.db, "saiz", &ids).await.unwrap();
        assert_eq!(
            rows.iter().map(|m| m.id).collect::<Vec<_>>(),
            ids,
            "ranking order must survive the id lookup"
        );

        // Another user's ids simply aren't returned.
        let rows = db::media_by_ids(&st.db, "someone-else", &ids)
            .await
            .unwrap();
        assert!(rows.is_empty());
    }

    /// Search runs in the database over the whole library, not in the browser
    /// over whatever pages it happens to have loaded.
    #[tokio::test]
    async fn search_matches_across_the_whole_library() {
        let st = test_state().await;
        let auth = bearer(&st);

        for (name, rel, kind) in [
            ("IMG_2201.jpg", "clean/Trip 2026/IMG_2201.jpg", "image"),
            ("IMG_2202.jpg", "source/IMG_2202.jpg", "image"),
            ("sunset.mp4", "clean/Trip 2026/sunset.mp4", "video"),
            ("100%_done.jpg", "source/100%_done.jpg", "image"),
            ("IMG-2203.jpg", "source/IMG-2203.jpg", "image"),
        ] {
            let id = db::insert_media(
                &st.db,
                &NewMedia {
                    user: "saiz".into(),
                    path: format!("/media/saiz/{name}"),
                    rel_path: Some(rel.into()),
                    filename: name.into(),
                    media_type: kind.into(),
                    size_bytes: 10,
                    created_at: 1_700_000_000,
                    gps_lat: None,
                    gps_lon: None,
                    content_hash: None,
                },
            )
            .await
            .unwrap();
            assert!(id > 0);
        }

        let search = |st: AppState, auth: String, qs: String| async move {
            let r = app(st)
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/media/batch?mode=library&{qs}"))
                        .header("authorization", auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::OK);
            let v = body_json(r).await;
            let names: Vec<String> = v["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["filename"].as_str().unwrap().to_string())
                .collect();
            (v["total"].as_i64().unwrap(), names)
        };

        // Filename substring.
        let (total, names) = search(st.clone(), auth.clone(), "size=50&q=IMG_22".into()).await;
        assert_eq!(total, 2, "both IMG_22xx, and not IMG-2203");
        assert!(names.iter().all(|n| n.starts_with("IMG_22")));

        // The path counts too, so an album name finds what's filed under it.
        let (total, _) = search(st.clone(), auth.clone(), "size=50&q=Trip%202026".into()).await;
        assert_eq!(total, 2, "the photo and the video in that album");

        // `_` is a LIKE wildcard; unescaped, this would also match IMG-2203.
        let (total, names) = search(st.clone(), auth.clone(), "size=50&q=IMG_2203".into()).await;
        assert_eq!(
            total, 0,
            "underscore is literal, not 'any character': {names:?}"
        );

        // `%` likewise must not mean "everything".
        let (total, names) = search(st.clone(), auth.clone(), "size=50&q=100%25_done".into()).await;
        assert_eq!(total, 1, "percent is literal: {names:?}");
        assert_eq!(names, vec!["100%_done.jpg"]);

        // Type narrows independently of the text.
        let (total, names) = search(st.clone(), auth.clone(), "size=50&type=video".into()).await;
        assert_eq!(total, 1, "{names:?}");
        assert_eq!(names, vec!["sunset.mp4"]);

        // ...and the two compose.
        let (total, _) = search(st.clone(), auth.clone(), "size=50&q=Trip&type=image".into()).await;
        assert_eq!(total, 1, "the album's photo, not its video");

        // A search finds matches beyond the first page, which is the whole
        // point — page 2 of a 1-per-page search still reports the full total.
        let (total, names) =
            search(st.clone(), auth.clone(), "q=IMG_22&size=1&page=2".into()).await;
        assert_eq!(total, 2, "total is every match, not the page");
        assert_eq!(names.len(), 1, "and page 2 holds the second one");

        // A junk type is refused rather than silently matching nothing.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/media/batch?mode=library&type=gif")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    /// The Gallery's three tabs must actually separate cleaned from not.
    /// Everything a POST with a JSON body needs, since the vault flow is all
    /// posts.
    async fn post_json(
        st: AppState,
        auth: &str,
        vault: Option<&str>,
        uri: &str,
        body: &'static str,
    ) -> Response {
        let mut req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("authorization", auth)
            .header("content-type", "application/json");
        if let Some(v) = vault {
            req = req.header("x-vault-token", v);
        }
        app(st)
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    /// Any vault call, with a body built at runtime — folder ids are only known
    /// once the folder exists, so `post_json`'s `&'static str` can't carry them.
    async fn vault_call(
        st: AppState,
        auth: &str,
        vault: Option<&str>,
        method: &str,
        uri: &str,
        body: Option<String>,
    ) -> Response {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", auth)
            .header("content-type", "application/json");
        if let Some(v) = vault {
            req = req.header("x-vault-token", v);
        }
        app(st)
            .oneshot(
                req.body(body.map(Body::from).unwrap_or_else(Body::empty))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Put `n` photos on disk and in the library, set a PIN, unlock, and hide
    /// them all. Returns the vault token and the hidden ids.
    async fn vault_with_hidden(st: &AppState, auth: &str, n: usize) -> (String, Vec<i64>) {
        let dir = st.cfg.media_root.join("saiz").join("source");
        std::fs::create_dir_all(&dir).unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let p = dir.join(format!("h{i}.jpg"));
            std::fs::write(&p, b"jpeg-bytes").unwrap();
            let id = db::insert_media(
                &st.db,
                &NewMedia {
                    user: "saiz".into(),
                    path: p.to_string_lossy().into(),
                    rel_path: None,
                    filename: format!("h{i}.jpg"),
                    media_type: "image".into(),
                    size_bytes: 10,
                    created_at: 1_700_000_000 + i as i64,
                    gps_lat: None,
                    gps_lon: None,
                },
            )
            .await
            .unwrap();
            ids.push(id);
        }
        let r = post_json(
            st.clone(),
            auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"4821"}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let r = post_json(
            st.clone(),
            auth,
            None,
            "/api/vault/unlock",
            r#"{"pin":"4821"}"#,
        )
        .await;
        let vt = body_json(r).await["vault_token"]
            .as_str()
            .unwrap()
            .to_string();
        let r = vault_call(
            st.clone(),
            auth,
            Some(&vt),
            "POST",
            "/api/vault/add",
            Some(json!({ "ids": ids }).to_string()),
        )
        .await;
        assert_eq!(body_json(r).await["hidden"], n);
        (vt, ids)
    }

    /// Folders sort hidden photos without ever un-hiding them, and their names
    /// are as private as the photos.
    #[tokio::test]
    async fn vault_folders_group_without_leaking() {
        let st = test_state().await;
        let auth = bearer(&st);
        let (vts, ids) = vault_with_hidden(&st, &auth, 3).await;
        let vt = Some(vts.as_str());

        let listed = |st: AppState, auth: String, vt: String, q: &'static str| async move {
            let r = vault_call(
                st,
                &auth,
                Some(&vt),
                "GET",
                &format!("/api/vault/files{q}"),
                None,
            )
            .await;
            assert_eq!(r.status(), StatusCode::OK);
            let mut got: Vec<i64> = body_json(r).await["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["id"].as_i64().unwrap())
                .collect();
            got.sort();
            got
        };

        // No token, no folders — not even the list of names.
        let r = vault_call(st.clone(), &auth, None, "GET", "/api/vault/folders", None).await;
        assert_ne!(r.status(), StatusCode::OK, "folder list needs the PIN");
        let r = vault_call(
            st.clone(),
            &auth,
            None,
            "POST",
            "/api/vault/folders",
            Some(r#"{"name":"Secret"}"#.into()),
        )
        .await;
        assert_ne!(r.status(), StatusCode::OK, "creating needs the PIN too");

        // Create one; the same name in another case is the same folder.
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/folders",
            Some(r#"{"name":"Secret"}"#.into()),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let secret = body_json(r).await["id"].as_i64().unwrap();
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/folders",
            Some(r#"{"name":"  secret "}"#.into()),
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::CONFLICT,
            "names are case-insensitive"
        );
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/folders",
            Some(r#"{"name":"   "}"#.into()),
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::BAD_REQUEST,
            "a blank name is refused"
        );

        // Everything starts Unfiled.
        assert_eq!(
            listed(st.clone(), auth.clone(), vts.clone(), "?folder=unfiled").await,
            ids
        );
        assert_eq!(
            listed(st.clone(), auth.clone(), vts.clone(), "").await,
            ids,
            "All is everything"
        );

        // Move two in.
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/move",
            Some(json!({ "ids": [ids[0], ids[1]], "folder_id": secret }).to_string()),
        )
        .await;
        assert_eq!(body_json(r).await["moved"], 2);

        let r = vault_call(st.clone(), &auth, vt, "GET", "/api/vault/folders", None).await;
        let body = body_json(r).await;
        assert_eq!(body["folders"][0]["name"], "Secret");
        assert_eq!(body["folders"][0]["file_count"], 2);
        assert_eq!(
            body["folders"][0]["cover_file_id"], ids[1],
            "cover is the newest photo in it"
        );
        assert_eq!(body["unfiled"], 1);
        assert_eq!(body["total"], 3);
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "GET",
            &format!("/api/vault/files?folder={secret}"),
            None,
        )
        .await;
        assert_eq!(body_json(r).await["files"].as_array().unwrap().len(), 2);

        // The folder's name reaches no screen that opens without the PIN.
        for uri in [
            "/api/albums",
            "/api/media/batch?mode=library&size=50",
            "/api/stats/summary",
        ] {
            let r = vault_call(st.clone(), &auth, None, "GET", uri, None).await;
            let text = String::from_utf8(
                axum::body::to_bytes(r.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(!text.contains("Secret"), "{uri} mentions the folder");
            assert!(
                !text.contains("vault_folder"),
                "{uri} carries a field that says a vault exists"
            );
        }

        // Another account's folder cannot be filed into, read or renamed — and
        // is indistinguishable from one that does not exist.
        let theirs = db::create_vault_folder(&st.db, "eve", "Eve's", 1)
            .await
            .unwrap()
            .unwrap();
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/move",
            Some(json!({ "ids": [ids[2]], "folder_id": theirs }).to_string()),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "GET",
            &format!("/api/vault/files?folder={theirs}"),
            None,
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "PATCH",
            &format!("/api/vault/folders/{theirs}"),
            Some(r#"{"name":"mine now"}"#.into()),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);

        // Move cannot reach an ordinary library photo: the test's own x.jpg
        // (id 1) was never hidden.
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "POST",
            "/api/vault/move",
            Some(json!({ "ids": [1], "folder_id": secret }).to_string()),
        )
        .await;
        assert_eq!(
            body_json(r).await["moved"],
            0,
            "only hidden photos can be filed"
        );

        // Rename, including to a different case of its own name.
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "PATCH",
            &format!("/api/vault/folders/{secret}"),
            Some(r#"{"name":"SECRET"}"#.into()),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);

        // Deleting the folder keeps every photo hidden, now Unfiled.
        let r = vault_call(
            st.clone(),
            &auth,
            vt,
            "DELETE",
            &format!("/api/vault/folders/{secret}"),
            None,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["unfiled"], 2);
        assert_eq!(
            listed(st.clone(), auth.clone(), vts.clone(), "?folder=unfiled").await,
            ids
        );
        for id in &ids {
            let m = db::get_media(&st.db, *id).await.unwrap().unwrap();
            assert_eq!(m.status, "vault", "deleting a folder un-hid photo {id}");
        }
    }

    /// Hiding from inside a folder files the photo there; putting it back
    /// forgets the folder, so hiding it again lands it Unfiled.
    #[tokio::test]
    async fn vault_add_into_folder_and_restore_forgets_it() {
        let st = test_state().await;
        let auth = bearer(&st);
        let (vt, ids) = vault_with_hidden(&st, &auth, 1).await;

        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "POST",
            "/api/vault/folders",
            Some(r#"{"name":"Docs"}"#.into()),
        )
        .await;
        let docs = body_json(r).await["id"].as_i64().unwrap();

        // The test's own x.jpg, hidden straight into Docs.
        let dir = st.cfg.media_root.join("saiz");
        std::fs::create_dir_all(&dir).unwrap();
        let x = db::get_media(&st.db, 1).await.unwrap().unwrap();
        std::fs::write(&x.path, b"jpeg-bytes").unwrap();
        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "POST",
            "/api/vault/add",
            Some(json!({ "ids": [1], "folder_id": docs }).to_string()),
        )
        .await;
        assert_eq!(body_json(r).await["hidden"], 1);

        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "GET",
            &format!("/api/vault/files?folder={docs}"),
            None,
        )
        .await;
        let f = body_json(r).await;
        assert_eq!(f["files"].as_array().unwrap().len(), 1);
        assert_eq!(f["files"][0]["id"], 1);

        // A folder that isn't there refuses the whole add before anything moves.
        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "POST",
            "/api/vault/add",
            Some(json!({ "ids": [ids[0]], "folder_id": 9999 }).to_string()),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);

        // Put it back, then hide it again with no folder.
        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "POST",
            "/api/vault/restore",
            Some(r#"{"ids":[1]}"#.into()),
        )
        .await;
        assert_eq!(body_json(r).await["restored"], 1);
        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "POST",
            "/api/vault/add",
            Some(r#"{"ids":[1]}"#.into()),
        )
        .await;
        assert_eq!(body_json(r).await["hidden"], 1);

        let r = vault_call(
            st.clone(),
            &auth,
            Some(&vt),
            "GET",
            &format!("/api/vault/files?folder={docs}"),
            None,
        )
        .await;
        assert_eq!(
            body_json(r).await["files"].as_array().unwrap().len(),
            0,
            "the old folder did not come back with it"
        );
    }

    /// A vaulted photo must be absent from every way of listing the library.
    ///
    /// This is the test that has to keep passing. Hiding is enforced by an
    /// `AND status <> 'vault'` in a handful of queries, and the failure mode of
    /// that design is a *new* query that forgets one — which leaks silently,
    /// because nothing else breaks. Every assertion below is one of those
    /// queries, so adding a listing without the exclusion fails here rather
    /// than in front of whoever the vault was hiding from.
    #[tokio::test]
    async fn vault_leaks_into_nothing() {
        let st = test_state().await;
        let auth = bearer(&st);

        // Vaulting moves the file, so it has to be on disk.
        let dir = st.cfg.media_root.join("saiz");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.jpg"), b"jpeg-bytes").unwrap();

        // Keep it first, so it is a real part of the library before it is hidden.
        post_json(
            st.clone(),
            &auth,
            None,
            "/api/media/swipe",
            r#"{"file_id":1,"action":"keep"}"#,
        )
        .await;

        let seen = |st: AppState, auth: String, uri: &str| {
            let uri = uri.to_string();
            async move {
                let r = app(st)
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .header("authorization", auth)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                body_json(r).await
            }
        };

        // Visible to begin with — otherwise the assertions below prove nothing.
        assert_eq!(
            seen(
                st.clone(),
                auth.clone(),
                "/api/media/batch?mode=library&size=50"
            )
            .await["files"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "precondition: the photo is in the library"
        );

        // Set a PIN and unlock.
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"4821"}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK, "PIN accepted");
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/unlock",
            r#"{"pin":"4821"}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let vt = body_json(r).await["vault_token"]
            .as_str()
            .unwrap()
            .to_string();

        let r = post_json(
            st.clone(),
            &auth,
            Some(&vt),
            "/api/vault/add",
            r#"{"ids":[1]}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["hidden"], 1, "one photo hidden");

        // ── Now it must be gone from all of these. ────────────────────────
        for mode in [
            "library",
            "original",
            "cleaned",
            "timeline",
            "onthisday",
            "recents",
        ] {
            let uri = format!("/api/media/batch?mode={mode}&size=50");
            let n = seen(st.clone(), auth.clone(), &uri).await["files"]
                .as_array()
                .unwrap()
                .len();
            assert_eq!(n, 0, "batch mode `{mode}` still shows the hidden photo");
        }

        let s = seen(st.clone(), auth.clone(), "/api/stats/summary").await;
        assert_eq!(s["total"], 0, "stats counts the hidden photo");
        assert_eq!(s["keep"], 0, "stats counts it as kept");

        let tl = seen(st.clone(), auth.clone(), "/api/stats/timeline").await;
        let months = tl["months"].as_array().map(|m| m.len()).unwrap_or(0);
        assert_eq!(months, 0, "the timeline checklist still lists its month");

        let alb = seen(st.clone(), auth.clone(), "/api/albums").await;
        for a in alb["albums"].as_array().unwrap() {
            assert_eq!(a["file_count"], 0, "album {} still counts it", a["name"]);
        }

        let tr = seen(st.clone(), auth.clone(), "/api/trash").await;
        assert_eq!(
            tr["files"].as_array().map(|f| f.len()).unwrap_or(0),
            0,
            "the trash shows it"
        );

        // Per-file routes must not serve it with an ordinary login.
        for uri in [
            "/api/media/1/thumb",
            "/api/media/1/preview",
            "/api/media/1/raw",
            "/api/media/1/albums",
        ] {
            let r = app(st.clone())
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{uri} served it");
        }

        // And it really is in the vault, reachable only with the vault token.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/vault/files")
                    .header("x-vault-token", &vt)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["total"], 1, "the vault has it");

        // A login token is not a vault token.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/vault/files")
                    .header("x-vault-token", &auth.replace("Bearer ", ""))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::UNAUTHORIZED,
            "a login token opened the vault"
        );

        // The photo left the shared caches rather than merely being filtered.
        assert!(
            !preview_path(&st, 1).exists(),
            "the cached preview is still readable outside the vault"
        );
        assert!(
            !dir.join("x.jpg").exists(),
            "the original is still sitting in the ordinary library folder"
        );
    }

    /// A login alone must not be able to move a photo *out* of the vault.
    ///
    /// The listing queries were all careful to exclude hidden rows, but the two
    /// status writes were not: `swipe` and `duplicates/resolve` take a file id
    /// straight from the request body and never pass through `owned_media`, so
    /// "undo" at a vaulted id used to set the row back to `pending` — and from
    /// there `/raw` served the photo to someone who never entered the PIN. The
    /// vault defends against exactly one person, somebody holding an unlocked
    /// phone, and that was the person it let in.
    #[tokio::test]
    async fn a_login_cannot_swipe_a_photo_out_of_the_vault() {
        let st = test_state().await;
        let auth = bearer(&st);

        let dir = st.cfg.media_root.join("saiz");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.jpg"), b"jpeg-bytes").unwrap();

        // Hide it.
        post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"4821"}"#,
        )
        .await;
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/unlock",
            r#"{"pin":"4821"}"#,
        )
        .await;
        let vt = body_json(r).await["vault_token"]
            .as_str()
            .unwrap()
            .to_string();
        let r = post_json(
            st.clone(),
            &auth,
            Some(&vt),
            "/api/vault/add",
            r#"{"ids":[1]}"#,
        )
        .await;
        assert_eq!(
            body_json(r).await["hidden"],
            1,
            "precondition: it is hidden"
        );

        // Every status write reachable with a login token only. None may land.
        for (uri, body) in [
            ("/api/media/swipe", r#"{"file_id":1,"action":"undo"}"#),
            ("/api/media/swipe", r#"{"file_id":1,"action":"keep"}"#),
            ("/api/media/swipe", r#"{"file_id":1,"action":"delete"}"#),
            ("/api/media/duplicates/resolve", r#"{"undo":[1]}"#),
            ("/api/media/duplicates/resolve", r#"{"keep":[1]}"#),
            ("/api/media/duplicates/resolve", r#"{"delete":[1]}"#),
        ] {
            post_json(st.clone(), &auth, None, uri, body).await;

            // Still hidden: absent from the library, and `/raw` still refuses.
            let r = app(st.clone())
                .oneshot(
                    Request::builder()
                        .uri("/api/media/batch?mode=library&size=50")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                body_json(r).await["files"].as_array().unwrap().len(),
                0,
                "`{body}` to {uri} pulled the photo back into the library"
            );

            let r = app(st.clone())
                .oneshot(
                    Request::builder()
                        .uri("/api/media/1/raw")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                r.status(),
                StatusCode::NOT_FOUND,
                "`{body}` to {uri} made the hidden original readable"
            );
        }

        // And the vault still holds it — nothing was quietly destroyed either.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/vault/files")
                    .header("x-vault-token", &vt)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(r).await["total"], 1, "the vault lost the photo");
    }

    /// Guessing a 4-digit PIN is 10,000 tries; the throttle is what makes that
    /// cost something. Both PIN entry points are covered, since `current_pin`
    /// on the change-PIN route is the same oracle as unlock.
    #[tokio::test]
    async fn pin_guessing_is_throttled() {
        let st = test_state().await;
        let auth = bearer(&st);

        post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"4821"}"#,
        )
        .await;

        let mut locked_out = false;
        for _ in 0..12 {
            let r = post_json(
                st.clone(),
                &auth,
                None,
                "/api/vault/unlock",
                r#"{"pin":"0000"}"#,
            )
            .await;
            if r.status() == StatusCode::TOO_MANY_REQUESTS {
                locked_out = true;
                break;
            }
            assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "wrong PIN accepted");
        }
        assert!(locked_out, "unlimited PIN guesses allowed");

        // The change-PIN route must not become the way around it.
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"1111","current_pin":"0000"}"#,
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "change-PIN is a second, unthrottled PIN oracle"
        );
    }

    /// The vault is worthless if the wrong PIN opens it, or if changing the PIN
    /// doesn't need the old one.
    #[tokio::test]
    async fn the_vault_pin_is_actually_checked() {
        let st = test_state().await;
        let auth = bearer(&st);

        // Locked before a PIN exists, and unlock has nothing to check against.
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/unlock",
            r#"{"pin":"1234"}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);

        // Too short, and not digits.
        for bad in [r#"{"pin":"12"}"#, r#"{"pin":"abcd"}"#] {
            let r = post_json(st.clone(), &auth, None, "/api/vault/pin", bad).await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "accepted {bad}");
        }

        post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"4821"}"#,
        )
        .await;

        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/unlock",
            r#"{"pin":"0000"}"#,
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::UNAUTHORIZED,
            "wrong PIN unlocked it"
        );

        // Changing it without proving the old one.
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"9999"}"#,
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::UNAUTHORIZED,
            "the PIN was replaced without the current one"
        );

        // With it.
        let r = post_json(
            st.clone(),
            &auth,
            None,
            "/api/vault/pin",
            r#"{"pin":"9999","current_pin":"4821"}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);

        // Vault routes refuse a request with no vault token at all.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/vault/files")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn gallery_scopes_split_original_from_cleaned() {
        let st = test_state().await;
        let auth = bearer(&st);

        let count = |st: AppState, auth: String, mode: &str| {
            let uri = format!("/api/media/batch?mode={mode}&size=50");
            async move {
                let r = app(st)
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .header("authorization", auth)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                body_json(r).await["files"].as_array().unwrap().len()
            }
        };

        assert_eq!(count(st.clone(), auth.clone(), "original").await, 1);
        assert_eq!(count(st.clone(), auth.clone(), "cleaned").await, 0);

        app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/swipe")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"action":"keep"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            count(st.clone(), auth.clone(), "original").await,
            0,
            "no longer original"
        );
        assert_eq!(
            count(st.clone(), auth.clone(), "cleaned").await,
            1,
            "now cleaned"
        );
        assert_eq!(
            count(st.clone(), auth.clone(), "library").await,
            1,
            "still in All"
        );
    }

    /// Paging must cover every row exactly once — the Gallery stopped at 500
    /// before, and a page that repeats or skips is worse than a cap.
    #[tokio::test]
    async fn paging_walks_every_row_once() {
        let st = test_state().await;
        let auth = bearer(&st);

        // Seed enough rows to need several pages, all sharing a timestamp so
        // the tie-break in the ORDER BY is what has to keep paging stable.
        for i in 0..25 {
            db::insert_media(
                &st.db,
                &NewMedia {
                    user: "saiz".into(),
                    path: format!("/media/saiz/p{i}.jpg"),
                    rel_path: None,
                    filename: format!("p{i}.jpg"),
                    media_type: "image".into(),
                    size_bytes: 10,
                    created_at: 1_700_000_000,
                    gps_lat: None,
                    gps_lon: None,
                    content_hash: None,
                },
            )
            .await
            .unwrap();
        }

        let mut seen: Vec<i64> = Vec::new();
        for page in 1..=5 {
            let r = app(st.clone())
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/media/batch?mode=library&size=6&page={page}"))
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let v = body_json(r).await;
            assert_eq!(
                v["total"],
                json!(26),
                "total is the whole set, not the page"
            );
            for f in v["files"].as_array().unwrap() {
                seen.push(f["id"].as_i64().unwrap());
            }
        }

        let mut uniq = seen.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(seen.len(), 26, "every row returned");
        assert_eq!(uniq.len(), 26, "and none of them twice");
    }

    /// Deleting from the Gallery bins the file immediately — there is no
    /// session to finish there — and Put back returns it to `source/` as
    /// unreviewed, not into the cleaned folder.
    #[tokio::test]
    async fn gallery_delete_then_put_back_lands_in_original() {
        let st = test_state().await;
        let auth = bearer(&st);

        // A real file on disk, so the move is exercised rather than skipped.
        let src_dir = st.cfg.media_root.join("saiz").join("source");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("shot.jpg");
        std::fs::write(&src, b"not really a jpeg").unwrap();
        db::update_path(&st.db, 1, &src.to_string_lossy(), "source/shot.jpg")
            .await
            .unwrap();

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/1/trash")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["status"], json!("trashed"));

        let moved = db::get_media(&st.db, 1).await.unwrap().unwrap();
        assert!(
            moved.path.contains("/.trash/"),
            "moved to .trash now: {}",
            moved.path
        );
        assert!(
            moved.trashed_at.is_some(),
            "clock started for the countdown"
        );
        assert!(!src.exists(), "gone from source/");

        // Put back.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/trash/restore/1")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        let back = db::get_media(&st.db, 1).await.unwrap().unwrap();
        assert_eq!(back.status, "pending", "unreviewed again, not cleaned");
        assert!(
            back.path.contains("/source/"),
            "back in source/: {}",
            back.path
        );
        assert!(
            !back.path.contains("/clean/"),
            "never into the cleaned folder"
        );
    }

    /// Originals are served in ranges, the way Safari asks for a video.
    ///
    /// iOS opens a clip with `Range: bytes=0-1` and needs a `206` back; given
    /// the whole file instead it downloads all of it before showing a frame,
    /// and cannot seek. This pins the partial answer, and that an ordinary
    /// request still gets the whole file and says ranges are available.
    #[tokio::test]
    async fn originals_answer_byte_ranges() {
        let st = test_state().await;
        let m = db::get_media(&st.db, 1).await.unwrap().unwrap();
        std::fs::create_dir_all(FsPath::new(&m.path).parent().unwrap()).unwrap();
        std::fs::write(&m.path, b"0123456789abcdef").unwrap();
        let token = core_auth::make_media_token("saiz", &st.cfg.secret, 60).unwrap();

        let get = |st: AppState, range: Option<&'static str>| {
            let uri = format!("/api/media/1/raw?token={token}");
            async move {
                let mut req = Request::builder().uri(uri);
                if let Some(r) = range {
                    req = req.header("range", r);
                }
                app(st)
                    .oneshot(req.body(Body::empty()).unwrap())
                    .await
                    .unwrap()
            }
        };
        let bytes = |r: Response| async move {
            axum::body::to_bytes(r.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        };

        let r = get(st.clone(), Some("bytes=0-1")).await;
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT, "Safari's probe");
        assert_eq!(r.headers()["content-range"], "bytes 0-1/16");
        assert_eq!(bytes(r).await, b"01");

        let r = get(st.clone(), Some("bytes=10-")).await;
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT, "a seek");
        assert_eq!(bytes(r).await, b"abcdef");

        let r = get(st.clone(), None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["accept-ranges"], "bytes");
        assert_eq!(bytes(r).await, b"0123456789abcdef");
    }

    /// Manual album filing, used by the viewer's Album action.
    #[tokio::test]
    async fn album_add_creates_then_reuses() {
        let st = test_state().await;
        let auth = bearer(&st);

        let add = |st: AppState, auth: String, body: &'static str| async move {
            app(st)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/albums/add")
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
        };

        let r = add(st.clone(), auth.clone(), r#"{"file_id":1,"name":"Trip"}"#).await;
        assert_eq!(r.status(), StatusCode::OK);
        let album_id = body_json(r).await["album_id"].as_i64().unwrap();

        let albums = db::list_albums(&st.db, "saiz").await.unwrap();
        let made = albums
            .iter()
            .find(|a| a.id == album_id)
            .expect("album created");
        assert_eq!(made.name, "Trip");
        assert_eq!(made.file_count, 1, "count refreshed on add");

        // Filing the same photo again must not duplicate the membership row.
        let r = add(st.clone(), auth.clone(), r#"{"file_id":1,"name":"Trip"}"#).await;
        assert_eq!(r.status(), StatusCode::OK);
        let albums = db::list_albums(&st.db, "saiz").await.unwrap();
        assert_eq!(albums.iter().filter(|a| a.name == "Trip").count(), 1);
        assert_eq!(
            albums.iter().find(|a| a.id == album_id).unwrap().file_count,
            1
        );

        // Neither an id nor a name is a bad request, not a silent no-op.
        let r = add(st.clone(), auth.clone(), r#"{"file_id":1}"#).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    /// A binned photo leaves every album it was filed under — and comes back
    /// to them when it is put back.
    ///
    /// This is the bug that made an album a place a deleted photo could hide:
    /// the Gallery filters on status and the trash lists it, but the album grid
    /// joined `album_file` with no filter at all, so the photo stayed on the
    /// shelf, kept its place in the count, and could not be removed from there
    /// — the viewer's Delete answers "already in the trash" for a file that is.
    #[tokio::test]
    async fn binning_a_photo_takes_it_off_the_album_shelf() {
        let st = test_state().await;
        let auth = bearer(&st);

        // File the one test photo into an album of the user's own.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/albums/add")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"name":"Trip"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let album_id = body_json(r).await["album_id"].as_i64().unwrap();

        let listed = |st: AppState, auth: String, id: i64| async move {
            let r = app(st)
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/albums/{id}/files"))
                        .header("authorization", auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::OK);
            body_json(r).await["files"].as_array().unwrap().len()
        };
        let count_of = |st: AppState, id: i64| async move {
            db::list_albums(&st.db, "saiz")
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.id == id)
                .expect("album still there")
        };

        assert_eq!(listed(st.clone(), auth.clone(), album_id).await, 1);
        let before = count_of(st.clone(), album_id).await;
        assert_eq!(before.file_count, 1);
        assert_eq!(before.cover_file_id, Some(1), "cover is the photo in it");

        // Bin it.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/1/trash")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        assert_eq!(
            listed(st.clone(), auth.clone(), album_id).await,
            0,
            "gone from the album grid"
        );
        let after = count_of(st.clone(), album_id).await;
        assert_eq!(after.file_count, 0, "and gone from the count");
        assert_eq!(
            after.cover_file_id, None,
            "a deleted photo is not an album cover"
        );

        // …but the membership itself survived, so a restore undoes the whole
        // thing — including for a manual album, which nothing ever rebuilds.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/trash/restore/1")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        assert_eq!(
            listed(st.clone(), auth.clone(), album_id).await,
            1,
            "back on the shelf"
        );
        assert_eq!(count_of(st.clone(), album_id).await.file_count, 1);

        // Purging for good is what finally drops the membership row.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/1/trash")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/trash/purge/1")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        assert!(
            db::album_ids_for_file(&st.db, 1).await.unwrap().is_empty(),
            "membership goes with the row"
        );
        assert_eq!(count_of(st.clone(), album_id).await.file_count, 0);
    }

    /// The trash reads as a history of what you just deleted, so it must be
    /// ordered by deletion time — not by when the photo was taken.
    #[tokio::test]
    async fn trash_lists_newest_deletion_first() {
        let st = test_state().await;

        // An old photo binned recently must outrank a new photo binned earlier.
        let old_photo = db::insert_media(
            &st.db,
            &NewMedia {
                user: "saiz".into(),
                path: "/media/saiz/ancient.jpg".into(),
                rel_path: None,
                filename: "ancient.jpg".into(),
                media_type: "image".into(),
                size_bytes: 10,
                created_at: 1_000_000_000, // taken long ago
                gps_lat: None,
                gps_lon: None,
                content_hash: None,
            },
        )
        .await
        .unwrap();

        db::set_status(&st.db, "saiz", 1, "trashed", Some(1_700_000_100))
            .await
            .unwrap();
        db::set_status(&st.db, "saiz", old_photo, "trashed", Some(1_700_000_900))
            .await
            .unwrap();

        let listed = db::list_status(&st.db, "saiz", "trashed").await.unwrap();
        assert_eq!(
            listed.first().map(|f| f.id),
            Some(old_photo),
            "most recently binned comes first, regardless of capture date"
        );
    }

    /// Helper: insert one photo with an explicit capture time.
    async fn add_photo(st: &AppState, name: &str, created_at: i64) -> i64 {
        db::insert_media(
            &st.db,
            &NewMedia {
                user: "saiz".into(),
                path: format!("/media/saiz/{name}"),
                rel_path: None,
                filename: name.into(),
                media_type: "image".into(),
                size_bytes: 10,
                created_at,
                gps_lat: None,
                gps_lon: None,
                content_hash: None,
            },
        )
        .await
        .unwrap()
    }

    /// Count what a batch mode returns, via the HTTP surface the app uses.
    async fn batch_total(st: AppState, auth: &str, mode: &str) -> i64 {
        let r = app(st)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/batch?mode={mode}&size=1"))
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        body_json(r).await["total"].as_i64().unwrap()
    }

    /// `onthisday` means this calendar day in any year, and `recents` means the
    /// last week. Both used to fall through to the plain `timeline` branch,
    /// which is why the two cards showed the same photos as each other and the
    /// same count as everything else.
    #[tokio::test]
    async fn on_this_day_and_recents_filter_by_date() {
        let st = test_state().await;
        let auth = bearer(&st);

        let now = now_secs();
        // The fixture photo from `test_state` is at a fixed 2023 timestamp and
        // is neither recent nor (all but surely) today's date.
        let baseline_today = batch_total(st.clone(), &auth, "onthisday").await;

        // Same month-day, four years back → on this day, but not recent.
        let four_years = 4 * 365 * 86_400 + 86_400; // +1 day for leap years
        add_photo(&st, "anniversary.jpg", now - four_years).await;
        // Yesterday → recent, but (almost certainly) not this day-of-year.
        add_photo(&st, "yesterday.jpg", now - 86_400).await;
        // Long ago → neither.
        add_photo(&st, "ancient.jpg", now - 400 * 86_400 - 43_200).await;

        let today = batch_total(st.clone(), &auth, "onthisday").await;
        let recents = batch_total(st.clone(), &auth, "recents").await;
        let timeline = batch_total(st.clone(), &auth, "timeline").await;

        assert!(
            today > baseline_today,
            "a photo from this day in an earlier year belongs to On This Day"
        );
        assert_eq!(recents, 1, "only yesterday's photo is from the last week");
        assert_eq!(timeline, 4, "timeline is still everything pending");
        assert!(
            today < timeline,
            "On This Day must be a subset, not the whole deck"
        );
    }

    /// Duplicates are reviewed as whole clusters — two copies of the same photo
    /// have to arrive together, or the compare screen can only ask about one of
    /// them at a time.
    #[tokio::test]
    async fn duplicate_groups_arrive_whole_and_resolve_together() {
        let st = test_state().await;
        let auth = bearer(&st);

        let a = add_photo(&st, "copy_a.jpg", 1_700_000_100).await;
        let b = add_photo(&st, "copy_b.jpg", 1_700_000_200).await;
        let lonely = add_photo(&st, "unique.jpg", 1_700_000_300).await;
        db::set_group(&st.db, a, "g1").await.unwrap();
        db::set_group(&st.db, b, "g1").await.unwrap();
        // A group of one is not a duplicate of anything and must not show up.
        db::set_group(&st.db, lonely, "g2").await.unwrap();

        let groups = |st: AppState, auth: String| async move {
            let r = app(st)
                .oneshot(
                    Request::builder()
                        .uri("/api/media/duplicates")
                        .header("authorization", auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::OK);
            body_json(r).await
        };

        let v = groups(st.clone(), auth.clone()).await;
        assert_eq!(v["total_groups"], 1, "a cluster of one is not a duplicate");
        assert_eq!(v["total_files"], 2);
        assert_eq!(v["groups"][0]["files"].as_array().unwrap().len(), 2);

        // Settle it: keep one copy, bin the other, in a single call.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/duplicates/resolve")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"keep":[{a}],"delete":[{b}]}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        assert_eq!(v["kept"], 1);
        assert_eq!(v["deleted"], 1);

        // The whole group leaves at once — no half-decided cluster comes back.
        let v = groups(st.clone(), auth.clone()).await;
        assert_eq!(v["total_groups"], 0);

        // Undo puts both back.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/duplicates/resolve")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"undo":[{a},{b}]}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await["restored"], 2);
        let v = groups(st.clone(), auth.clone()).await;
        assert_eq!(v["total_groups"], 1, "undo restores the whole cluster");
    }

    /// The shell carries a CSP, and every response carries the frame/sniff
    /// guards.
    ///
    /// The session JWT lives in `localStorage`, so any script that manages to
    /// run on this origin owns the account outright rather than one session —
    /// the CSP is what leaves it nowhere to load from. The frame guard matters
    /// because emptying the trash is permanent and one tap away.
    #[tokio::test]
    async fn responses_carry_security_headers() {
        let st = test_state().await;
        let get = |st: AppState, uri: &'static str| async move {
            app(st)
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
        };
        let header_of = |r: &Response<Body>, h: header::HeaderName| {
            r.headers()
                .get(h)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };

        // On the API too — it is the half worth framing or sniffing.
        for uri in ["/sw.js", "/api/health"] {
            let r = get(st.clone(), uri).await;
            assert_eq!(header_of(&r, header::X_FRAME_OPTIONS), "DENY", "{uri}");
            assert_eq!(
                header_of(&r, header::X_CONTENT_TYPE_OPTIONS),
                "nosniff",
                "{uri}"
            );
            assert_eq!(
                header_of(&r, header::REFERRER_POLICY),
                "no-referrer",
                "{uri} — the token rides in query strings, so a referrer leaks it"
            );
        }

        let csp = header_of(
            &get(st.clone(), "/sw.js").await,
            header::CONTENT_SECURITY_POLICY,
        );
        assert!(csp.contains("frame-ancestors 'none'"), "csp: {csp}");
        assert!(csp.contains("object-src 'none'"), "csp: {csp}");
        assert!(csp.contains("base-uri 'none'"), "csp: {csp}");
        assert!(
            csp.contains("default-src 'self'"),
            "csp must not be open by default: {csp}"
        );
        // Inter is pulled from Google Fonts by tokens.css; a CSP that forgets
        // it silently drops the app's typography.
        assert!(
            csp.contains("https://fonts.googleapis.com")
                && csp.contains("https://fonts.gstatic.com"),
            "csp would block the webfont: {csp}"
        );
    }

    /// The shell must be revalidated on every launch or an installed PWA keeps
    /// serving the build it first saw; hashed assets are the opposite case.
    #[tokio::test]
    async fn static_shell_is_revalidated_and_assets_are_immutable() {
        let st = test_state().await;
        let cache = |r: &Response<Body>| {
            r.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/sw.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cache(&r), "no-cache");

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/assets/index-abc123.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cache(&r), "public, max-age=31536000, immutable");

        // The API sets its own headers and must not be touched.
        let r = app(st)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cache(&r), "");
        assert_eq!(body_json(r).await["version"], env!("CARGO_PKG_VERSION"));
    }

    /// Hearting a photo has to fill the Favourites album then and there.
    /// Waiting for a session finish meant the album stayed empty for a photo
    /// you hearted from the Gallery, possibly forever.
    #[tokio::test]
    async fn favouriting_files_the_photo_immediately() {
        let st = test_state().await;
        let auth = bearer(&st);

        let toggle = |st: AppState, auth: String| async move {
            let r = app(st)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/media/1/favorite")
                        .header("authorization", auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::OK);
            body_json(r).await["is_favorite"].as_bool().unwrap()
        };

        assert!(
            toggle(st.clone(), auth.clone()).await,
            "first tap hearts it"
        );
        let albums = db::list_albums(&st.db, "saiz").await.unwrap();
        let fav = albums
            .iter()
            .find(|a| a.category_type == "favorites")
            .expect("Favourites album exists as soon as something is hearted");
        assert_eq!(fav.file_count, 1);

        // Un-hearting takes it back out rather than leaving a stale member.
        assert!(!toggle(st.clone(), auth.clone()).await);
        let albums = db::list_albums(&st.db, "saiz").await.unwrap();
        let fav = albums
            .iter()
            .find(|a| a.category_type == "favorites")
            .unwrap();
        assert_eq!(fav.file_count, 0, "un-hearting removes it");
    }

    /// Auto-built albums say what a photo *is*. Hand-filing into one would
    /// state something untrue — a 2021 photo cannot belong to 2023 — and the
    /// next rebuild would undo it regardless.
    #[tokio::test]
    async fn only_your_own_albums_accept_a_manual_add() {
        let st = test_state().await;
        let auth = bearer(&st);

        let add = |st: AppState, auth: String, body: String| async move {
            app(st)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/albums/add")
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
        };

        // A year album, exactly as categorize would build one.
        let year = db::get_or_create_album(&st.db, "saiz", "2021", "year", 1_700_000_000)
            .await
            .unwrap();
        let r = add(
            st.clone(),
            auth.clone(),
            format!(r#"{{"file_id":1,"album_id":{year}}}"#),
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::BAD_REQUEST,
            "a year album is locked to its rule"
        );

        // One of your own takes it.
        let r = add(
            st.clone(),
            auth.clone(),
            r#"{"file_id":1,"name":"Food"}"#.to_string(),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let id = body_json(r).await["album_id"].as_i64().unwrap();
        let made = db::get_album(&st.db, "saiz", id).await.unwrap().unwrap();
        assert_eq!(made.category_type, "manual");

        let r = add(
            st.clone(),
            auth.clone(),
            format!(r#"{{"file_id":1,"album_id":{id}}}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK, "your own album stays open");

        // And Details can read back where it landed.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/media/1/albums")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        let names: Vec<String> = v["albums"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["Food"]);
    }

    /// The Gallery must keep showing a photo after you keep it. Every review
    /// mode is scoped to `pending`, which is right for a swipe deck and wrong
    /// for a library view — `library` mode is what separates the two.
    #[tokio::test]
    async fn library_mode_keeps_reviewed_photos_visible() {
        let st = test_state().await;
        let auth = bearer(&st);

        let count = |st: AppState, auth: String, mode: &str| {
            let uri = format!("/api/media/batch?mode={mode}&size=50");
            async move {
                let r = app(st)
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .header("authorization", auth)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(r.status(), StatusCode::OK);
                body_json(r).await["files"].as_array().unwrap().len()
            }
        };

        assert_eq!(count(st.clone(), auth.clone(), "library").await, 1);
        assert_eq!(count(st.clone(), auth.clone(), "timeline").await, 1);

        // Keep it: it leaves the review deck but must stay in the library.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/swipe")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"action":"keep"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        assert_eq!(
            count(st.clone(), auth.clone(), "timeline").await,
            0,
            "a kept photo is out of the review deck"
        );
        assert_eq!(
            count(st.clone(), auth.clone(), "library").await,
            1,
            "but it is still in the gallery"
        );

        // Trashing does remove it from the library — trash has its own screen.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/swipe")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"action":"delete"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(count(st.clone(), auth.clone(), "library").await, 0);
    }

    /// The Settings slider has to change real behaviour, not just persist a
    /// number: saving a distance must re-cluster the library immediately.
    #[tokio::test]
    async fn duplicate_sensitivity_persists_and_regroups() {
        let st = test_state().await;
        let auth = bearer(&st);

        // Two identical hashes plus one far away. At distance 0 the identical
        // pair groups and the outlier doesn't; the pair is the only group.
        for (id, phash) in [(1i64, "ffffffffffffffff")] {
            db::update_analysis(&st.db, id, Some(500.0), false, phash, None)
                .await
                .unwrap();
        }
        for (path, phash) in [
            ("media/saiz/dup.jpg", "ffffffffffffffff"),
            ("media/saiz/other.jpg", "0000000000000000"),
        ] {
            let id = db::insert_media(
                &st.db,
                &NewMedia {
                    user: "saiz".into(),
                    path: st.cfg.data_dir.join(path).to_string_lossy().into(),
                    rel_path: None,
                    filename: "f.jpg".into(),
                    media_type: "image".into(),
                    size_bytes: 10,
                    created_at: 1_700_000_000,
                    gps_lat: None,
                    gps_lon: None,
                    content_hash: None,
                },
            )
            .await
            .unwrap();
            db::update_analysis(&st.db, id, Some(500.0), false, phash, None)
                .await
                .unwrap();
        }

        // Default, before anything is saved, is the env/config value.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            body_json(r).await["duplicate_distance"],
            json!(st.cfg.duplicate_distance_threshold)
        );

        // Saving 0 groups only the byte-identical pair.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"duplicate_distance":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        assert_eq!(v["duplicate_distance"], json!(0));
        assert_eq!(v["duplicate_groups"], json!(1), "the identical pair groups");

        // It survives the request — this is the bit the old UI never did.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(r).await["duplicate_distance"], json!(0));

        // At the widest setting everything collapses into one group, proving
        // the value actually feeds the clustering.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"duplicate_distance":20}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = body_json(r).await;
        assert_eq!(v["duplicate_distance"], json!(20));
        assert_eq!(v["duplicate_groups"], json!(1));

        // Out-of-range is rejected rather than silently clamped.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"duplicate_distance":64}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    /// The Settings chart is fed by this endpoint; it used to be a hardcoded
    /// array of bars. Every day in the window must be present, even empty ones,
    /// and the totals must add up to what was actually binned.
    #[tokio::test]
    async fn storage_series_reports_real_trashed_bytes() {
        let st = test_state().await;
        let auth = bearer(&st);

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/stats/storage?days=30")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v = body_json(r).await;
        assert_eq!(
            v["days"].as_array().unwrap().len(),
            30,
            "zero-filled window"
        );
        assert_eq!(v["total_bytes"], json!(0));

        // Bin the seeded 10-byte file, then it should show up today.
        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/swipe")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_id":1,"action":"delete"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/stats/storage?days=30")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = body_json(r).await;
        assert_eq!(v["total_bytes"], json!(10));
        assert_eq!(v["total_files"], json!(1));
        let days = v["days"].as_array().unwrap();
        assert_eq!(days.last().unwrap()["bytes"], json!(10), "landed on today");
    }

    /// A preview is rendered from the original and cached; it must be bigger
    /// than the thumbnail, or the swipe deck is no easier to judge than before.
    #[tokio::test]
    async fn preview_renders_larger_than_thumb_and_caches() {
        use image::{Rgb, RgbImage};

        let st = test_state().await;
        let auth = bearer(&st);

        // Replace the seeded row's path with a real on-disk image bigger than
        // THUMB_MAX, so the two sizes are actually distinguishable.
        let media_dir = st.cfg.media_root.join("saiz");
        std::fs::create_dir_all(&media_dir).unwrap();
        let src = media_dir.join("big.png");
        let mut img = RgbImage::new(1200, 800);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let v = ((x * 3 + y * 7) % 256) as u8;
            *px = Rgb([v, v.wrapping_mul(2), 255 - v]);
        }
        img.save(&src).unwrap();
        db::update_path(&st.db, 1, &src.to_string_lossy(), "source/big.png")
            .await
            .unwrap();

        let cached = st.cfg.data_dir.join("previews").join("1.jpg");
        assert!(!cached.exists(), "nothing cached before the first request");

        let r = app(st.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/media/1/preview")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();

        assert!(cached.exists(), "preview cached for the next request");
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(
            decoded.width() > easyclean_core::thumbs::THUMB_MAX,
            "preview ({}px) must beat the {}px thumbnail",
            decoded.width(),
            easyclean_core::thumbs::THUMB_MAX
        );
        assert!(decoded.width() <= easyclean_core::thumbs::PREVIEW_MAX);
    }

    /// Another user's id must not be readable through the preview route, the
    /// same as thumb and raw.
    #[tokio::test]
    async fn preview_refuses_another_users_media() {
        let st = test_state().await;
        let other = db::insert_media(
            &st.db,
            &NewMedia {
                user: "someone-else".into(),
                path: "/media/other/secret.jpg".into(),
                rel_path: None,
                filename: "secret.jpg".into(),
                media_type: "image".into(),
                size_bytes: 10,
                created_at: 1_700_000_000,
                gps_lat: None,
                gps_lon: None,
                content_hash: None,
            },
        )
        .await
        .unwrap();
        let auth = bearer(&st);

        let r = app(st)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/{other}/preview"))
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn album_list_empty_then_refresh() {
        let st = test_state().await;
        let auth = bearer(&st);
        let a = app(st);

        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/albums")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["albums"].as_array().unwrap().len(), 0);

        // refresh (no kept files yet — should be a no-op, not an error)
        let j = body_json(
            a.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/albums/refresh")
                        .header("authorization", &auth)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(j["refreshed"], 0);
    }
}

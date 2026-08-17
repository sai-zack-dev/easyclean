use std::collections::HashMap;
use std::convert::Infallible;
use std::path::Path as FsPath;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{json, Value};

use easyclean_core::auth as core_auth;
use easyclean_core::categorize;
use easyclean_core::fileops;
use easyclean_core::models::MediaFile;
use easyclean_core::storage::UserRoot;
use easyclean_core::{db, detect, scan};

use crate::auth::AuthUser;
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
        // scan
        .route("/api/scan", post(scan_start))
        .route("/api/scan/status", get(scan_status_sse))
        // media
        .route("/api/media/batch", get(batch))
        .route("/api/media/swipe", post(swipe))
        .route("/api/media/session/finish", post(session_finish))
        .route("/api/media/:id/thumb", get(thumb))
        .route("/api/media/:id/raw", get(raw))
        .route("/api/media/:id/favorite", post(favorite))
        // albums
        .route("/api/albums", get(albums_list))
        .route("/api/albums/refresh", post(albums_refresh))
        .route("/api/albums/:id/files", get(album_files))
        // stats
        .route("/api/stats/summary", get(stats))
        .route("/api/stats/streak", get(streak))
        .route("/api/stats/timeline", get(timeline))
        // trash
        .route("/api/trash", get(trash_list))
        .route("/api/trash/restore/:id", post(trash_restore))
        .route("/api/trash/purge/:id", delete(trash_purge_one))
        .route("/api/trash/purge-all", delete(trash_purge_all))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors_layer(&state.cfg))
        .with_state(state)
        .fallback_service(static_files)
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
    Json(json!({ "status": "ok" }))
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
    match detect::run_detection(
        &st.db,
        &user,
        st.cfg.blur_threshold,
        &thumb_dir,
        st.cfg.duplicate_distance_threshold,
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
    _user: AuthUser,
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

// ── Media ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BatchQuery {
    mode: Option<String>,
    month: Option<String>,
    size: Option<i64>,
}

async fn batch(
    State(st): State<AppState>,
    user: AuthUser,
    Query(q): Query<BatchQuery>,
) -> Result<Json<Value>, ApiError> {
    let mode = q.mode.as_deref().unwrap_or("timeline");
    let size = q.size.unwrap_or(50).clamp(1, 500);
    let files = db::get_batch(&st.db, &user.username, mode, q.month.as_deref(), size).await?;
    // `total` is every pending match for the mode, not just this page — the UI
    // shows it as the review-progress denominator.
    let total = db::count_batch(&st.db, &user.username, mode, q.month.as_deref()).await?;
    Ok(Json(json!({ "files": enrich(files), "total": total })))
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

async fn favorite(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let fav = db::toggle_favorite(&st.db, &user.username, id).await?;
    Ok(Json(json!({ "id": id, "is_favorite": fav })))
}

async fn thumb(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let m = owned_media(&st, &user, id).await?;
    let tp = m
        .thumb_path
        .ok_or_else(|| ApiError::not_found("no thumbnail yet"))?;
    serve_file(FsPath::new(&tp)).await
}

async fn raw(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let m = owned_media(&st, &user, id).await?;
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
    serve_file(p).await
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
    let m = owned_media(&st, &user, id).await?;
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
    Ok(Json(json!({ "id": id, "status": "pending" })))
}

async fn trash_purge_one(
    State(st): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let m = owned_media(&st, &user, id).await?;
    if m.status != "trashed" {
        return Err(ApiError::bad("file is not in trash"));
    }
    fileops::purge(FsPath::new(&m.path)).map_err(ApiError::internal)?;
    db::delete_media_row(&st.db, id).await?;
    Ok(Json(json!({ "purged": id })))
}

async fn trash_purge_all(
    State(st): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let files = db::list_status(&st.db, &user.username, "trashed").await?;
    let mut purged = 0usize;
    for f in files {
        fileops::purge(FsPath::new(&f.path)).map_err(ApiError::internal)?;
        db::delete_media_row(&st.db, f.id).await?;
        purged += 1;
    }
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
                for f in expired {
                    if let Err(e) = fileops::purge(FsPath::new(&f.path)) {
                        tracing::warn!("retention purge {}: {e}", f.path);
                    } else if let Err(e) = db::delete_media_row(&st.db, f.id).await {
                        tracing::warn!("retention db delete {}: {e}", f.id);
                    } else {
                        tracing::info!("retention: purged {}", f.filename);
                    }
                }
            }
            Err(e) => tracing::error!("retention: expired_trash for {user}: {e}"),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn owned_media(st: &AppState, user: &AuthUser, id: i64) -> Result<MediaFile, ApiError> {
    let m = db::get_media(&st.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("media not found"))?;
    if m.user != user.username {
        return Err(ApiError::not_found("media not found"));
    }
    Ok(m)
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
                map.insert("raw_url".into(), json!(format!("/api/media/{id}/raw")));
            }
            v
        })
        .collect()
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
            },
        )
        .await
        .unwrap();
        AppState {
            db,
            cfg: Arc::new(cfg),
            scan: Default::default(),
            rate_limiter: Default::default(),
        }
    }

    fn bearer(st: &AppState) -> String {
        format!(
            "Bearer {}",
            core_auth::make_token("saiz", "admin", &st.cfg.secret, 7).unwrap()
        )
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

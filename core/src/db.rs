use std::collections::HashSet;
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::models::{Album, MediaFile, NewMedia, StatsSummary, TimelineMonth};

/// Shared database handle.
pub type Db = SqlitePool;

/// Open (creating if needed) the SQLite database at `db_path` and run migrations.
pub async fn init(db_path: &Path) -> Result<Db> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Insert a freshly-scanned file (ignored if its path already exists);
/// returns the new id (0 if it was ignored).
pub async fn insert_media(pool: &Db, m: &NewMedia) -> Result<i64> {
    let res = sqlx::query(
        "INSERT OR IGNORE INTO media_file \
         (user, path, rel_path, filename, media_type, size_bytes, created_at, gps_lat, gps_lon) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&m.user)
    .bind(&m.path)
    .bind(&m.rel_path)
    .bind(&m.filename)
    .bind(&m.media_type)
    .bind(m.size_bytes)
    .bind(m.created_at)
    .bind(m.gps_lat)
    .bind(m.gps_lon)
    .execute(pool)
    .await?;
    Ok(res.last_insert_rowid())
}

/// Count a user's indexed files.
pub async fn count_media(pool: &Db, user: &str) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM media_file WHERE user = ?")
        .bind(user)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// All known file paths for a user (used to skip already-indexed files).
pub async fn existing_paths(pool: &Db, user: &str) -> Result<HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT path FROM media_file WHERE user = ?")
        .bind(user)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Files that still need analysis (no perceptual hash yet): `(id, path, media_type)`.
pub async fn pending_for_detection(pool: &Db, user: &str) -> Result<Vec<(i64, String, String)>> {
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, path, media_type FROM media_file WHERE user = ? AND phash IS NULL",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Store analysis results for one file.
pub async fn update_analysis(
    pool: &Db,
    id: i64,
    blur_score: Option<f64>,
    is_blurry: bool,
    phash: &str,
    thumb_path: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE media_file SET blur_score = ?, is_blurry = ?, phash = ?, thumb_path = ? WHERE id = ?",
    )
    .bind(blur_score)
    .bind(is_blurry as i64)
    .bind(phash)
    .bind(thumb_path)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record that a video produced a thumbnail (no hash/blur for video).
pub async fn set_thumb(pool: &Db, id: i64, thumb_path: &str) -> Result<()> {
    sqlx::query("UPDATE media_file SET thumb_path = ? WHERE id = ?")
        .bind(thumb_path)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// `(id, phash)` for every hashed file of a user — input to duplicate grouping.
pub async fn all_phashes(pool: &Db, user: &str) -> Result<Vec<(i64, String)>> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, phash FROM media_file WHERE user = ? AND phash IS NOT NULL",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Assign a duplicate-group id to a file.
pub async fn set_group(pool: &Db, id: i64, group_id: &str) -> Result<()> {
    sqlx::query("UPDATE media_file SET group_id = ? WHERE id = ?")
        .bind(group_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch one file by id.
pub async fn get_media(pool: &Db, id: i64) -> Result<Option<MediaFile>> {
    let row = sqlx::query_as::<_, MediaFile>("SELECT * FROM media_file WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// A review batch for one mode: `timeline` (optional `YYYY-MM` month), `blurs`,
/// or `duplicates`. Only `pending` files are returned, newest first.
pub async fn get_batch(
    pool: &Db,
    user: &str,
    mode: &str,
    month: Option<&str>,
    size: i64,
) -> Result<Vec<MediaFile>> {
    let (filter, month_bind) = batch_filter(mode, month);
    let sql = format!("SELECT * FROM media_file {filter} ORDER BY created_at DESC LIMIT ?");

    let mut q = sqlx::query_as::<_, MediaFile>(&sql).bind(user);
    if let Some(m) = &month_bind {
        q = q.bind(m);
    }
    q = q.bind(size);
    Ok(q.fetch_all(pool).await?)
}

/// How many pending files match a batch mode, ignoring the page size — the
/// denominator the swipe screen's progress counter needs.
pub async fn count_batch(pool: &Db, user: &str, mode: &str, month: Option<&str>) -> Result<i64> {
    let (filter, month_bind) = batch_filter(mode, month);
    let sql = format!("SELECT COUNT(*) FROM media_file {filter}");

    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(user);
    if let Some(m) = &month_bind {
        q = q.bind(m);
    }
    Ok(q.fetch_one(pool).await?)
}

/// Shared `WHERE` clause for `get_batch`/`count_batch`, plus the optional
/// month value to bind after `user`.
fn batch_filter(mode: &str, month: Option<&str>) -> (String, Option<String>) {
    let mut sql = String::from("WHERE user = ? AND status = 'pending'");
    let mut month_bind: Option<String> = None;
    match mode {
        "blurs" => sql.push_str(" AND is_blurry = 1"),
        "duplicates" => sql.push_str(" AND group_id IS NOT NULL"),
        _ => {
            if let Some(m) = month {
                sql.push_str(" AND strftime('%Y-%m', created_at, 'unixepoch') = ?");
                month_bind = Some(m.to_string());
            }
        }
    }
    (sql, month_bind)
}

/// List a user's files in a given status (e.g. `trashed`), newest first.
pub async fn list_status(pool: &Db, user: &str, status: &str) -> Result<Vec<MediaFile>> {
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT * FROM media_file WHERE user = ? AND status = ? ORDER BY created_at DESC",
    )
    .bind(user)
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Update a file's review status (scoped to the owning user).
pub async fn set_status(
    pool: &Db,
    user: &str,
    id: i64,
    status: &str,
    trashed_at: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE media_file SET status = ?, trashed_at = ? WHERE id = ? AND user = ?")
        .bind(status)
        .bind(trashed_at)
        .bind(id)
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}

/// Flip a file's favorite flag (scoped to the owning user); returns the new value.
pub async fn toggle_favorite(pool: &Db, user: &str, id: i64) -> Result<bool> {
    sqlx::query(
        "UPDATE media_file SET is_favorite = CASE WHEN is_favorite = 1 THEN 0 ELSE 1 END \
         WHERE id = ? AND user = ?",
    )
    .bind(id)
    .bind(user)
    .execute(pool)
    .await?;
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT is_favorite FROM media_file WHERE id = ? AND user = ?")
            .bind(id)
            .bind(user)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v != 0).unwrap_or(false))
}

/// Number of users in the DB.
pub async fn count_users(pool: &Db) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

/// Insert or update a user's passcode hash and role.
pub async fn upsert_user(pool: &Db, username: &str, pass_hash: &str, role: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO user (username, pass_hash, role) VALUES (?, ?, ?) \
         ON CONFLICT(username) DO UPDATE SET pass_hash = excluded.pass_hash, role = excluded.role",
    )
    .bind(username)
    .bind(pass_hash)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

/// Look up a user: `(username, pass_hash, role)`.
pub async fn get_user(pool: &Db, username: &str) -> Result<Option<(String, String, String)>> {
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT username, pass_hash, role FROM user WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Aggregate counts for a user.
pub async fn stats_summary(pool: &Db, user: &str) -> Result<StatsSummary> {
    let r: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           COUNT(*), \
           COALESCE(SUM(status = 'pending'), 0), \
           COALESCE(SUM(status = 'keep'), 0), \
           COALESCE(SUM(status = 'trashed'), 0), \
           COALESCE(SUM(is_blurry), 0), \
           COALESCE(SUM(size_bytes), 0), \
           COALESCE(SUM(CASE WHEN status = 'trashed' THEN size_bytes ELSE 0 END), 0) \
         FROM media_file WHERE user = ?",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    Ok(StatsSummary {
        total: r.0,
        pending: r.1,
        keep: r.2,
        trashed: r.3,
        blurry: r.4,
        total_size: r.5,
        trashed_size: r.6,
    })
}

/// Per-month review progress, newest month first — the Clean tab's timeline
/// checklist. `reviewed` counts anything no longer pending (kept or trashed).
pub async fn timeline_months(pool: &Db, user: &str) -> Result<Vec<TimelineMonth>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT strftime('%Y-%m', created_at, 'unixepoch') AS sort_key, \
                COUNT(*), \
                COALESCE(SUM(status <> 'pending'), 0) \
         FROM media_file WHERE user = ? \
         GROUP BY sort_key ORDER BY sort_key DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(sort_key, total, reviewed)| TimelineMonth {
            month: month_label(&sort_key),
            sort_key,
            total,
            reviewed,
        })
        .collect())
}

/// `"2026-08"` → `"August 2026"`. Falls back to the raw key if unparseable.
fn month_label(sort_key: &str) -> String {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    match sort_key.split_once('-') {
        Some((year, mon)) => match mon.parse::<usize>() {
            Ok(m) if (1..=12).contains(&m) => format!("{} {}", NAMES[m - 1], year),
            _ => sort_key.to_string(),
        },
        None => sort_key.to_string(),
    }
}

// ── Albums ────────────────────────────────────────────────────────────────

/// All albums for a user, newest first.
pub async fn list_albums(pool: &Db, user: &str) -> Result<Vec<Album>> {
    let rows =
        sqlx::query_as::<_, Album>("SELECT * FROM album WHERE user = ? ORDER BY created_at DESC")
            .bind(user)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// Paginated files in an album (joined through album_file).
pub async fn album_files(
    pool: &Db,
    user: &str,
    album_id: i64,
    page: i64,
    size: i64,
) -> Result<Vec<MediaFile>> {
    let offset = (page.max(1) - 1) * size;
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT m.* FROM media_file m \
         JOIN album_file af ON af.file_id = m.id \
         WHERE af.album_id = ? AND m.user = ? \
         ORDER BY m.created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(album_id)
    .bind(user)
    .bind(size)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get-or-create an album by its unique key `(user, name, category_type)`.
/// Returns the album id.
pub async fn get_or_create_album(
    pool: &Db,
    user: &str,
    name: &str,
    category_type: &str,
    now: i64,
) -> Result<i64> {
    // Try existing first.
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM album WHERE user = ? AND name = ? AND category_type = ?")
            .bind(user)
            .bind(name)
            .bind(category_type)
            .fetch_optional(pool)
            .await?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let res = sqlx::query(
        "INSERT INTO album (user, name, category_type, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(user)
    .bind(name)
    .bind(category_type)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(res.last_insert_rowid())
}

/// Add a file to an album (ignored if already present).
pub async fn add_to_album(pool: &Db, album_id: i64, file_id: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO album_file (album_id, file_id) VALUES (?, ?)")
        .bind(album_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Recompute `file_count` and `cover_file_id` for an album.
pub async fn refresh_album_meta(pool: &Db, album_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE album SET \
           file_count = (SELECT COUNT(*) FROM album_file WHERE album_id = ?), \
           cover_file_id = (SELECT file_id FROM album_file WHERE album_id = ? LIMIT 1) \
         WHERE id = ?",
    )
    .bind(album_id)
    .bind(album_id)
    .bind(album_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Swipe log / streak ────────────────────────────────────────────────────

/// Record that the user made N swipes at timestamp `now`.
pub async fn log_swipes(pool: &Db, user: &str, count: usize, now: i64) -> Result<()> {
    for _ in 0..count {
        sqlx::query("INSERT INTO swipe_log (user, swiped_at) VALUES (?, ?)")
            .bind(user)
            .bind(now)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Remove the most recent swipe from the log — the counterpart to
/// `log_swipes` when a swipe is undone, so the streak doesn't count it.
pub async fn unlog_last_swipe(pool: &Db, user: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM swipe_log WHERE id = \
           (SELECT id FROM swipe_log WHERE user = ? ORDER BY swiped_at DESC, id DESC LIMIT 1)",
    )
    .bind(user)
    .execute(pool)
    .await?;
    Ok(())
}

/// Compute streak stats for a user.
/// `now_secs` = current unix timestamp (in seconds).
pub async fn streak_stats(
    pool: &Db,
    user: &str,
    now_secs: i64,
) -> Result<crate::models::StreakStats> {
    let today_start = now_secs - (now_secs % 86_400);

    // Swipes per calendar day (last 7).
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT (swiped_at / 86400) AS day_bucket, COUNT(*) \
         FROM swipe_log WHERE user = ? AND swiped_at >= ? \
         GROUP BY day_bucket ORDER BY day_bucket ASC",
    )
    .bind(user)
    .bind(today_start - 6 * 86_400)
    .fetch_all(pool)
    .await?;

    let today_bucket = today_start / 86_400;
    let mut last_7 = vec![0i64; 7];
    for (bucket, count) in &rows {
        let idx = (bucket - (today_bucket - 6)) as usize;
        if idx < 7 {
            last_7[idx] = *count;
        }
    }

    // Streak: walk backwards from today counting consecutive days with swipes.
    let active_days: std::collections::HashSet<i64> = sqlx::query_as::<_, (i64,)>(
        "SELECT DISTINCT (swiped_at / 86400) FROM swipe_log WHERE user = ?",
    )
    .bind(user)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(d,)| d)
    .collect();

    let mut streak = 0i64;
    let mut day = today_bucket;
    loop {
        if active_days.contains(&day) {
            streak += 1;
            day -= 1;
        } else {
            break;
        }
    }

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM swipe_log WHERE user = ?")
        .bind(user)
        .fetch_one(pool)
        .await?;

    Ok(crate::models::StreakStats {
        current_streak: streak,
        total_swipes: total.0,
        last_7_days: last_7,
    })
}

// ── Trash purge ───────────────────────────────────────────────────────────

/// Distinct users who currently have trashed files.
pub async fn users_with_trash(pool: &Db) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT user FROM media_file WHERE status = 'trashed'")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(u,)| u).collect())
}

/// All trashed files older than `older_than_secs` for a user.
pub async fn expired_trash(pool: &Db, user: &str, older_than_secs: i64) -> Result<Vec<MediaFile>> {
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        - older_than_secs;
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT * FROM media_file \
         WHERE user = ? AND status = 'trashed' AND trashed_at IS NOT NULL AND trashed_at < ?",
    )
    .bind(user)
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete a media_file row (hard delete; caller is responsible for the file on disk).
pub async fn delete_media_row(pool: &Db, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM album_file WHERE file_id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM media_file WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// All kept files for a user (for session/finish → move to clean/).
pub async fn kept_files(pool: &Db, user: &str) -> Result<Vec<MediaFile>> {
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT * FROM media_file WHERE user = ? AND status = 'keep'",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Update the path of a media file after it has been physically moved.
pub async fn update_path(pool: &Db, id: i64, new_path: &str, new_rel: &str) -> Result<()> {
    sqlx::query("UPDATE media_file SET path = ?, rel_path = ? WHERE id = ?")
        .bind(new_path)
        .bind(new_rel)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_and_inserts() {
        let dir = std::env::temp_dir().join("easyclean_db_test");
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("migrate_insert.db");
        let _ = std::fs::remove_file(&db_path);

        let pool = init(&db_path).await.expect("init + migrate");

        let id = insert_media(
            &pool,
            &NewMedia {
                user: "saiz".into(),
                path: "/media/saiz/source/a.jpg".into(),
                rel_path: Some("source/a.jpg".into()),
                filename: "a.jpg".into(),
                media_type: "image".into(),
                size_bytes: 1234,
                created_at: 1_700_000_000,
                gps_lat: None,
                gps_lon: None,
            },
        )
        .await
        .expect("insert");
        assert!(id > 0);
        assert_eq!(count_media(&pool, "saiz").await.unwrap(), 1);
        assert_eq!(count_media(&pool, "nobody").await.unwrap(), 0);

        let _ = std::fs::remove_file(&db_path);
    }
}

use std::collections::{HashMap, HashSet};
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
    // `ignore_missing` is what makes a rollback survivable.
    //
    // By default sqlx refuses to start when the database carries a migration
    // the binary doesn't know about — exactly the situation after deploying a
    // release that adds one and then rolling back. The old image would then
    // crash-loop instead of restoring service, defeating the auto-rollback in
    // `deploy.yml`. Migrations here are additive, so an older binary simply
    // ignoring a newer table is correct; it reads and writes the columns it
    // knows and leaves the rest alone.
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_ignore_missing(true);
    migrator.run(&pool).await?;
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

/// A review batch for one mode: `timeline` (optional `YYYY-MM` month),
/// `onthisday`, `recents`, `blurs` or `duplicates`. Only `pending` files are
/// returned, newest first.
/// What a Gallery or review request is asking for.
///
/// Grouped into one struct rather than a growing tail of `Option` arguments:
/// the binds have to line up with the `?`s in the generated SQL, and every new
/// positional parameter was one more chance to get that order wrong.
#[derive(Debug, Default, Clone)]
pub struct BatchQuery<'a> {
    /// `timeline` | `onthisday` | `recents` | `blurs` | `duplicates` (review
    /// decks, pending only) · `library` | `original` | `cleaned` (Gallery
    /// scopes).
    pub mode: &'a str,
    /// `YYYY-MM`, restricting to one month.
    pub month: Option<&'a str>,
    /// Free text matched against the filename and the path inside the library.
    pub q: Option<&'a str>,
    /// `image` or `video`.
    pub media_type: Option<&'a str>,
}

/// The `WHERE` clause for a batch, plus its binds in the order they appear.
struct Filter {
    sql: String,
    binds: Vec<String>,
}

/// Escape a user's text for a `LIKE` pattern.
///
/// Without this, typing `%` matches everything and `_` matches any character —
/// so a search for `IMG_01` would also hit `IMG-01`, and a lone `%` would look
/// like the filter silently did nothing.
fn like_pattern(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('%');
    for c in raw.chars() {
        if c == '%' || c == '_' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// How far back `recents` reaches: the last seven days, matching the
/// "This week" label the Clean tab puts on that card.
pub const RECENT_WINDOW_SECS: i64 = 7 * 86_400;

/// Shared `WHERE` clause for `get_batch`/`count_batch`.
///
/// Review modes (`timeline`, `onthisday`, `recents`, `blurs`, `duplicates`) are
/// scoped to `pending` — a swipe deck must not re-offer a photo you already
/// decided on.
///
/// The Gallery scopes are the exception; they show what you *have*, and map to
/// its All / Original / Cleaned tabs:
///   - `library`  — everything except the trash
///   - `original` — not reviewed yet, still in `source/`
///   - `cleaned`  — kept, filed into `clean/<album>/` at session finish
fn batch_filter(q: &BatchQuery<'_>) -> Filter {
    let mut sql = match q.mode {
        "library" => String::from("WHERE user = ? AND status <> 'trashed'"),
        "original" => String::from("WHERE user = ? AND status = 'pending'"),
        "cleaned" => String::from("WHERE user = ? AND status = 'keep'"),
        _ => String::from("WHERE user = ? AND status = 'pending'"),
    };
    let mut binds: Vec<String> = Vec::new();

    match q.mode {
        "blurs" => sql.push_str(" AND is_blurry = 1"),
        "duplicates" => sql.push_str(" AND group_id IS NOT NULL"),
        // The same calendar day in *any* year — that's what makes it a memory
        // rather than a second copy of Recents, which is what it was while
        // both modes fell through to the unfiltered `timeline` branch below.
        "onthisday" => sql
            .push_str(" AND strftime('%m-%d', created_at, 'unixepoch') = strftime('%m-%d', 'now')"),
        // Shot in the last week. `strftime('%s')` returns TEXT, and SQLite
        // sorts every integer below every string, so the CAST is what makes
        // this a date comparison instead of one that matches everything.
        "recents" => sql.push_str(&format!(
            " AND created_at >= CAST(strftime('%s', 'now') AS INTEGER) - {RECENT_WINDOW_SECS}"
        )),
        _ => {
            if let Some(m) = q.month {
                sql.push_str(" AND strftime('%Y-%m', created_at, 'unixepoch') = ?");
                binds.push(m.to_string());
            }
        }
    }

    if let Some(t) = q.media_type.filter(|t| !t.is_empty()) {
        sql.push_str(" AND media_type = ?");
        binds.push(t.to_string());
    }

    // Searching `rel_path` as well as the filename means a query can name the
    // album a photo was filed into, not just the file itself.
    if let Some(text) = q.q.map(str::trim).filter(|t| !t.is_empty()) {
        sql.push_str(
            " AND (filename LIKE ? ESCAPE '\\' OR COALESCE(rel_path, '') LIKE ? ESCAPE '\\')",
        );
        let pat = like_pattern(text);
        binds.push(pat.clone());
        binds.push(pat);
    }

    Filter { sql, binds }
}

/// One page of a batch. `page` is 1-based; the Gallery walks it to load a
/// library far larger than any single response should carry.
pub async fn get_batch(
    pool: &Db,
    user: &str,
    query: &BatchQuery<'_>,
    size: i64,
    page: i64,
) -> Result<Vec<MediaFile>> {
    let f = batch_filter(query);
    // `id` breaks ties: two photos sharing a timestamp would otherwise be free
    // to swap places between pages, so one could appear twice and another not
    // at all while paging.
    let sql = format!(
        "SELECT * FROM media_file {} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        f.sql
    );

    let mut stmt = sqlx::query_as::<_, MediaFile>(&sql).bind(user);
    for b in &f.binds {
        stmt = stmt.bind(b);
    }
    stmt = stmt.bind(size).bind((page.max(1) - 1) * size);
    Ok(stmt.fetch_all(pool).await?)
}

/// How many files match a batch, ignoring the page size — the denominator the
/// swipe screen's progress counter and the Gallery's result count need.
pub async fn count_batch(pool: &Db, user: &str, query: &BatchQuery<'_>) -> Result<i64> {
    let f = batch_filter(query);
    let sql = format!("SELECT COUNT(*) FROM media_file {}", f.sql);

    let mut stmt = sqlx::query_scalar::<_, i64>(&sql).bind(user);
    for b in &f.binds {
        stmt = stmt.bind(b);
    }
    Ok(stmt.fetch_one(pool).await?)
}

/// Every un-reviewed file that belongs to a duplicate cluster, ordered so a
/// group's members arrive together.
///
/// The compare screen needs whole groups, not a flat deck: deciding between two
/// near-identical photos means seeing both at once, and a paged batch can't
/// promise that the other copy is on the same page. Groups are ordered by their
/// newest member so the most recent pile-ups come first, and members within a
/// group oldest-first — the original before the copies of it.
pub async fn duplicate_clusters(pool: &Db, user: &str) -> Result<Vec<MediaFile>> {
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT m.* FROM media_file m \
         JOIN (SELECT group_id, MAX(created_at) AS newest FROM media_file \
               WHERE user = ? AND status = 'pending' AND group_id IS NOT NULL \
               GROUP BY group_id HAVING COUNT(*) > 1) g \
           ON g.group_id = m.group_id \
         WHERE m.user = ? AND m.status = 'pending' AND m.group_id IS NOT NULL \
         ORDER BY g.newest DESC, m.group_id, m.created_at ASC, m.id ASC",
    )
    .bind(user)
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List a user's files in a given status (e.g. `trashed`), newest first.
///
/// Ordered by *when it was binned*, falling back to capture date. The trash is
/// read as a history of what you just deleted, so a photo from 2019 deleted a
/// minute ago belongs at the top; `created_at` alone buried it.
pub async fn list_status(pool: &Db, user: &str, status: &str) -> Result<Vec<MediaFile>> {
    let rows = sqlx::query_as::<_, MediaFile>(
        "SELECT * FROM media_file WHERE user = ? AND status = ? \
         ORDER BY COALESCE(trashed_at, created_at) DESC, id DESC",
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

/// Move several files to one status at once; returns how many actually moved.
///
/// One statement rather than a loop of [`set_status`]: resolving a duplicate
/// group decides every copy at once, so the update that carries it out should
/// be one round trip rather than one per file.
pub async fn set_status_many(
    pool: &Db,
    user: &str,
    ids: &[i64],
    status: &str,
    trashed_at: Option<i64>,
) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let holes = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "UPDATE media_file SET status = ?, trashed_at = ? WHERE user = ? AND id IN ({holes})"
    );
    let mut q = sqlx::query(&sql).bind(status).bind(trashed_at).bind(user);
    for id in ids {
        q = q.bind(id);
    }
    Ok(q.execute(pool).await?.rows_affected())
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

/// Take one file out of an album. No-op if it was never in it.
pub async fn remove_from_album(pool: &Db, album_id: i64, file_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM album_file WHERE album_id = ? AND file_id = ?")
        .bind(album_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// One album by id, scoped to its owner.
pub async fn get_album(pool: &Db, user: &str, album_id: i64) -> Result<Option<Album>> {
    let row = sqlx::query_as::<_, Album>("SELECT * FROM album WHERE id = ? AND user = ?")
        .bind(album_id)
        .bind(user)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Every album one file belongs to — what the viewer's Details panel lists.
pub async fn albums_for_file(pool: &Db, user: &str, file_id: i64) -> Result<Vec<Album>> {
    let rows = sqlx::query_as::<_, Album>(
        "SELECT a.* FROM album a \
         JOIN album_file af ON af.album_id = a.id \
         WHERE af.file_id = ? AND a.user = ? \
         ORDER BY a.category_type, a.name",
    )
    .bind(file_id)
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
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
    // The embedding is derived data keyed by file id; leaving it behind would
    // both leak rows and let a recycled id inherit a stranger's vector.
    sqlx::query("DELETE FROM media_embedding WHERE file_id = ?")
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

// ── User settings ─────────────────────────────────────────────────────────

/// Read one setting. `None` means the user never set it — callers fall back to
/// the `EASYCLEAN_*` config default rather than inventing one here.
pub async fn get_setting(pool: &Db, user: &str, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM user_setting WHERE user = ? AND key = ?")
            .bind(user)
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// Write one setting, replacing any previous value.
pub async fn set_setting(pool: &Db, user: &str, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_setting (user, key, value) VALUES (?, ?, ?) \
         ON CONFLICT(user, key) DO UPDATE SET value = excluded.value",
    )
    .bind(user)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop every duplicate-group assignment for a user. Run before re-grouping at
/// a new sensitivity, or files would keep stale group ids from the old pass.
pub async fn clear_groups(pool: &Db, user: &str) -> Result<()> {
    sqlx::query("UPDATE media_file SET group_id = NULL WHERE user = ?")
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}

/// Bytes sent to the trash per calendar day over the last `days` days, oldest
/// first, with empty days zero-filled so the caller can render a bar per day.
///
/// Reads `trashed_at`, so it only covers files still in the trash: once
/// retention purges a row the history goes with it. That keeps the series
/// honest — it is what the trash currently holds, by the day it was binned.
pub async fn storage_by_day(
    pool: &Db,
    user: &str,
    days: i64,
    now_secs: i64,
) -> Result<Vec<crate::models::StorageDay>> {
    let days = days.clamp(1, 365);
    let today_bucket = now_secs / 86_400;
    let first_bucket = today_bucket - (days - 1);

    let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT (trashed_at / 86400) AS day_bucket, \
                COALESCE(SUM(size_bytes), 0), \
                COUNT(*) \
         FROM media_file \
         WHERE user = ? AND status = 'trashed' AND trashed_at IS NOT NULL \
           AND (trashed_at / 86400) >= ? \
         GROUP BY day_bucket ORDER BY day_bucket ASC",
    )
    .bind(user)
    .bind(first_bucket)
    .fetch_all(pool)
    .await?;

    let mut series: Vec<crate::models::StorageDay> = (0..days)
        .map(|i| crate::models::StorageDay {
            day: (first_bucket + i) * 86_400,
            bytes: 0,
            files: 0,
        })
        .collect();
    for (bucket, bytes, files) in rows {
        let idx = (bucket - first_bucket) as usize;
        if let Some(slot) = series.get_mut(idx) {
            slot.bytes = bytes;
            slot.files = files;
        }
    }
    Ok(series)
}

// ── Embeddings (semantic search) ──────────────────────────────────────────

/// Store (or replace) one file's embedding.
pub async fn upsert_embedding(
    pool: &Db,
    file_id: i64,
    model: &str,
    dim: i64,
    scale: f64,
    vec: &[u8],
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO media_embedding (file_id, model, dim, scale, vec, created_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(file_id) DO UPDATE SET \
           model = excluded.model, dim = excluded.dim, scale = excluded.scale, \
           vec = excluded.vec, created_at = excluded.created_at",
    )
    .bind(file_id)
    .bind(model)
    .bind(dim)
    .bind(scale)
    .bind(vec)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop one file's embedding — used when the file itself is purged.
pub async fn delete_embedding(pool: &Db, file_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM media_embedding WHERE file_id = ?")
        .bind(file_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Files still needing an embedding for `model`: `(id, thumb_path, path)`.
///
/// Only images, and only ones that already have a thumbnail — the embedding is
/// taken from that cached 224-ish render rather than by decoding the original
/// again. Anything not yet thumbnailed is picked up on the next pass, after
/// detection has produced one.
pub async fn files_needing_embedding(
    pool: &Db,
    user: &str,
    model: &str,
) -> Result<Vec<(i64, String)>> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT m.id, m.thumb_path FROM media_file m \
         LEFT JOIN media_embedding e ON e.file_id = m.id AND e.model = ? \
         WHERE m.user = ? AND m.status <> 'trashed' AND m.media_type = 'image' \
           AND m.thumb_path IS NOT NULL AND e.file_id IS NULL \
         ORDER BY m.created_at DESC",
    )
    .bind(model)
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// How many of a user's searchable files carry an embedding, and how many could.
pub async fn embedding_stats(pool: &Db, user: &str, model: &str) -> Result<(i64, i64)> {
    let indexed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM media_embedding e JOIN media_file m ON m.id = e.file_id \
         WHERE m.user = ? AND m.status <> 'trashed' AND e.model = ?",
    )
    .bind(user)
    .bind(model)
    .fetch_one(pool)
    .await?;
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM media_file \
         WHERE user = ? AND status <> 'trashed' AND media_type = 'image' \
           AND thumb_path IS NOT NULL",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    Ok((indexed.0, total.0))
}

/// Every embedding for a user, as `(file_id, scale, int8 vector)`.
///
/// Deliberately the whole set: an exact scan of 100K vectors costs about 15ms,
/// less than the text encoder spends on the query, so there is nothing for an
/// approximate index to buy at this scale.
pub async fn all_embeddings(
    pool: &Db,
    user: &str,
    model: &str,
) -> Result<Vec<(i64, f64, Vec<u8>)>> {
    let rows = sqlx::query_as::<_, (i64, f64, Vec<u8>)>(
        "SELECT e.file_id, e.scale, e.vec FROM media_embedding e \
         JOIN media_file m ON m.id = e.file_id \
         WHERE m.user = ? AND m.status <> 'trashed' AND e.model = ?",
    )
    .bind(user)
    .bind(model)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch specific files by id, preserving the caller's ordering.
///
/// Search ranks by similarity, and `WHERE id IN (...)` would hand them back in
/// whatever order SQLite likes — so the ranking is re-applied here.
pub async fn media_by_ids(pool: &Db, user: &str, ids: &[i64]) -> Result<Vec<MediaFile>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let places = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT * FROM media_file WHERE user = ? AND id IN ({places})");
    let mut q = sqlx::query_as::<_, MediaFile>(&sql).bind(user);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(pool).await?;

    let mut by_id: HashMap<i64, MediaFile> = rows.into_iter().map(|m| (m.id, m)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database carrying a migration this binary has never heard of must
    /// still open. That is the rollback case: deploy a release that adds a
    /// migration, roll back, and the older image meets a newer database. sqlx
    /// aborts on that by default, which would turn an auto-rollback into a
    /// crash loop instead of a recovery.
    #[tokio::test]
    async fn opens_a_database_from_a_newer_version() {
        let dir = std::env::temp_dir().join("easyclean_db_test");
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("future_migration.db");
        let _ = std::fs::remove_file(&db_path);

        let pool = init(&db_path).await.expect("first open");

        // Forge a migration from a future release: a higher version than any
        // file in ./migrations, recorded as applied.
        let next: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("read applied migrations");
        sqlx::query(
            "INSERT INTO _sqlx_migrations \
             (version, description, installed_on, success, checksum, execution_time) \
             VALUES (?, 'from the future', CURRENT_TIMESTAMP, 1, X'00', 0)",
        )
        .bind(next)
        .execute(&pool)
        .await
        .expect("record future migration");
        pool.close().await;

        // Re-opening is what an older binary does on rollback.
        let reopened = init(&db_path)
            .await
            .expect("a database from a newer release must still open, or rollback breaks");
        assert_eq!(count_media(&reopened, "nobody").await.unwrap(), 0);
        reopened.close().await;

        let _ = std::fs::remove_file(&db_path);
    }

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

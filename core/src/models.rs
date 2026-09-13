use serde::{Deserialize, Serialize};

/// Review lifecycle of a media file.
pub mod status {
    pub const PENDING: &str = "pending";
    pub const KEEP: &str = "keep";
    pub const TRASHED: &str = "trashed";
}

/// A single indexed photo or video. Timestamps are unix epoch seconds.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MediaFile {
    pub id: i64,
    pub user: String,
    pub path: String,
    pub rel_path: Option<String>,
    pub filename: String,
    pub media_type: String, // "image" | "video"
    pub size_bytes: i64,
    pub created_at: i64,
    pub phash: Option<String>,
    pub blur_score: Option<f64>,
    pub is_blurry: bool,
    pub status: String,
    pub trashed_at: Option<i64>,
    pub group_id: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub is_favorite: bool,
    pub thumb_path: Option<String>,
}

/// Fields needed to register a freshly-scanned file.
#[derive(Debug, Clone)]
pub struct NewMedia {
    pub user: String,
    pub path: String,
    pub rel_path: Option<String>,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub created_at: i64,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Album {
    pub id: i64,
    pub user: String,
    pub name: String,
    pub category_type: String,
    pub cover_file_id: Option<i64>,
    pub file_count: i64,
    pub created_at: i64,
}

/// A row in the album_file join table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlbumFile {
    pub album_id: i64,
    pub file_id: i64,
}

/// Stats for the streak widget on the Clean screen.
#[derive(Debug, Default, Clone, Serialize)]
pub struct StreakStats {
    /// Consecutive calendar days (including today) with at least one swipe.
    pub current_streak: i64,
    /// Total swipes ever recorded for the user.
    pub total_swipes: i64,
    /// Swipe counts per day for the last 7 days (index 0 = 7 days ago, 6 = today).
    pub last_7_days: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub role: String, // "admin" | "user"
}

/// Result of a scan pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanStats {
    pub scanned: usize,
    pub added: usize,
    pub skipped: usize,
}

/// Result of a detection pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct DetectStats {
    pub processed: usize,
    pub blurry: usize,
    pub duplicate_groups: usize,
    pub thumbs: usize,
}

/// One row of the Clean tab's timeline checklist.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineMonth {
    /// Display label, e.g. `"August 2026"`.
    pub month: String,
    /// Sortable/filterable key, e.g. `"2026-08"`.
    pub sort_key: String,
    pub total: i64,
    pub reviewed: i64,
}

/// One day of the Settings screen's "Storage Saved" chart.
#[derive(Debug, Default, Clone, Serialize)]
pub struct StorageDay {
    /// Unix seconds at the start of the day (UTC).
    pub day: i64,
    /// Bytes of media binned that day.
    pub bytes: i64,
    /// How many files that was.
    pub files: i64,
}

/// Aggregate counts for the stats screen.
#[derive(Debug, Default, Clone, Serialize)]
pub struct StatsSummary {
    pub total: i64,
    pub pending: i64,
    pub keep: i64,
    pub trashed: i64,
    pub blurry: i64,
    pub total_size: i64,
    pub trashed_size: i64,
}

use chrono::{DateTime, Utc};

use crate::models::MediaFile;

/// All category-type keys used in `album.category_type`.
pub mod cat {
    /// No longer produced — kept so the cleanup migration and any old row that
    /// predates it still have a name for what they are removing.
    pub const MONTH: &str = "month";
    pub const YEAR: &str = "year";
    pub const VIDEOS: &str = "videos";
    pub const SCREENSHOTS: &str = "screenshots";
    pub const FAVORITES: &str = "favorites";
    pub const PLACE: &str = "place";
}

/// Return every category key that `mf` belongs to.
/// e.g. `["year:2026", "type:videos"]`
///
/// Years only, not months. Emitting both filed every photo twice and left
/// Memories showing "Nov 2023" next to "2023" — the same pictures under two
/// headings, which is noise rather than two ways in. The Clean tab's month
/// checklist is unaffected: it queries `created_at` directly and never reads
/// albums.
pub fn categories_for(mf: &MediaFile) -> Vec<String> {
    let mut cats: Vec<String> = Vec::new();

    let dt = DateTime::<Utc>::from_timestamp(mf.created_at, 0).unwrap_or(DateTime::UNIX_EPOCH);

    cats.push(format!("year:{}", dt.format("%Y")));

    if mf.media_type == "video" {
        cats.push(format!("{}:videos", cat::VIDEOS));
    }

    if mf.is_favorite {
        cats.push(format!("{}:favorites", cat::FAVORITES));
    }

    if is_screenshot(mf) {
        cats.push(format!("{}:screenshots", cat::SCREENSHOTS));
    }

    cats
}

/// Heuristic: landscape-cropped or very-wide-portrait images are likely screenshots.
/// Uses aspect-ratio detection only (no EXIF software tag access here).
fn is_screenshot(mf: &MediaFile) -> bool {
    // Screenshots typically have names starting with "Screenshot" or similar patterns.
    let name = mf.filename.to_lowercase();
    name.starts_with("screenshot")
        || name.starts_with("screen_")
        || name.starts_with("scr_")
        || name.contains("screenshot")
}

/// Human-readable album name for a category key.
/// e.g. `"month:2026-05"` → `"May 2026"`
pub fn album_name(cat_key: &str) -> String {
    if let Some(ym) = cat_key.strip_prefix("month:") {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(&format!("{ym}-01"), "%Y-%m-%d") {
            return format!("{}", dt.format("%b %Y"));
        }
    }
    if let Some(y) = cat_key.strip_prefix("year:") {
        return y.to_string();
    }
    // "videos:videos" → "Videos" etc.
    if let Some(label) = cat_key.split(':').nth(1) {
        let mut s = label.to_string();
        if let Some(c) = s.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        return s;
    }
    cat_key.to_string()
}

/// Derive the `category_type` field from a full category key.
pub fn cat_type(cat_key: &str) -> &str {
    cat_key.split(':').next().unwrap_or(cat_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mf(filename: &str, created_at: i64, media_type: &str) -> MediaFile {
        MediaFile {
            id: 1,
            user: "u".into(),
            path: filename.into(),
            rel_path: None,
            filename: filename.into(),
            media_type: media_type.into(),
            size_bytes: 0,
            created_at,
            phash: None,
            blur_score: None,
            is_blurry: false,
            status: "keep".into(),
            trashed_at: None,
            group_id: None,
            gps_lat: None,
            gps_lon: None,
            is_favorite: false,
            thumb_path: None,
        }
    }

    #[test]
    fn image_gets_a_year_and_no_month() {
        let cats = categories_for(&mf("photo.jpg", 1_746_057_600, "image")); // 2025-05-01
        assert!(cats.iter().any(|c| c.starts_with("year:")));
        assert!(
            !cats.iter().any(|c| c.starts_with("month:")),
            "months duplicate the year album and clutter Memories"
        );
        assert!(!cats.iter().any(|c| c.contains("videos")));
    }

    #[test]
    fn video_gets_videos_category() {
        let cats = categories_for(&mf("clip.mp4", 1_746_057_600, "video"));
        assert!(cats.iter().any(|c| c.contains("videos")));
    }

    #[test]
    fn screenshot_detected() {
        let cats = categories_for(&mf("Screenshot 2026-01.png", 1_746_057_600, "image"));
        assert!(cats.iter().any(|c| c.contains("screenshots")));
    }

    #[test]
    fn album_name_formats_month() {
        assert_eq!(album_name("month:2026-05"), "May 2026");
        assert_eq!(album_name("year:2026"), "2026");
        assert_eq!(album_name("videos:videos"), "Videos");
    }
}

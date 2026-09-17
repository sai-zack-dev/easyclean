use std::path::Path;
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

use crate::db::{self, Db};
use crate::error::Result;
use crate::models::{NewMedia, ScanStats};

pub const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "heic", "heif",
];
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv", "m4v", "3gp", "webm"];

fn classify(ext: &str) -> Option<&'static str> {
    if IMAGE_EXTS.contains(&ext) {
        Some("image")
    } else if VIDEO_EXTS.contains(&ext) {
        Some("video")
    } else {
        None
    }
}

/// Walk `dir` recursively, indexing any new image/video files for `user`.
/// `progress` is called with the running count of newly-added files.
pub async fn scan_folder(
    pool: &Db,
    user: &str,
    dir: &Path,
    mut progress: impl FnMut(usize),
) -> Result<ScanStats> {
    let existing = db::existing_paths(pool, user).await?;
    let mut stats = ScanStats::default();

    // Don't descend into our own managed folders.
    //
    // `.vault` matters most: a hidden file physically lives there, and walking
    // it would re-index the photo as a brand new `pending` one — putting it
    // straight back into the Clean tab for anyone to see.
    let walker = WalkDir::new(dir).into_iter().filter_entry(|e| {
        !(e.file_type().is_dir()
            && matches!(
                e.file_name().to_str(),
                Some("clean") | Some(".trash") | Some(".vault")
            ))
    });
    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        let media_type = match ext.as_deref().and_then(classify) {
            Some(m) => m,
            None => continue,
        };
        stats.scanned += 1;

        let path_str = path.to_string_lossy().to_string();
        if existing.contains(&path_str) {
            stats.skipped += 1;
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        db::insert_media(
            pool,
            &describe(user, path, dir, media_type, size, mtime, None),
        )
        .await?;
        stats.added += 1;
        progress(stats.added);
    }
    Ok(stats)
}

/// Index one already-placed file, without walking anything.
///
/// The upload path's counterpart to [`scan_folder`]: the bytes are already
/// verified and moved into `source/`, and re-walking the whole directory to
/// notice a single new file would cost the entire library per photo received.
///
/// Returns `None` for an extension this app does not handle, so a caller that
/// somehow lands a `.txt` in `source/` gets a clear answer instead of a row
/// nothing can render. `content_hash` is what the server computed while
/// receiving the file — never what the client claimed.
pub async fn index_file(
    pool: &Db,
    user: &str,
    path: &Path,
    base_dir: &Path,
    content_hash: Option<String>,
) -> Result<Option<i64>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let Some(media_type) = ext.as_deref().and_then(classify) else {
        return Ok(None);
    };
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let id = db::insert_media(
        pool,
        &describe(
            user,
            path,
            base_dir,
            media_type,
            meta.len() as i64,
            mtime,
            content_hash,
        ),
    )
    .await?;
    Ok(Some(id))
}

/// Build the row for one file: EXIF date and GPS where available, filesystem
/// mtime where not.
///
/// Shared by the scanner and the uploader so a photo's recorded date does not
/// depend on which door it came in through.
fn describe(
    user: &str,
    path: &Path,
    base_dir: &Path,
    media_type: &str,
    size: i64,
    mtime: i64,
    content_hash: Option<String>,
) -> NewMedia {
    let (exif_ts, lat, lon) = read_exif(path);
    NewMedia {
        user: user.to_string(),
        path: path.to_string_lossy().to_string(),
        rel_path: path
            .strip_prefix(base_dir)
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        filename: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string(),
        media_type: media_type.to_string(),
        size_bytes: size,
        created_at: exif_ts.unwrap_or(mtime),
        gps_lat: lat,
        gps_lon: lon,
        content_hash,
    }
}

/// Best-effort EXIF read: `(epoch_seconds, gps_lat, gps_lon)`. Any failure ⇒ `None`s.
fn read_exif(path: &Path) -> (Option<i64>, Option<f64>, Option<f64>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None, None),
    };
    let mut buf = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut buf) {
        Ok(e) => e,
        Err(_) => return (None, None, None),
    };

    let ts = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .and_then(parse_datetime);
    let lat = gps_coord(
        &exif,
        exif::Tag::GPSLatitude,
        exif::Tag::GPSLatitudeRef,
        b'S',
    );
    let lon = gps_coord(
        &exif,
        exif::Tag::GPSLongitude,
        exif::Tag::GPSLongitudeRef,
        b'W',
    );
    (ts, lat, lon)
}

fn parse_datetime(f: &exif::Field) -> Option<i64> {
    if let exif::Value::Ascii(ref v) = f.value {
        let bytes = v.first()?;
        let s = std::str::from_utf8(bytes).ok()?;
        let dt = chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y:%m:%d %H:%M:%S").ok()?;
        return Some(dt.and_utc().timestamp());
    }
    None
}

fn gps_coord(exif: &exif::Exif, coord: exif::Tag, refr: exif::Tag, neg: u8) -> Option<f64> {
    let cf = exif.get_field(coord, exif::In::PRIMARY)?;
    let mut deg = if let exif::Value::Rational(ref r) = cf.value {
        if r.len() < 3 {
            return None;
        }
        let d = r[0].num as f64 / r[0].denom as f64;
        let m = r[1].num as f64 / r[1].denom as f64;
        let s = r[2].num as f64 / r[2].denom as f64;
        d + m / 60.0 + s / 3600.0
    } else {
        return None;
    };
    if let Some(rf) = exif.get_field(refr, exif::In::PRIMARY) {
        if let exif::Value::Ascii(ref v) = rf.value {
            if v.first().and_then(|b| b.first()).copied() == Some(neg) {
                deg = -deg;
            }
        }
    }
    Some(deg)
}

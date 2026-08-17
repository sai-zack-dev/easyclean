use std::path::Path;
use std::process::{Command, Stdio};

use image::DynamicImage;

use crate::error::Result;

/// Longest edge of a generated thumbnail, in pixels.
pub const THUMB_MAX: u32 = 480;

/// Write a JPEG thumbnail of an already-decoded image to `out` (`.jpg`).
pub fn make_image_thumb(img: &DynamicImage, out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    img.thumbnail(THUMB_MAX, THUMB_MAX).to_rgb8().save(out)?;
    Ok(())
}

/// Extract a first-frame JPEG thumbnail from a video via the `ffmpeg` CLI.
/// Returns `Ok(false)` if ffmpeg is unavailable or fails (video stays indexed,
/// just without a thumbnail).
pub fn ensure_video_thumb(src: &str, out: &Path) -> Result<bool> {
    if out.exists() {
        return Ok(true);
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let out_str = match out.to_str() {
        Some(s) => s,
        None => return Ok(false),
    };
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            src,
            "-frames:v",
            "1",
            "-vf",
            "scale=480:-1",
            out_str,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Ok(matches!(status, Ok(s) if s.success()))
}

use std::path::Path;
use std::process::{Command, Stdio};

use image::DynamicImage;

use crate::error::Result;

/// Longest edge of a generated thumbnail, in pixels. Grid-cell sized.
pub const THUMB_MAX: u32 = 480;

/// Longest edge of a generated *preview*, in pixels.
///
/// Thumbnails are far too soft to judge a photo by — which is the entire point
/// of the swipe deck and the detail viewer. 1600px covers a full-screen card on
/// a 2x phone and a desktop viewport without shipping the original (often a
/// 4-12 MB HEIC the browser cannot even decode).
pub const PREVIEW_MAX: u32 = 1600;

/// JPEG quality for previews. High enough that compression artefacts are not
/// mistaken for a soft shot when deciding keep-or-delete.
const PREVIEW_QUALITY: u8 = 88;

/// Write a JPEG thumbnail of an already-decoded image to `out` (`.jpg`).
pub fn make_image_thumb(img: &DynamicImage, out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    img.thumbnail(THUMB_MAX, THUMB_MAX).to_rgb8().save(out)?;
    Ok(())
}

/// Write a JPEG preview of an already-decoded image to `out` (`.jpg`).
///
/// Only downscales: an image already smaller than [`PREVIEW_MAX`] is re-encoded
/// at its own size rather than being blown up.
pub fn make_image_preview(img: &DynamicImage, out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (w, h) = (img.width(), img.height());
    let scaled = if w.max(h) > PREVIEW_MAX {
        img.thumbnail(PREVIEW_MAX, PREVIEW_MAX)
    } else {
        img.clone()
    };
    // Written via the encoder rather than `save()` so the quality is ours to
    // choose; `save()` bakes in the crate default (75), which is visibly soft.
    let rgb = scaled.to_rgb8();
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, PREVIEW_QUALITY)
        .encode_image(&DynamicImage::ImageRgb8(rgb))?;
    Ok(())
}

/// Decode `src` and cache a preview at `out`, reusing an existing one.
/// Returns `false` when the file could not be decoded (corrupt, or HEIC in a
/// build without libheif).
pub fn ensure_image_preview(src: &Path, out: &Path) -> bool {
    if out.exists() {
        return true;
    }
    match crate::decode::open_image(src) {
        Ok(img) => make_image_preview(&img, out).is_ok(),
        Err(_) => false,
    }
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

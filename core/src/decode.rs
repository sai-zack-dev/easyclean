//! One place where pixels enter the system.
//!
//! Everything that needs decoded pixels (blur score, perceptual hash,
//! thumbnails) goes through [`open_image`], so format support and EXIF
//! orientation are handled once rather than per call site.
//!
//! HEIC/HEIF lives behind the `heic` cargo feature because it links the system
//! `libheif`. With the feature on, [`init_decoders`] registers libheif as an
//! `image`-crate decoding hook and `image::open` transparently handles `.heic`.

use std::path::Path;
use std::sync::Once;

use image::metadata::Orientation;
use image::DynamicImage;

use crate::error::Result;

static INIT: Once = Once::new();

/// Register optional decoders. Idempotent; cheap to call on every decode.
pub fn init_decoders() {
    INIT.call_once(|| {
        #[cfg(feature = "heic")]
        {
            libheif_rs::integration::image::register_all_decoding_hooks();
            tracing::info!("HEIF/HEIC decoding enabled (libheif)");
        }
        #[cfg(not(feature = "heic"))]
        tracing::debug!("built without the `heic` feature — .heic files are indexed, not decoded");
    });
}

/// True when this build can decode HEIC/HEIF.
pub const fn heic_supported() -> bool {
    cfg!(feature = "heic")
}

/// Decode an image from disk with its EXIF orientation already applied, so a
/// phone photo shot in portrait doesn't come out sideways in the thumbnail.
pub fn open_image(path: &Path) -> Result<DynamicImage> {
    init_decoders();
    let mut img = image::open(path)?;
    if let Some(orientation) = exif_orientation(path) {
        img.apply_orientation(orientation);
    }
    Ok(img)
}

/// Best-effort EXIF orientation. `None` when absent, unreadable, or already
/// upright — callers then leave the pixels alone.
pub fn exif_orientation(path: &Path) -> Option<Orientation> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut buf).ok()?;
    let raw = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
        .value
        .get_uint(0)?;
    match Orientation::from_exif(u8::try_from(raw).ok()?) {
        Some(Orientation::NoTransforms) | None => None,
        Some(o) => Some(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// A 4×2 image rotated by EXIF tag 6 must come back 2×4.
    #[test]
    fn orientation_is_applied_when_present() {
        let mut img = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 2, Rgb([1, 2, 3])));
        assert_eq!((img.width(), img.height()), (4, 2));
        img.apply_orientation(Orientation::from_exif(6).unwrap());
        assert_eq!((img.width(), img.height()), (2, 4));
    }

    /// Orientation 1 means "upright" — we must not spend work on a no-op.
    #[test]
    fn upright_orientation_is_ignored() {
        assert_eq!(Orientation::from_exif(1), Some(Orientation::NoTransforms));
    }

    #[test]
    fn plain_image_without_exif_decodes() {
        let dir = std::env::temp_dir().join(format!("ec-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plain.png");
        RgbImage::from_pixel(8, 4, Rgb([9, 9, 9])).save(&p).unwrap();

        let img = open_image(&p).unwrap();
        assert_eq!((img.width(), img.height()), (8, 4));
        assert!(exif_orientation(&p).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

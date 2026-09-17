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

/// Largest edge this will decode, in pixels.
///
/// Well past any real camera — a 100-megapixel medium-format frame is about
/// 12000 across — and below the point where the dimensions alone are evidence
/// the file is not a photograph.
const MAX_EDGE: u32 = 65_535;

/// Bounds for decoding a file we did not create.
///
/// `image` already refuses to allocate more than 512 MiB for one decode, and
/// applies it to anything implementing `ImageDecoder` — libheif's hook
/// included, so HEIC is covered by the same check. That default is doing the
/// real work here; what this adds is to say so on purpose rather than inherit
/// it, and to bound the declared dimensions as well as the allocation.
///
/// The allocation cap is deliberately left at 512 MiB: a 48-megapixel phone
/// photo needs about 144 MB decoded, so anything much tighter would start
/// refusing real photographs. Total memory is bounded at the container instead,
/// where it also covers what libheif and ffmpeg allocate out of Rust's reach.
fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits
}

/// Decode an image from disk with its EXIF orientation already applied, so a
/// phone photo shot in portrait doesn't come out sideways in the thumbnail.
///
/// Spelled out rather than `image::open`, which is the same thing with the
/// default limits; going through the reader is what makes room for
/// [`decode_limits`].
pub fn open_image(path: &Path) -> Result<DynamicImage> {
    init_decoders();
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(decode_limits());
    let mut img = reader.decode()?;
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

    /// The bounds we ask for, stated so a change to them is deliberate.
    ///
    /// The allocation cap is the part that actually stops a decompression bomb;
    /// it comes from `image`'s own default, so this asserts we did not drop it
    /// while adding the dimension bounds.
    #[test]
    fn our_limits_bound_dimensions_and_keep_the_allocation_cap() {
        let l = decode_limits();
        assert_eq!(l.max_image_width, Some(MAX_EDGE));
        assert_eq!(l.max_image_height, Some(MAX_EDGE));
        assert!(
            l.max_alloc.is_some_and(|b| b > 0),
            "the allocation cap is what refuses an image that decodes to more memory than we have"
        );
    }

    /// Limits set on the reader are honoured on this code path.
    ///
    /// `open_image` used to be `image::open`, which applies the defaults and
    /// gives no way to say otherwise. If it ever goes back to that, the bounds
    /// above become decoration — so this checks the mechanism rather than the
    /// numbers: a cap far below what the file needs must refuse it.
    #[test]
    fn a_decode_over_the_allocation_cap_is_refused() {
        let dir = std::env::temp_dir().join(format!("ec-limits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.png");
        // 64×64 RGB is ~12 KB decoded; the cap below is 1 KB.
        RgbImage::from_pixel(64, 64, Rgb([7, 7, 7]))
            .save(&p)
            .unwrap();

        let mut reader = image::ImageReader::open(&p)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        let mut tiny = image::Limits::default();
        tiny.max_alloc = Some(1024);
        reader.limits(tiny);
        assert!(
            reader.decode().is_err(),
            "the reader ignored the limits it was given"
        );

        // The same file decodes fine under the limits we actually ship.
        assert!(open_image(&p).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

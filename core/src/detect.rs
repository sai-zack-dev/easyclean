use std::collections::HashMap;
use std::path::{Path, PathBuf};

use futures_util::stream::{self, StreamExt};
use image::{imageops::FilterType, DynamicImage, GrayImage};

use crate::db::{self, Db};
use crate::error::Result;
use crate::models::DetectStats;
use crate::thumbs;

/// Per-image analysis result.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub blur_score: f64,
    pub is_blurry: bool,
    pub phash: String,
    pub thumb_made: bool,
}

/// How many files to analyze at once. Decoding is CPU-bound, so one in flight
/// per core; a floor of 2 keeps single-core boxes making progress on two files
/// while one waits on disk.
fn analysis_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(2)
}

/// What a worker produced for one file.
enum Analyzed {
    Image(Option<Analysis>),
    /// Video: whether ffmpeg wrote a first-frame thumbnail.
    Video(bool),
}

/// Analyze every un-hashed file for `user`: compute blur + perceptual hash,
/// write a thumbnail, then group near-duplicates.
///
/// Decoding runs on the blocking pool, `analysis_concurrency()` files at a time
/// — this is the slowest stage by far (see `examples/bench.rs`), and doing it
/// one-at-a-time left every core but one idle. Results are applied to SQLite as
/// they land, in completion order: writes stay serialized (SQLite has a single
/// writer anyway) while the decoding fans out.
pub async fn run_detection(
    pool: &Db,
    user: &str,
    blur_threshold: f64,
    thumb_dir: &Path,
    dup_distance: u32,
    mut progress: impl FnMut(usize),
) -> Result<DetectStats> {
    let pending = db::pending_for_detection(pool, user).await?;
    let mut stats = DetectStats::default();

    let jobs = pending.into_iter().map(|(id, path, media_type)| {
        let thumb_path = thumb_dir.join(format!("{id}.jpg"));
        async move {
            let tp = thumb_path.clone();
            let done = tokio::task::spawn_blocking(move || {
                if media_type == "image" {
                    Analyzed::Image(analyze_image(Path::new(&path), &tp, blur_threshold))
                } else {
                    Analyzed::Video(thumbs::ensure_video_thumb(&path, &tp).unwrap_or(false))
                }
            })
            .await;
            (id, thumb_path, done)
        }
    });

    let mut in_flight = stream::iter(jobs).buffer_unordered(analysis_concurrency());

    while let Some((id, thumb_path, done)) = in_flight.next().await {
        // A panicking decode kills only its own file, not the whole run.
        let Ok(done) = done else { continue };

        match done {
            Analyzed::Video(made_thumb) => {
                if made_thumb {
                    let _ = db::set_thumb(pool, id, &thumb_path.to_string_lossy()).await;
                    stats.thumbs += 1;
                }
            }
            Analyzed::Image(Some(a)) => {
                let thumb_str: Option<PathBuf> = a.thumb_made.then(|| thumb_path.clone());
                let thumb_ref = thumb_str.as_ref().map(|p| p.to_string_lossy().to_string());
                db::update_analysis(
                    pool,
                    id,
                    Some(a.blur_score),
                    a.is_blurry,
                    &a.phash,
                    thumb_ref.as_deref(),
                )
                .await?;
                stats.processed += 1;
                if a.is_blurry {
                    stats.blurry += 1;
                }
                if a.thumb_made {
                    stats.thumbs += 1;
                }
                progress(stats.processed);
            }
            // Undecodable file (corrupt, or HEIC in a build without libheif):
            // it stays indexed and un-hashed, so a later run can retry it.
            Analyzed::Image(None) => {}
        }
    }

    // Group near-duplicates by perceptual-hash Hamming distance.
    let hashes = db::all_phashes(pool, user).await?;
    let groups = group_duplicates(&hashes, dup_distance);
    for (gid, ids) in &groups {
        for id in ids {
            db::set_group(pool, *id, gid).await?;
        }
    }
    stats.duplicate_groups = groups.len();
    Ok(stats)
}

/// Decode an image once and derive blur score, perceptual hash, and a thumbnail.
pub fn analyze_image(path: &Path, thumb_out: &Path, blur_threshold: f64) -> Option<Analysis> {
    let img = crate::decode::open_image(path).ok()?;
    let blur = laplacian_variance(&img);
    let phash = dhash(&img);
    let thumb_made = thumbs::make_image_thumb(&img, thumb_out).is_ok();
    Some(Analysis {
        blur_score: blur,
        is_blurry: blur < blur_threshold,
        phash,
        thumb_made,
    })
}

/// 64-bit difference hash (dHash), hex-encoded. Resize to 9×8 grayscale and
/// compare horizontally adjacent pixels.
pub fn dhash(img: &DynamicImage) -> String {
    let small = image::imageops::resize(&img.to_luma8(), 9, 8, FilterType::Triangle);
    let mut bits: u64 = 0;
    let mut i = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            if left < right {
                bits |= 1u64 << i;
            }
            i += 1;
        }
    }
    format!("{bits:016x}")
}

/// Variance of the Laplacian — a sharpness proxy (low ⇒ blurry). Computed on a
/// grayscale image downscaled so the score is roughly resolution-independent.
pub fn laplacian_variance(img: &DynamicImage) -> f64 {
    let gray = downscale_luma(img.to_luma8(), 512);
    let (w, h) = gray.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let at = |x: u32, y: u32| gray.get_pixel(x, y).0[0] as f64;
    let mut vals = Vec::with_capacity(((w - 2) * (h - 2)) as usize);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4.0 * at(x, y);
            vals.push(lap);
        }
    }
    let n = vals.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let mean = vals.iter().sum::<f64>() / n;
    vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n
}

fn downscale_luma(img: GrayImage, max_edge: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    let longest = w.max(h);
    if longest <= max_edge {
        return img;
    }
    let scale = max_edge as f32 / longest as f32;
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    image::imageops::resize(&img, nw, nh, FilterType::Triangle)
}

/// Union-find clustering of hashes within `max_dist` Hamming distance.
/// Returns only clusters with more than one member, each with a group id.
/// O(n²) — fine for MVP volumes; replace with a BK-tree for very large sets.
pub fn group_duplicates(hashes: &[(i64, String)], max_dist: u32) -> Vec<(String, Vec<i64>)> {
    let parsed: Vec<(i64, u64)> = hashes
        .iter()
        .filter_map(|(id, h)| u64::from_str_radix(h, 16).ok().map(|v| (*id, v)))
        .collect();
    let n = parsed.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if (parsed[i].1 ^ parsed[j].1).count_ones() <= max_dist {
                let a = find(&mut parent, i);
                let b = find(&mut parent, j);
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }

    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        clusters.entry(r).or_default().push(i);
    }

    let mut out = Vec::new();
    for members in clusters.values() {
        if members.len() < 2 {
            continue;
        }
        let rep_hash = members.iter().map(|&i| parsed[i].1).min().unwrap_or(0);
        let gid = format!("{rep_hash:016x}")[..8].to_string();
        out.push((gid, members.iter().map(|&i| parsed[i].0).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    fn solid(w: u32, h: u32, v: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([v, v, v])))
    }

    fn noisy(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let v = (((x * 37 + y * 91) % 256) as u8) ^ ((x.wrapping_mul(y) % 256) as u8);
            *px = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn identical_images_have_zero_distance() {
        let a = u64::from_str_radix(&dhash(&noisy(64, 64)), 16).unwrap();
        let b = u64::from_str_radix(&dhash(&noisy(64, 64)), 16).unwrap();
        assert_eq!((a ^ b).count_ones(), 0);
    }

    #[test]
    fn flat_vs_textured_differ() {
        let flat = u64::from_str_radix(&dhash(&solid(64, 64, 128)), 16).unwrap();
        let tex = u64::from_str_radix(&dhash(&noisy(64, 64)), 16).unwrap();
        assert!((flat ^ tex).count_ones() > 5);
    }

    #[test]
    fn blur_orders_sharp_above_flat() {
        // A flat image has ~0 Laplacian variance; a textured one is much higher.
        assert!(laplacian_variance(&noisy(128, 128)) > laplacian_variance(&solid(128, 128, 128)));
    }

    /// End-to-end over the concurrent path: every file must be analyzed exactly
    /// once, get a thumbnail, and duplicates must still group — none of which is
    /// guaranteed by construction now that files are decoded in parallel and
    /// applied in completion order.
    #[tokio::test]
    async fn detection_processes_every_file_concurrently() {
        let dir = std::env::temp_dir().join(format!("ec-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let media = dir.join("media");
        std::fs::create_dir_all(&media).unwrap();

        // More files than cores, so the concurrency limit actually kicks in.
        let count = analysis_concurrency() * 3 + 1;
        for i in 0..count {
            // Every third image repeats the previous one → duplicate groups.
            let seed = if i % 3 == 0 { i.saturating_sub(1) } else { i };
            let mut img = RgbImage::new(48, 32);
            for (x, y, px) in img.enumerate_pixels_mut() {
                let v = ((x * 7 + y * 13 + seed as u32 * 29) % 256) as u8;
                *px = Rgb([v, v.wrapping_mul(3), v.wrapping_add(seed as u8)]);
            }
            img.save(media.join(format!("img_{i:03}.png"))).unwrap();
        }

        let pool = crate::db::init(&dir.join("test.db")).await.unwrap();
        let scanned = crate::scan::scan_folder(&pool, "u", &media, |_| {})
            .await
            .unwrap();
        assert_eq!(scanned.added, count);

        let thumbs_dir = dir.join("thumbs");
        let mut progress_calls = 0;
        let stats = run_detection(&pool, "u", 100.0, &thumbs_dir, 4, |_| progress_calls += 1)
            .await
            .unwrap();

        assert_eq!(stats.processed, count, "every file analyzed exactly once");
        assert_eq!(stats.thumbs, count, "every file got a thumbnail");
        assert_eq!(progress_calls, count, "progress reported once per file");
        assert!(stats.duplicate_groups > 0, "repeated images should group");
        assert_eq!(
            std::fs::read_dir(&thumbs_dir).unwrap().count(),
            count,
            "thumbnail files on disk"
        );

        // A second pass has nothing left to do — rows are no longer un-hashed.
        let again = run_detection(&pool, "u", 100.0, &thumbs_dir, 4, |_| {})
            .await
            .unwrap();
        assert_eq!(again.processed, 0, "detection must not redo finished files");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grouping_clusters_duplicates() {
        let hashes = vec![
            (1, "ffffffffffffffff".to_string()),
            (2, "ffffffffffffffff".to_string()), // identical to 1
            (3, "0000000000000000".to_string()), // far from 1/2
        ];
        let groups = group_duplicates(&hashes, 4);
        assert_eq!(groups.len(), 1);
        let mut ids = groups[0].1.clone();
        ids.sort();
        assert_eq!(ids, vec![1, 2]);
    }
}

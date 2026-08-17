//! Scale benchmark — the "does this survive a real library?" check.
//!
//!   cargo run --release -p easyclean-core --example bench -- [count] [workdir]
//!
//! Generates `count` synthetic photos (default 10,000), then times the three
//! things that decide whether the app is usable at 100K+: the indexing walk,
//! the decode/analyze pass, and the queries the UI makes on every screen.
//!
//! Synthetic images are small, so per-image decode cost is far below a real
//! 12 MP phone photo — read the scan/query numbers as representative and the
//! detection number as a floor, not a promise.

use std::path::{Path, PathBuf};
use std::time::Instant;

use easyclean_core::{db, detect, scan, Config};
use image::codecs::jpeg::JpegEncoder;
use image::RgbImage;

const IMG_W: u32 = 320;
const IMG_H: u32 = 240;
/// Deliberately low: high-frequency synthetic texture barely compresses, and
/// 10K images at default quality is over a gigabyte of scratch space.
const JPEG_QUALITY: u8 = 55;
/// Every Nth image is an exact copy of its predecessor, so duplicate grouping
/// has something real to find.
const DUP_EVERY: usize = 20;

fn gen_images(dir: &Path, count: usize) -> std::io::Result<usize> {
    std::fs::create_dir_all(dir)?;
    let existing = std::fs::read_dir(dir)?.count();
    if existing >= count {
        println!("reusing {existing} images already in {dir:?}");
        return Ok(existing);
    }

    let t0 = Instant::now();
    let mut prev: Option<RgbImage> = None;
    for i in existing..count {
        let path = dir.join(format!("photo_{i:06}.jpg"));
        let img = match (&prev, i % DUP_EVERY == 0 && i > 0) {
            (Some(p), true) => p.clone(),
            _ => {
                let seed = i as u32;
                let mut img = RgbImage::new(IMG_W, IMG_H);
                for (x, y, px) in img.enumerate_pixels_mut() {
                    // Cheap deterministic texture; varies per image so hashes
                    // and blur scores spread out instead of all colliding.
                    let v = ((x * (seed % 13 + 1) + y * (seed % 7 + 1)) ^ (x * y / (seed % 5 + 1)))
                        % 256;
                    px.0 = [v as u8, ((v * 3) % 256) as u8, ((v * 7 + seed) % 256) as u8];
                }
                img
            }
        };
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path)?);
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .encode_image(&img)
            .map_err(std::io::Error::other)?;
        prev = Some(img);
        if (i + 1) % 1000 == 0 {
            println!("  generated {}/{count}", i + 1);
        }
    }
    println!(
        "generated {count} images in {:.1}s",
        t0.elapsed().as_secs_f64()
    );
    Ok(count)
}

fn rate(n: usize, secs: f64) -> String {
    if secs <= 0.0 {
        return "—".into();
    }
    format!("{:.0}/s", n as f64 / secs)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let count: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let work = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("easyclean-bench"));

    let media = work.join("media");
    let data = work.join("data");
    let user = "bench";

    println!("── EasyClean scale benchmark ──");
    println!("images: {count}   workdir: {work:?}\n");

    gen_images(&media, count)?;

    // Fresh DB each run so numbers are comparable.
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data)?;

    let cfg = Config {
        data_dir: data.clone(),
        media_root: media.clone(),
        ..Config::default()
    };
    let pool = db::init(&cfg.db_path()).await?;
    let thumb_dir = data.join("thumbs");

    // ── 1. Scan (walk + EXIF + insert) ────────────────────────────────────
    let t = Instant::now();
    let s = scan::scan_folder(&pool, user, &media, |_| {}).await?;
    let scan_secs = t.elapsed().as_secs_f64();
    println!(
        "scan     : {} added in {:.1}s  ({})",
        s.added,
        scan_secs,
        rate(s.added, scan_secs)
    );

    // ── 2. Detection (decode + blur + dHash + thumbnail + grouping) ───────
    let t = Instant::now();
    let d = detect::run_detection(
        &pool,
        user,
        cfg.blur_threshold,
        &thumb_dir,
        cfg.duplicate_distance_threshold,
        |n| {
            if n % 2000 == 0 {
                println!("  analyzed {n}");
            }
        },
    )
    .await?;
    let det_secs = t.elapsed().as_secs_f64();
    println!(
        "detect   : {} analyzed in {:.1}s  ({})  blurry={} dup_groups={} thumbs={}",
        d.processed,
        det_secs,
        rate(d.processed, det_secs),
        d.blurry,
        d.duplicate_groups,
        d.thumbs
    );

    // ── 3. The queries every screen makes ─────────────────────────────────
    // These are what decide whether the UI feels instant at 100K+.
    let q = |label: &'static str, ms: f64| {
        let flag = if ms > 200.0 { "  ⚠ over 200ms" } else { "" };
        println!("query    : {label:<22} {ms:>7.1} ms{flag}");
    };

    let t = Instant::now();
    let batch = db::get_batch(&pool, user, "timeline", None, 200).await?;
    q("batch(timeline,200)", t.elapsed().as_secs_f64() * 1000.0);

    let t = Instant::now();
    let _ = db::get_batch(&pool, user, "duplicates", None, 200).await?;
    q("batch(duplicates,200)", t.elapsed().as_secs_f64() * 1000.0);

    let t = Instant::now();
    let _ = db::get_batch(&pool, user, "blurs", None, 200).await?;
    q("batch(blurs,200)", t.elapsed().as_secs_f64() * 1000.0);

    let t = Instant::now();
    let stats = db::stats_summary(&pool, user).await?;
    q("stats/summary", t.elapsed().as_secs_f64() * 1000.0);

    let t = Instant::now();
    let months = db::timeline_months(&pool, user).await?;
    q("stats/timeline", t.elapsed().as_secs_f64() * 1000.0);

    // ── 4. Incremental rescan (the common case: nothing new) ──────────────
    let t = Instant::now();
    let s2 = scan::scan_folder(&pool, user, &media, |_| {}).await?;
    let rescan_secs = t.elapsed().as_secs_f64();
    println!(
        "rescan   : {} skipped, {} added in {:.1}s  ({})",
        s2.skipped,
        s2.added,
        rescan_secs,
        rate(s2.scanned, rescan_secs)
    );

    println!(
        "\ntotals   : {} files · {} in batch · {} months · db {:.1} MB",
        stats.total,
        batch.len(),
        months.len(),
        std::fs::metadata(cfg.db_path())
            .map(|m| m.len() as f64 / 1e6)
            .unwrap_or(0.0),
    );
    println!("workdir kept at {work:?} — delete it to reclaim space");
    Ok(())
}

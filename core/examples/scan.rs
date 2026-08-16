//! Dev tool: run a scan + detection pass against a real folder.
//!
//!   cargo run -p easyclean-core --example scan -- <dir> [user]
//!
//! Honors EASYCLEAN_DATA_DIR / EASYCLEAN_BLUR_THRESHOLD / etc.

use std::path::PathBuf;

use easyclean_core::{db, detect, scan, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: scan <dir> [user]"));
    let user = args.next().unwrap_or_else(|| "default".to_string());

    let cfg = Config::from_env();
    std::fs::create_dir_all(&cfg.data_dir)?;
    let pool = db::init(&cfg.db_path()).await?;
    let thumb_dir = cfg.data_dir.join("thumbs");

    println!("scanning {dir:?} as user '{user}' (db: {:?}) ...", cfg.db_path());
    let s = scan::scan_folder(&pool, &user, &dir, |n| {
        if n % 25 == 0 {
            println!("  indexed {n}");
        }
    })
    .await?;
    println!("scan:   scanned={} added={} skipped={}", s.scanned, s.added, s.skipped);

    println!("detecting (blur + duplicates + thumbnails) ...");
    let d = detect::run_detection(
        &pool,
        &user,
        cfg.blur_threshold,
        &thumb_dir,
        cfg.duplicate_distance_threshold,
        |n| {
            if n % 25 == 0 {
                println!("  analyzed {n}");
            }
        },
    )
    .await?;
    println!(
        "detect: processed={} blurry={} duplicate_groups={} thumbs={}",
        d.processed, d.blurry, d.duplicate_groups, d.thumbs
    );
    println!("thumbnails in {:?}", thumb_dir);
    Ok(())
}

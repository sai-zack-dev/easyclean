use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::storage::UserRoot;

/// Pick a non-colliding destination path inside `dir` for `filename`,
/// appending `_1`, `_2`, … before the extension if needed.
pub fn unique_dest(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let p = Path::new(filename);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str());
    let mut i = 1u32;
    loop {
        let name = match ext {
            Some(e) => format!("{stem}_{i}.{e}"),
            None => format!("{stem}_{i}"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

/// Move `src` into `dest_dir` (created if needed), avoiding name collisions.
/// Falls back to copy+remove across filesystem boundaries (e.g. EXDEV).
pub fn move_into(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let filename = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dest = unique_dest(dest_dir, filename);
    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(dest)
}

/// Move a file into the user's `.trash/` (trash-first deletion).
pub fn to_trash(root: &UserRoot, src: &Path) -> Result<PathBuf> {
    move_into(src, &root.trash())
}

/// Restore a trashed file back into `dest_dir`.
pub fn restore(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    move_into(src, dest_dir)
}

/// Move a kept file into `clean/<album>/` (virtual album → physical).
pub fn to_clean(root: &UserRoot, src: &Path, album: &str) -> Result<PathBuf> {
    move_into(src, &root.clean().join(album))
}

/// Permanently delete a file (used by retention purge).
pub fn purge(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_then_restore_round_trips() {
        let base = std::env::temp_dir().join("easyclean_fileops_test");
        let _ = std::fs::remove_dir_all(&base);
        let media_root = base.join("media");
        let root = UserRoot::new(&media_root, "saiz").unwrap();
        root.ensure().unwrap();

        let src = root.source().join("pic.jpg");
        std::fs::write(&src, b"hello").unwrap();

        let trashed = to_trash(&root, &src).unwrap();
        assert!(!src.exists(), "source should be gone after trashing");
        assert!(trashed.exists(), "trashed copy should exist");
        assert!(trashed.starts_with(root.trash()));

        let restored = restore(&trashed, &root.source()).unwrap();
        assert!(restored.exists());
        assert_eq!(std::fs::read(&restored).unwrap(), b"hello");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn collisions_get_suffixed() {
        let dir = std::env::temp_dir().join("easyclean_fileops_collide");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"x").unwrap();
        assert_eq!(unique_dest(&dir, "a.jpg"), dir.join("a_1.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

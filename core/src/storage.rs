use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Resolves the per-user `source/`, `clean/`, and `.trash/` folders and guards
/// every derived path against traversal outside the user's root.
#[derive(Debug, Clone)]
pub struct UserRoot {
    root: PathBuf,
}

impl UserRoot {
    /// `media_root/<user>`. Rejects usernames that aren't a single safe segment.
    pub fn new(media_root: &Path, user: &str) -> Result<Self> {
        if user.is_empty()
            || user == "."
            || user == ".."
            || user.contains('/')
            || user.contains('\\')
            || user.contains('\0')
        {
            return Err(Error::InvalidUser(user.to_string()));
        }
        Ok(Self {
            root: media_root.join(user),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn source(&self) -> PathBuf {
        self.root.join("source")
    }

    pub fn clean(&self) -> PathBuf {
        self.root.join("clean")
    }

    pub fn trash(&self) -> PathBuf {
        self.root.join(".trash")
    }

    /// Create the three folders if missing.
    pub fn ensure(&self) -> Result<()> {
        for d in [self.source(), self.clean(), self.trash()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// Join a relative path to the user root, normalizing `.`/`..` lexically and
    /// rejecting anything that escapes the root. Does not touch the filesystem.
    pub fn safe_join(&self, rel: &str) -> Result<PathBuf> {
        let mut out = self.root.clone();
        for comp in Path::new(rel).components() {
            match comp {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(Error::PathTraversal(rel.to_string()));
                }
                Component::ParentDir => {
                    if !out.pop() || !out.starts_with(&self.root) {
                        return Err(Error::PathTraversal(rel.to_string()));
                    }
                }
                Component::CurDir => {}
                Component::Normal(seg) => out.push(seg),
            }
        }
        if !out.starts_with(&self.root) {
            return Err(Error::PathTraversal(rel.to_string()));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ur() -> UserRoot {
        UserRoot::new(Path::new("/media"), "saiz").unwrap()
    }

    #[test]
    fn folders_resolve() {
        let u = ur();
        assert_eq!(u.source(), PathBuf::from("/media/saiz/source"));
        assert_eq!(u.clean(), PathBuf::from("/media/saiz/clean"));
        assert_eq!(u.trash(), PathBuf::from("/media/saiz/.trash"));
    }

    #[test]
    fn rejects_bad_usernames() {
        assert!(UserRoot::new(Path::new("/media"), "..").is_err());
        assert!(UserRoot::new(Path::new("/media"), "a/b").is_err());
        assert!(UserRoot::new(Path::new("/media"), "").is_err());
    }

    #[test]
    fn safe_join_allows_inside() {
        let u = ur();
        assert_eq!(
            u.safe_join("source/2026/img.jpg").unwrap(),
            PathBuf::from("/media/saiz/source/2026/img.jpg")
        );
        // a `..` that stays within root is fine
        assert_eq!(
            u.safe_join("source/../clean/x.jpg").unwrap(),
            PathBuf::from("/media/saiz/clean/x.jpg")
        );
    }

    #[test]
    fn safe_join_blocks_escape() {
        let u = ur();
        assert!(u.safe_join("../../etc/passwd").is_err());
        assert!(u.safe_join("source/../../secret").is_err());
        assert!(u.safe_join("/etc/passwd").is_err());
    }
}

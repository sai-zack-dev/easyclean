//! Resumable chunked uploads.
//!
//! A file arrives in three acts: `init` reserves room and opens a scratch file,
//! `chunk` appends bytes at an explicit offset, and `finish` verifies the whole
//! thing and moves it into the user's `source/` folder as though it had always
//! been there.
//!
//! Two properties shape everything here.
//!
//! **Scratch files never live in the media root.** A partial upload under
//! `<data_dir>/uploads/<user>/` cannot be walked by the scanner, indexed,
//! thumbnailed or served; the same file sitting half-written in `source/` could
//! be all four. The cost is that the final move usually crosses a filesystem
//! boundary — `/data` and `/media` are separate mounts in the deployment — so
//! it goes through [`fileops::move_into`], which falls back to copy+remove.
//!
//! **Nothing the client says is believed.** The declared size bounds how much
//! will be accepted, and the declared hash is checked against bytes the server
//! hashed itself. A mismatch discards the upload rather than indexing a file
//! that is not what it claims to be.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::db::{self, Db};
use crate::error::{Error, Result};
use crate::scan::{self, IMAGE_EXTS, VIDEO_EXTS};
use crate::storage::{validate_user_segment, UserRoot};
use crate::Config;

/// `user_setting` key holding an admin-set quota, in bytes.
///
/// The database is consulted before `EASYCLEAN_QUOTAS` so an in-app change
/// wins, and seeding never writes this key — which is what keeps a restart from
/// reverting it the way re-seeded passcodes would. See
/// `.claude/auth-redesign-brief.md` for the trap this sidesteps.
pub const QUOTA_SETTING_KEY: &str = "quota_bytes";

/// Longest filename kept, in bytes. Comfortably under the 255 that ext4 and
/// APFS allow, with room for the `_1`, `_2` … that collision-avoidance appends.
const MAX_FILENAME_LEN: usize = 180;

/// How long a session may sit untouched before the sweeper reclaims it.
///
/// Long enough to survive a phone being locked, a train tunnel, or a user
/// wandering off mid-batch; short enough that abandoned scratch files do not
/// accumulate on a disk whose free space is the thing being defended.
pub const SESSION_IDLE_SECS: i64 = 6 * 60 * 60;

/// What `init` tells the client.
#[derive(Debug, Clone)]
pub struct Init {
    pub id: String,
    pub offset: u64,
    pub chunk_size: u64,
    /// The name the file will actually get, which may differ from the one
    /// offered — the client should display this rather than its own.
    pub filename: String,
}

/// What `finish` produced.
#[derive(Debug, Clone)]
pub struct Finished {
    pub file_id: i64,
    pub filename: String,
    pub path: PathBuf,
    pub size: u64,
}

/// One in-flight upload.
struct Session {
    user: String,
    filename: String,
    declared_size: u64,
    claimed_hash: String,
    received: u64,
    part_path: PathBuf,
    touched: i64,
}

/// Registry of in-flight uploads.
///
/// In memory, not in the database: a session is worth exactly as long as the
/// process that holds the scratch file, and a restart drops both together
/// ([`purge_orphans`] clears what is left on disk). Resuming across a restart
/// would mean persisting offsets and trusting scratch files found at boot —
/// more surface than a household photo app needs to survive a `docker restart`.
/// Within one process lifetime, which is where dropped Wi-Fi and backgrounded
/// apps live, resume works.
#[derive(Clone, Default)]
pub struct UploadManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Session>>>>>,
    /// Bytes promised by open sessions, per user.
    ///
    /// Without this, ten `init` calls in the same second each see the same
    /// "bytes used" and each conclude there is room — and the quota is a
    /// suggestion. Reserving the declared size up front makes the check exact,
    /// and it means the per-chunk path needs no database query at all.
    reserved: Arc<Mutex<HashMap<String, u64>>>,
}

impl UploadManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session: validate, reserve quota and disk, create the scratch file.
    pub async fn init(
        &self,
        cfg: &Config,
        pool: &Db,
        user: &str,
        raw_filename: &str,
        size: u64,
        claimed_hash: &str,
    ) -> Result<Init> {
        validate_user_segment(user)?;
        let filename = sanitize_filename(raw_filename)?;
        let claimed_hash = normalize_hash(claimed_hash)?;

        // Zero-byte files have no thumbnail, no EXIF and no reason to exist
        // here; letting one through just creates a row nothing can render.
        if size == 0 {
            return Err(Error::InvalidFilename("file is empty".into()));
        }

        self.reserve(cfg, pool, user, size).await?;

        // From here on, any failure must give the reservation back.
        //
        // A directory per session, not just a file: `finish` renames the
        // scratch file to the user's chosen name before moving it, and two
        // people uploading `IMG_0001.jpg` at once would otherwise stage onto
        // the same path and one would overwrite the other.
        let id = random_id()?;
        let dir = uploads_dir(cfg, user).join(&id);
        let part_path = dir.join("upload.part");
        let opened = async {
            tokio::fs::create_dir_all(&dir).await?;
            tokio::fs::File::create(&part_path).await?;
            Ok::<_, Error>(())
        }
        .await;
        if let Err(e) = opened {
            self.release(user, size).await;
            return Err(e);
        }

        let session = Session {
            user: user.to_string(),
            filename: filename.clone(),
            declared_size: size,
            claimed_hash,
            received: 0,
            part_path,
            touched: now(),
        };
        self.sessions
            .lock()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(session)));

        Ok(Init {
            id,
            offset: 0,
            chunk_size: cfg.upload_chunk_size,
            filename,
        })
    }

    /// Bytes safely on disk for this session — what a resuming client asks for.
    pub async fn offset(&self, id: &str, user: &str) -> Result<u64> {
        let s = self.session(id).await?;
        let s = s.lock().await;
        owned_by(&s, user)?;
        Ok(s.received)
    }

    /// Append one chunk. `offset` must be exactly where the file currently ends.
    ///
    /// Requiring the offset rather than just appending is what makes a retry
    /// safe: a client that resends a chunk it is unsure about gets a mismatch
    /// naming the real offset, instead of silently writing those bytes twice.
    pub async fn write_chunk(&self, id: &str, user: &str, offset: u64, data: &[u8]) -> Result<u64> {
        let s = self.session(id).await?;
        let mut s = s.lock().await;
        owned_by(&s, user)?;

        if offset != s.received {
            return Err(Error::OffsetMismatch {
                expected: s.received,
            });
        }
        // The declared size was quota- and disk-checked at `init`; refusing to
        // exceed it here is what makes that check binding rather than advisory.
        let len = data.len() as u64;
        if s.received + len > s.declared_size {
            return Err(Error::InvalidFilename(format!(
                "chunk would exceed the declared size of {} bytes",
                s.declared_size
            )));
        }

        let mut f = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&s.part_path)
            .await?;
        f.write_all(data).await?;
        f.flush().await?;

        s.received += len;
        s.touched = now();
        Ok(s.received)
    }

    /// Verify the assembled file and move it into the user's `source/`.
    pub async fn finish(&self, cfg: &Config, pool: &Db, id: &str, user: &str) -> Result<Finished> {
        let s = self.session(id).await?;
        let guard = s.lock().await;
        owned_by(&guard, user)?;

        if guard.received != guard.declared_size {
            return Err(Error::OffsetMismatch {
                expected: guard.received,
            });
        }

        // Hash the finished file rather than keeping a rolling digest across
        // chunks. A rolling hash cannot be rewound, so a chunk that fails
        // halfway would poison it with no way back; reading the file once is a
        // few hundred milliseconds even for a video, and it is always right.
        let actual = hash_file(&guard.part_path).await?;
        if actual != guard.claimed_hash {
            discard(&guard.part_path).await;
            let user = guard.user.clone();
            let size = guard.declared_size;
            drop(guard);
            self.forget(id).await;
            self.release(&user, size).await;
            return Err(Error::HashMismatch);
        }

        let root = UserRoot::new(&cfg.media_root, user)?;
        let source = root.source();
        tokio::fs::create_dir_all(&source).await?;

        // Rename under the sanitized name first so the move lands on the name
        // we vetted, not the random session id.
        let staged = guard.part_path.with_file_name(&guard.filename);
        tokio::fs::rename(&guard.part_path, &staged).await?;
        let dest = crate::fileops::move_into(&staged, &source)?;

        let file_id = scan::index_file(pool, user, &dest, &source, Some(actual))
            .await?
            .ok_or_else(|| Error::UnsupportedMedia(guard.filename.clone()))?;

        let out = Finished {
            file_id,
            filename: dest
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&guard.filename)
                .to_string(),
            path: dest,
            size: guard.declared_size,
        };
        let user = guard.user.clone();
        let size = guard.declared_size;
        drop(guard);
        self.forget(id).await;
        self.release(&user, size).await;
        Ok(out)
    }

    /// Give up on a session and delete its scratch file.
    pub async fn abort(&self, id: &str, user: &str) -> Result<()> {
        let s = self.session(id).await?;
        let (owner, size, part) = {
            let s = s.lock().await;
            owned_by(&s, user)?;
            (s.user.clone(), s.declared_size, s.part_path.clone())
        };
        discard(&part).await;
        self.forget(id).await;
        self.release(&owner, size).await;
        Ok(())
    }

    /// Drop sessions untouched for longer than [`SESSION_IDLE_SECS`].
    ///
    /// Returns how many were reclaimed. Without this an abandoned batch holds
    /// both a scratch file and a quota reservation for as long as the process
    /// runs — the reservation being the worse of the two, since it silently
    /// shrinks the ceiling the user is allowed to reach.
    pub async fn sweep(&self, max_idle_secs: i64) -> usize {
        let cutoff = now() - max_idle_secs;
        let stale: Vec<String> = {
            let map = self.sessions.lock().await;
            let mut out = Vec::new();
            for (id, s) in map.iter() {
                if s.lock().await.touched < cutoff {
                    out.push(id.clone());
                }
            }
            out
        };
        let mut reclaimed = 0;
        for id in stale {
            let Some(s) = self.sessions.lock().await.remove(&id) else {
                continue;
            };
            let (user, size, part) = {
                let s = s.lock().await;
                (s.user.clone(), s.declared_size, s.part_path.clone())
            };
            discard(&part).await;
            self.release(&user, size).await;
            reclaimed += 1;
        }
        reclaimed
    }

    /// Reserve `size` against the user's quota and the disk floor.
    async fn reserve(&self, cfg: &Config, pool: &Db, user: &str, size: u64) -> Result<()> {
        let used = db::bytes_used(pool, user).await?.max(0) as u64;
        let mut reserved = self.reserved.lock().await;
        let in_flight = reserved.get(user).copied().unwrap_or(0);

        if let Some(limit) = quota_bytes(pool, cfg, user).await? {
            let committed = used.saturating_add(in_flight);
            if committed.saturating_add(size) > limit {
                return Err(Error::QuotaExceeded {
                    used: committed,
                    limit,
                    needed: size,
                });
            }
        }

        // Disk space is checked against everything in flight too, for the same
        // reason the quota is: ten concurrent inits must not each see the same
        // free space and all conclude they fit.
        if let Some(free) = available_bytes(&cfg.media_root) {
            let after = free.saturating_sub(in_flight).saturating_sub(size);
            if after < cfg.disk_floor_bytes {
                return Err(Error::DiskFull);
            }
        }

        *reserved.entry(user.to_string()).or_insert(0) += size;
        Ok(())
    }

    async fn release(&self, user: &str, size: u64) {
        let mut reserved = self.reserved.lock().await;
        if let Some(v) = reserved.get_mut(user) {
            *v = v.saturating_sub(size);
            if *v == 0 {
                reserved.remove(user);
            }
        }
    }

    async fn session(&self, id: &str) -> Result<Arc<Mutex<Session>>> {
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or(Error::UploadNotFound)
    }

    async fn forget(&self, id: &str) {
        self.sessions.lock().await.remove(id);
    }
}

/// A session belongs to exactly one account.
///
/// Returns [`Error::UploadNotFound`] rather than a forbidden, so someone
/// guessing ids learns nothing about which ones exist.
fn owned_by(s: &Session, user: &str) -> Result<()> {
    if s.user == user {
        Ok(())
    } else {
        Err(Error::UploadNotFound)
    }
}

/// This user's ceiling in bytes, or `None` for unlimited.
///
/// Database first, `EASYCLEAN_QUOTAS` second. That order is the whole point:
/// the admin UI writes the database, and the environment is only ever a
/// fallback that nothing copies into it — so there is no boot-time re-seed to
/// overwrite what an admin changed.
pub async fn quota_bytes(pool: &Db, cfg: &Config, user: &str) -> Result<Option<u64>> {
    if let Some(raw) = db::get_setting(pool, user, QUOTA_SETTING_KEY).await? {
        if let Ok(v) = raw.trim().parse::<u64>() {
            return Ok(Some(v));
        }
        tracing::warn!("ignoring unreadable {QUOTA_SETTING_KEY} {raw:?} for {user}");
    }
    Ok(cfg.quotas.get(user).copied())
}

/// Delete every leftover scratch file. Called once at startup.
///
/// Sessions do not survive a restart, so anything still here is orphaned: bytes
/// nobody will ever finish, occupying the free space the floor exists to
/// protect.
pub fn purge_orphans(cfg: &Config) -> std::io::Result<()> {
    let root = cfg.data_dir.join("uploads");
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    Ok(())
}

fn uploads_dir(cfg: &Config, user: &str) -> PathBuf {
    cfg.data_dir.join("uploads").join(user)
}

/// Delete a session's scratch directory, scratch file and all.
///
/// Best-effort by design: every caller is already on an error or cleanup path,
/// and failing to tidy up must not turn a handled outcome into an unhandled
/// one. The startup purge catches whatever is left.
async fn discard(part_path: &Path) {
    if let Some(dir) = part_path.parent() {
        let _ = tokio::fs::remove_dir_all(dir).await;
    } else {
        let _ = tokio::fs::remove_file(part_path).await;
    }
}

/// Make a client-supplied name safe to place on disk, or refuse it.
///
/// Refusing is a real outcome here rather than a fallback to something
/// generated: a name that had to be invented is a name the user will not
/// recognise in their own gallery.
pub fn sanitize_filename(raw: &str) -> Result<String> {
    // Take only the last component, treating both separators as such — a
    // Windows client sends backslashes, and `Path` on Unix does not agree that
    // those divide anything.
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if base.is_empty() || base == "." || base == ".." {
        return Err(Error::InvalidFilename(raw.to_string()));
    }
    if base.chars().any(|c| c.is_control()) {
        return Err(Error::InvalidFilename("contains control characters".into()));
    }

    // Leading dots would make the file hidden, and a name like `.trash` sitting
    // in `source/` invites confusion with the managed folders.
    let base = base.trim_start_matches('.').to_string();
    if base.is_empty() {
        return Err(Error::InvalidFilename(raw.to_string()));
    }

    let ext = Path::new(&base)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !IMAGE_EXTS.contains(&ext.as_str()) && !VIDEO_EXTS.contains(&ext.as_str()) {
        return Err(Error::UnsupportedMedia(base));
    }

    Ok(truncate_keeping_ext(&base, &ext))
}

/// Shorten an over-long name without losing the extension.
///
/// The extension is what decides whether the file can ever be decoded, so it is
/// the last thing that may be cut.
fn truncate_keeping_ext(name: &str, ext: &str) -> String {
    if name.len() <= MAX_FILENAME_LEN {
        return name.to_string();
    }
    let suffix = format!(".{ext}");
    let room = MAX_FILENAME_LEN.saturating_sub(suffix.len());
    let stem: String = name.chars().take(room).collect();
    format!("{stem}{suffix}")
}

/// Accept a SHA-256 in either case, hand back the lowercase form the database
/// stores. Anything else is refused rather than normalized into a lookup that
/// silently matches nothing.
pub fn normalize_hash(raw: &str) -> Result<String> {
    let h = raw.trim().to_ascii_lowercase();
    if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(h)
    } else {
        Err(Error::InvalidFilename("not a SHA-256 hex digest".into()))
    }
}

/// SHA-256 of a file, read in 1 MiB slices so a large video never sits in
/// memory whole.
async fn hash_file(path: &Path) -> Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Free bytes on the filesystem holding `path`, or `None` where it cannot be
/// determined — in which case the quota is the only limit left.
#[cfg(unix)]
fn available_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call,
    // and `stat` is fully written by `statvfs` before we read it.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut stat) != 0 {
            return None;
        }
        // `f_bavail`, not `f_bfree`: the latter counts blocks reserved for root,
        // which this process cannot use and must not promise.
        Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
    }
}

#[cfg(not(unix))]
fn available_bytes(_path: &Path) -> Option<u64> {
    None
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 16 random bytes, hex-encoded. Unguessable is the requirement — a session id
/// is the only thing standing between an upload and someone else's `source/`.
fn random_id() -> Result<String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| Error::Other(format!("no system randomness: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_directories_from_a_name() {
        assert_eq!(sanitize_filename("/etc/passwd.jpg").unwrap(), "passwd.jpg");
        assert_eq!(
            sanitize_filename("..\\..\\windows\\evil.png").unwrap(),
            "evil.png"
        );
        assert_eq!(sanitize_filename("a/b/c/photo.JPG").unwrap(), "photo.JPG");
    }

    #[test]
    fn refuses_names_that_cannot_be_made_safe() {
        assert!(sanitize_filename("").is_err());
        assert!(sanitize_filename("   ").is_err());
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("../..").is_err());
        assert!(sanitize_filename("photo\u{0}.jpg").is_err());
        assert!(sanitize_filename("photo\n.jpg").is_err());
        // A name that is nothing but dots has no stem left after they go.
        assert!(sanitize_filename("...").is_err());
    }

    #[test]
    fn hidden_files_cannot_be_created() {
        // `.trash.jpg` landing in source/ is a name chosen to be confusing.
        assert_eq!(sanitize_filename(".trash.jpg").unwrap(), "trash.jpg");
        assert_eq!(sanitize_filename("..hidden.png").unwrap(), "hidden.png");
    }

    #[test]
    fn only_media_extensions_are_accepted() {
        assert!(sanitize_filename("shell.sh").is_err());
        assert!(sanitize_filename("index.html").is_err());
        assert!(sanitize_filename("photo.jpg.exe").is_err());
        assert!(sanitize_filename("noextension").is_err());
        // The ones that must work, including the awkward cases.
        assert!(sanitize_filename("IMG_0001.HEIC").is_ok());
        assert!(sanitize_filename("clip.mov").is_ok());
        assert!(sanitize_filename("scan.tiff").is_ok());
    }

    #[test]
    fn long_names_keep_their_extension() {
        let long = format!("{}.jpg", "a".repeat(400));
        let out = sanitize_filename(&long).unwrap();
        assert!(out.len() <= MAX_FILENAME_LEN, "got {} bytes", out.len());
        assert!(out.ends_with(".jpg"), "extension must survive: {out}");
    }

    #[test]
    fn hashes_are_normalized_or_refused() {
        let upper = "A".repeat(64);
        assert_eq!(normalize_hash(&upper).unwrap(), "a".repeat(64));
        assert!(normalize_hash("deadbeef").is_err());
        assert!(normalize_hash(&"z".repeat(64)).is_err());
        assert!(normalize_hash("").is_err());
    }
}

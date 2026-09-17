-- Exact content hash (SHA-256, lowercase hex) for uploaded files.
--
-- Deliberately not `phash`. That column is a *perceptual* hash whose whole
-- purpose is to match photos that merely look alike — a resize, a re-encode and
-- the original all collide there, which is what makes duplicate detection
-- useful and what makes it useless for "have I already received these exact
-- bytes?". Upload dedupe needs the opposite property: identical or nothing.
--
-- Nullable, and only written by the upload path. Files found by a filesystem
-- scan leave it NULL rather than paying a full read of every file on disk at
-- scan time — at 100K photos that is the whole library through the CPU for a
-- column only the uploader reads. The cost of a NULL is that re-uploading a
-- photo which was already on the server by other means transfers it once more;
-- the existing perceptual duplicate pass catches that afterwards.
ALTER TABLE media_file ADD COLUMN content_hash TEXT;

-- Lookups are always "does this user already have this hash", never a global
-- hash scan, so the user column leads.
CREATE INDEX IF NOT EXISTS idx_media_user_hash ON media_file(user, content_hash);

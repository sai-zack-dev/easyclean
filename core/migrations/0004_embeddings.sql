-- CLIP embeddings, one row per media file.
--
-- Stored as int8 with a per-vector scale rather than float32: it is 4x smaller
-- (51MB vs 205MB at 100K photos) and ~5x faster to scan, and the ranking is
-- indistinguishable in practice. No separate vector database — an exact scan
-- over this table is ~15ms at 100K, which no ANN index would meaningfully beat
-- at the sizes one household's library reaches.
CREATE TABLE IF NOT EXISTS media_embedding (
    file_id    INTEGER PRIMARY KEY,
    -- Which model produced it. A different model means a different vector
    -- space, so changing models has to invalidate every row rather than
    -- silently compare vectors that were never comparable.
    model      TEXT    NOT NULL,
    dim        INTEGER NOT NULL,
    scale      REAL    NOT NULL,
    vec        BLOB    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_embedding_model ON media_embedding(model);

CREATE TABLE IF NOT EXISTS user (
    username  TEXT PRIMARY KEY,
    pass_hash TEXT NOT NULL,
    role      TEXT NOT NULL DEFAULT 'user'
);

CREATE TABLE IF NOT EXISTS media_file (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user        TEXT    NOT NULL,
    path        TEXT    NOT NULL UNIQUE,
    rel_path    TEXT,
    filename    TEXT    NOT NULL,
    media_type  TEXT    NOT NULL,
    size_bytes  INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    phash       TEXT,
    blur_score  REAL,
    is_blurry   INTEGER NOT NULL DEFAULT 0,
    status      TEXT    NOT NULL DEFAULT 'pending',
    trashed_at  INTEGER,
    group_id    TEXT,
    gps_lat     REAL,
    gps_lon     REAL,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    thumb_path  TEXT
);

CREATE INDEX IF NOT EXISTS idx_media_user_status  ON media_file(user, status);
CREATE INDEX IF NOT EXISTS idx_media_group        ON media_file(group_id);
CREATE INDEX IF NOT EXISTS idx_media_user_created ON media_file(user, created_at);
CREATE INDEX IF NOT EXISTS idx_media_phash        ON media_file(phash);

CREATE TABLE IF NOT EXISTS album (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user          TEXT    NOT NULL,
    name          TEXT    NOT NULL,
    category_type TEXT    NOT NULL,
    cover_file_id INTEGER,
    file_count    INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_album_user ON album(user);

CREATE TABLE IF NOT EXISTS album_file (
    album_id INTEGER NOT NULL,
    file_id  INTEGER NOT NULL,
    PRIMARY KEY (album_id, file_id)
);

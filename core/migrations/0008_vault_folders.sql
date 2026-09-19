-- Folders inside the vault.
--
-- Their own table rather than rows in `album` with a new category_type, and
-- that is the whole security argument: `/api/albums` lists every album row a
-- user has, with no PIN, so a vault folder stored there would put its name on a
-- screen anyone holding the phone can open. Here it is unreachable except
-- through the vault's own routes, by construction rather than by every album
-- query remembering to exclude it.
--
-- NOCASE so "Personal" and "personal" are one folder, not two that look alike
-- in the list.
CREATE TABLE IF NOT EXISTS vault_folder (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user       TEXT    NOT NULL,
    name       TEXT    NOT NULL COLLATE NOCASE,
    created_at INTEGER NOT NULL,
    UNIQUE (user, name)
);

-- One folder per hidden photo, or none ("Unfiled"). A column rather than a join
-- table because a photo is *in* a folder, not tagged with several — that is
-- the difference between these and albums.
--
-- Deliberately not on the `MediaFile` model: every Gallery response serialises
-- that struct, and a `vault_folder_id` field appearing in them — even as null —
-- would announce that a vault exists. Only the vault's own queries read it.
ALTER TABLE media_file ADD COLUMN vault_folder_id INTEGER;

CREATE INDEX IF NOT EXISTS idx_media_vault_folder ON media_file(vault_folder_id);

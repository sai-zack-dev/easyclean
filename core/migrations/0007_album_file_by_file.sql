-- Look an album membership up by the file rather than by the album.
--
-- `album_file`'s primary key is (album_id, file_id), which answers "what is in
-- this album" and nothing else. The opposite question — "which albums is this
-- file in" — now runs on every swipe, every bin and every restore, because a
-- deleted photo has to leave the shelves it was filed on and a restored one has
-- to go back. Without this index each of those is a full scan of a table with a
-- row per photo per album, which at the 100K files the project is built for is
-- a scan of hundreds of thousands of rows per swipe.
CREATE INDEX IF NOT EXISTS idx_album_file_file ON album_file(file_id);

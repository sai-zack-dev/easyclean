-- Months stopped being album categories: every photo was filed into both a
-- month and a year, so Memories listed "Nov 2023" beside "2023" — the same
-- pictures under two headings. Years alone are the useful grouping, and the
-- Clean tab's month checklist never used albums (it queries created_at), so
-- nothing else depends on these rows.
--
-- Membership rows go first; album_file has no foreign key, so deleting the
-- albums alone would strand them.
DELETE FROM album_file
WHERE album_id IN (SELECT id FROM album WHERE category_type = 'month');

DELETE FROM album WHERE category_type = 'month';

-- The hidden vault.
--
-- Membership is `media_file.status = 'vault'` rather than a flag column. Every
-- review mode already scopes to an explicit status (`pending` or `keep`), so a
-- single status value keeps vault rows out of the swipe decks, the duplicate
-- compare, the trash and the album builder without editing any of those
-- queries. Only the handful that say `status <> 'trashed'` — the Gallery's
-- library scope, the stats, the timeline and the search index — had to learn
-- about it, and `vault_leaks_into_nothing` in server/src/routes.rs is the test
-- that stays honest about the rest.
--
-- `pre_vault_*` records what the file was before it was hidden, so restoring it
-- puts it back exactly where it came from instead of dropping it into source/
-- as though it had just been scanned.
ALTER TABLE media_file ADD COLUMN pre_vault_status TEXT;
ALTER TABLE media_file ADD COLUMN pre_vault_path TEXT;

-- One PIN per user, bcrypt-hashed the same way the login passcode is.
--
-- Deliberately not the login passcode: the only thing a hidden vault defends
-- against is someone who can already open the app, and a vault that unlocks
-- with the password they used to get in defends against nobody.
CREATE TABLE IF NOT EXISTS vault (
    user       TEXT PRIMARY KEY,
    pin_hash   TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

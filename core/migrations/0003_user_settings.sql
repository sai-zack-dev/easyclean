-- Per-user, UI-editable settings. Key/value rather than columns: these are
-- small, sparse preferences that the web app adds to over time, and a new
-- preference shouldn't cost a migration. `EASYCLEAN_*` env vars remain the
-- fallback for any key a user has never set.
CREATE TABLE IF NOT EXISTS user_setting (
    user  TEXT NOT NULL,
    key   TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user, key)
);

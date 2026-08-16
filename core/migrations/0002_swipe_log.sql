CREATE TABLE IF NOT EXISTS swipe_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user       TEXT    NOT NULL,
    swiped_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_swipe_log_user_day ON swipe_log(user, swiped_at);

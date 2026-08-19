-- Waitlist signups for the Jellespelletjes Goud paid account.
CREATE TABLE goud_waitlist (
  email TEXT PRIMARY KEY,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

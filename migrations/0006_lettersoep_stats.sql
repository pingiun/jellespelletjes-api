-- Global anonymous play statistics for lettersoep/lettersoup: one row per
-- submitted play, verified server-side before insert. Feeds the end-of-game
-- score and time histograms.
CREATE TABLE lettersoep_stats (
  id         INTEGER PRIMARY KEY,
  game       TEXT NOT NULL CHECK (game IN ('lettersoep','lettersoup')),
  day        INTEGER NOT NULL,
  score      INTEGER NOT NULL,
  time_ms    INTEGER NOT NULL,
  at_max     INTEGER NOT NULL,
  bingo      INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_lettersoep_stats_game_day ON lettersoep_stats(game, day);

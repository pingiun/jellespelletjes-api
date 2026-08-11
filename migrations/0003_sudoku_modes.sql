-- Two sudokudo puzzles per day: "normal" (accessible tiers) and "expert"
-- (the original scheduled game). Existing seeded rows are the expert mode.
-- The API exposes them as two games: `sudokudo` (normal) and
-- `sudokudo-expert`.

ALTER TABLE sudoku_puzzles RENAME TO sudoku_puzzles_old;
CREATE TABLE sudoku_puzzles (
  mode              TEXT NOT NULL CHECK (mode IN ('normal','expert')),
  puzzle_number     INTEGER NOT NULL,
  date              TEXT NOT NULL,
  generator_version TEXT NOT NULL,
  difficulty        TEXT NOT NULL,
  givens            TEXT NOT NULL,
  solution          TEXT NOT NULL,
  PRIMARY KEY (mode, puzzle_number),
  UNIQUE (mode, date)
);
INSERT INTO sudoku_puzzles
  SELECT 'expert', puzzle_number, date, generator_version, difficulty, givens, solution
  FROM sudoku_puzzles_old;
DROP TABLE sudoku_puzzles_old;

-- Widen the allowed game ids on results (SQLite cannot alter a CHECK).
ALTER TABLE results RENAME TO results_old;
CREATE TABLE results (
  id                  INTEGER PRIMARY KEY,
  user_id             INTEGER NOT NULL REFERENCES users(id),
  game                TEXT NOT NULL CHECK (game IN
                        ('sudokudo','sudokudo-expert','woordle','woordle6','wordle','wordle6')),
  day                 INTEGER NOT NULL,
  payload             TEXT NOT NULL,
  verified            INTEGER NOT NULL DEFAULT 0,
  client_submitted_at TEXT,
  created_at          TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (user_id, game, day)
);
-- Pre-modes 'sudokudo' results were the expert-scheduled game.
INSERT INTO results (id, user_id, game, day, payload, verified, client_submitted_at, created_at)
  SELECT id, user_id,
         CASE WHEN game = 'sudokudo' THEN 'sudokudo-expert' ELSE game END,
         day, payload, verified, client_submitted_at, created_at
  FROM results_old;
DROP TABLE results_old;
CREATE INDEX idx_results_user_game_day ON results(user_id, game, day);

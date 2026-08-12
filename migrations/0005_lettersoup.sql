-- Add 'lettersoup' (English lettersoep) to the allowed game ids on results.
ALTER TABLE results RENAME TO results_old;
CREATE TABLE results (
  id                  INTEGER PRIMARY KEY,
  user_id             INTEGER NOT NULL REFERENCES users(id),
  game                TEXT NOT NULL CHECK (game IN
                        ('lettersoep','lettersoup','sudokudo','sudokudo-expert','woordle','woordle6','wordle','wordle6')),
  day                 INTEGER NOT NULL,
  payload             TEXT NOT NULL,
  verified            INTEGER NOT NULL DEFAULT 0,
  client_submitted_at TEXT,
  created_at          TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (user_id, game, day)
);
INSERT INTO results (id, user_id, game, day, payload, verified, client_submitted_at, created_at)
  SELECT id, user_id, game, day, payload, verified, client_submitted_at, created_at
  FROM results_old;
DROP TABLE results_old;
CREATE INDEX idx_results_user_game_day ON results(user_id, game, day);

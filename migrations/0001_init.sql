CREATE TABLE users (
  id         INTEGER PRIMARY KEY,
  email      TEXT NOT NULL UNIQUE COLLATE NOCASE,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Magic-link tokens; only the sha256 of the random token is stored.
CREATE TABLE login_tokens (
  token_hash BLOB PRIMARY KEY,
  email      TEXT NOT NULL COLLATE NOCASE,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT NOT NULL,
  used_at    TEXT
);

-- One-time SSO codes handed from a hub session to a game origin.
CREATE TABLE sso_codes (
  code_hash  BLOB PRIMARY KEY,
  user_id    INTEGER NOT NULL REFERENCES users(id),
  origin     TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT NOT NULL,
  used_at    TEXT
);

-- Opaque bearer sessions, one per origin (hub and each game separately).
CREATE TABLE sessions (
  token_hash   BLOB PRIMARY KEY,
  user_id      INTEGER NOT NULL REFERENCES users(id),
  origin       TEXT NOT NULL,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at   TEXT NOT NULL,
  last_used_at TEXT
);

CREATE TABLE results (
  id                  INTEGER PRIMARY KEY,
  user_id             INTEGER NOT NULL REFERENCES users(id),
  game                TEXT NOT NULL CHECK (game IN
                        ('sudokudo','woordle','woordle6','wordle','wordle6')),
  day                 INTEGER NOT NULL,
  payload             TEXT NOT NULL,
  verified            INTEGER NOT NULL DEFAULT 0,
  client_submitted_at TEXT,
  created_at          TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (user_id, game, day)
);
CREATE INDEX idx_results_user_game_day ON results(user_id, game, day);

-- Seeded from the sudokudo TypeScript engine; the server never generates puzzles.
CREATE TABLE sudoku_puzzles (
  puzzle_number     INTEGER PRIMARY KEY,
  date              TEXT NOT NULL UNIQUE,
  generator_version TEXT NOT NULL,
  difficulty        TEXT NOT NULL,
  givens            TEXT NOT NULL,
  solution          TEXT NOT NULL
);

-- One-time import of pre-account localStorage aggregate stats.
CREATE TABLE imported_stats (
  user_id     INTEGER NOT NULL REFERENCES users(id),
  game        TEXT NOT NULL,
  payload     TEXT NOT NULL,
  imported_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (user_id, game)
);

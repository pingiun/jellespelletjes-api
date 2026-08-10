# jellespelletjes-api

Accounts and score sync for [woordle.nl](https://woordle.nl) and
[sudokudo.nl](https://sudokudo.nl), serving `api.jellespelletjes.nl`.
Rust (axum + sqlx) with SQLite; passwordless magic-link auth via Resend;
SSO across the game domains through the hub at
[jellespelletjes.nl](https://jellespelletjes.nl).

## Design in one paragraph

Players play anonymously (localStorage) as always; an account is an optional
upgrade for syncing scores. Login happens once on the hub (email magic link);
games obtain their own per-origin sessions via a one-time SSO code exchange, so
the second game never needs another email. Results are verified server-side:
sudoku puzzles are pre-seeded from the sudokudo TypeScript engine (the server
never generates puzzles), and woordle results are checked against the embedded
word lists with the daily word derived deterministically from the date. Failed
verification is rejected (422), never stored.

## Endpoints

```
POST /auth/magic-link            {email}            -> 204 (email sent; dev mode logs it)
POST /auth/magic-link/consume    {token}            -> {token, user}   hub session
POST /auth/sso-code              {origin}   [hub]   -> {code}
POST /auth/sso-code/consume      {code}             -> {token, user}   game session
GET  /me                                    [auth]  -> {user, origin}
POST /logout                                [auth]  -> 204
DELETE /me                                  [auth]  -> 204 (full account deletion)
PUT  /results/{game}/{day}       payload    [auth]  -> 201 verified | 200 idempotent | 409 conflict | 422 rejected
GET  /results?game=&since_day=              [auth]  -> {results: [...]}
POST /import/{game}              stats JSON [auth]  -> 204 (once per game)
GET  /profile                               [auth]  -> per-game stats + imported baseline
GET  /healthz                                       -> {ok, sudoku_seeded_until}
```

Games: `sudokudo`, `woordle`, `woordle6`, `wordle`, `wordle6`.
`day` is the sudoku puzzle number, or the woordle day offset since the
variant's start date (pre-modulo; ±1 day accepted for timezones).

## Development

```sh
cargo test          # unit + integration (in-memory SQLite)
cargo run -- serve  # dev server on 127.0.0.1:8080; magic links logged, not emailed
```

Configuration via env: `DATABASE_URL`, `LISTEN_ADDR`, `PUBLIC_HUB_URL`,
`ALLOWED_ORIGINS`, `RESEND_API_KEY` (absent = dev mode), `EMAIL_FROM`.

## Seeding sudoku puzzles

In the sudokudo repo:

```sh
npx tsx scripts/seed-puzzles.ts 2026-08-31 2027-12-31 \
  | ssh vps 'jellespelletjes-api seed-sudoku'
```

The seeder refuses to modify existing rows; `/healthz` reports the seeding
horizon. Re-seed before it runs out.

## Word lists

`data/woordle/` mirrors the woordle repo's `data/` files and is embedded in the
binary. `MANIFEST.sha256` is checked in CI: updating the lists is a deliberate
two-file change here after changing them in woordle.

## Deployment

Hetzner VPS, Debian: static musl binary + systemd (`deploy/…service`), Caddy
for TLS (`deploy/Caddyfile`), Litestream replicating the SQLite file to R2.
CI (`.github/workflows/deploy.yml`) tests, builds, and deploys on push to main
via a scoped deploy script (`deploy/deploy-jellespelletjes-api`).

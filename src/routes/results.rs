use crate::auth::Session;
use crate::games::{self, lettersoep, sudokudo, woordle};
use crate::SharedState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(json!({ "error": message })))
}

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    tracing::error!("internal error: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// Today's sudoku puzzle number, derived from the seeded rows (the server has
/// no knowledge of the epoch — the seed data is the source of truth).
async fn today_sudoku_number(pool: &sqlx::SqlitePool, mode: &str) -> anyhow::Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT puzzle_number FROM sudoku_puzzles
         WHERE mode = ? AND date <= date('now') ORDER BY date DESC LIMIT 1",
    )
    .bind(mode)
    .fetch_optional(pool)
    .await?)
}

/// Map an API game id to a puzzle-table mode ("sudokudo" is the normal game).
fn sudoku_mode(game: &str) -> Option<&'static str> {
    match game {
        "sudokudo" => Some("normal"),
        "sudokudo-expert" => Some("expert"),
        _ => None,
    }
}

pub async fn put_result(
    State(state): State<SharedState>,
    session: Session,
    Path((game, day)): Path<(String, i64)>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if !games::is_known_game(&game) {
        return Err(err(StatusCode::NOT_FOUND, "unknown game"));
    }

    let payload = if game == "lettersoep" {
        let submission: lettersoep::LettersoepSubmission =
            serde_json::from_value(body).map_err(|_| err(StatusCode::BAD_REQUEST, "malformed submission"))?;
        // Verification may generate the day's puzzle (~1s of CPU).
        tokio::task::spawn_blocking(move || lettersoep::verify(day, &submission))
            .await
            .map_err(internal)?
            .map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.message()))?
    } else {
        match sudoku_mode(&game) {
        Some(mode) => {
            let submission: sudokudo::SudokuSubmission =
                serde_json::from_value(body).map_err(|_| err(StatusCode::BAD_REQUEST, "malformed submission"))?;
            let seeded: Option<(String, String, String)> = sqlx::query_as(
                "SELECT generator_version, difficulty, solution
                 FROM sudoku_puzzles WHERE mode = ? AND puzzle_number = ?",
            )
            .bind(mode)
            .bind(day)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal)?;
            let seeded = seeded.map(|(v, d, s)| sudokudo::SeededPuzzle {
                generator_version: v,
                difficulty: d,
                solution: s,
            });
            let today = today_sudoku_number(&state.pool, mode)
                .await
                .map_err(internal)?
                .unwrap_or(i64::MIN);
            sudokudo::verify(seeded.as_ref(), today, day, &submission)
                .map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.message()))?
        }
        None => {
            let submission: woordle::WoordleSubmission =
                serde_json::from_value(body).map_err(|_| err(StatusCode::BAD_REQUEST, "malformed submission"))?;
            woordle::verify(&game, day, &submission)
                .map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.message()))?
        }
        }
    };

    // First write wins; an identical re-PUT is fine, a conflicting one is 409.
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT payload FROM results WHERE user_id = ? AND game = ? AND day = ?",
    )
    .bind(session.user_id)
    .bind(&game)
    .bind(day)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;
    if let Some(existing) = existing {
        let stored: serde_json::Value = serde_json::from_str(&existing).unwrap_or_default();
        return if stored == payload {
            Ok((StatusCode::OK, Json(json!({ "verified": true, "existing": true }))))
        } else {
            Err(err(StatusCode::CONFLICT, "a different result for this day already exists"))
        };
    }

    sqlx::query(
        "INSERT INTO results (user_id, game, day, payload, verified, client_submitted_at)
         VALUES (?, ?, ?, ?, 1, datetime('now'))",
    )
    .bind(session.user_id)
    .bind(&game)
    .bind(day)
    .bind(payload.to_string())
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    Ok((StatusCode::CREATED, Json(json!({ "verified": true }))))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub game: Option<String>,
    pub since_day: Option<i64>,
}

pub async fn list_results(
    State(state): State<SharedState>,
    session: Session,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows: Vec<(String, i64, String, i64)> = sqlx::query_as(
        "SELECT game, day, payload, verified FROM results
         WHERE user_id = ?1
           AND (?2 IS NULL OR game = ?2)
           AND (?3 IS NULL OR day >= ?3)
         ORDER BY game, day",
    )
    .bind(session.user_id)
    .bind(&query.game)
    .bind(query.since_day)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let results: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(game, day, payload, verified)| {
            json!({
                "game": game,
                "day": day,
                "payload": serde_json::from_str::<serde_json::Value>(&payload).unwrap_or_default(),
                "verified": verified == 1,
            })
        })
        .collect();
    Ok(Json(json!({ "results": results })))
}

pub async fn import_stats(
    State(state): State<SharedState>,
    session: Session,
    Path(game): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, ApiError> {
    if !games::is_known_game(&game) {
        return Err(err(StatusCode::NOT_FOUND, "unknown game"));
    }
    if body.to_string().len() > 16 * 1024 {
        return Err(err(StatusCode::BAD_REQUEST, "payload too large"));
    }
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO imported_stats (user_id, game, payload) VALUES (?, ?, ?)",
    )
    .bind(session.user_id)
    .bind(&game)
    .bind(body.to_string())
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    if inserted.rows_affected() == 0 {
        return Err(err(StatusCode::CONFLICT, "stats already imported for this game"));
    }
    Ok(StatusCode::NO_CONTENT)
}

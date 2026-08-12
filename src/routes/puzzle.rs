//! Serving the lettersoep daily puzzle.
//!
//! Generation is deterministic but costs ~1s, so results are cached in
//! memory by date. Only a narrow date window around the server's own day is
//! served: enough for every timezone, no free compute for date scanning.

use crate::games::lettersoep;
use crate::SharedState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDate, Utc};
use serde_json::json;

pub async fn lettersoep_puzzle(
    State(_state): State<SharedState>,
    Path((game, date)): Path<(String, String)>,
) -> Response {
    let Some(lang) = lettersoep::Lang::from_game(&game) else {
        return (StatusCode::NOT_FOUND, "unknown game").into_response();
    };
    let Ok(date) = date.parse::<NaiveDate>() else {
        return (StatusCode::BAD_REQUEST, "invalid date").into_response();
    };
    let today = Utc::now().date_naive();
    if (date - today).num_days().abs() > 2 {
        return (StatusCode::NOT_FOUND, "date out of range").into_response();
    }

    // Generation takes ~1s of CPU on a cache miss: do it off the runtime.
    // (The cache in games::lettersoep is shared with result verification.)
    let generated = tokio::task::spawn_blocking(move || {
        lettersoep::cached_puzzle(date, lang).and_then(|p| Ok(serde_json::to_string(&*p)?))
    })
    .await;
    match generated {
        Ok(Ok(json)) => json_response(json),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "generation failed").into_response(),
    }
}

fn json_response(json: String) -> Response {
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/json"),
            // The puzzle for a date never changes.
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        json,
    )
        .into_response()
}


/// Record an anonymous play for the global histograms. The submission is
/// verified exactly like an account result (legal move, real rack, known
/// words), so junk can't reach the stats — only replays can, which is
/// acceptable noise for a daily game.
pub async fn record_stats(
    State(state): State<SharedState>,
    Path((game, day)): Path<(String, i64)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(lang) = lettersoep::Lang::from_game(&game) else {
        return (StatusCode::NOT_FOUND, "unknown game").into_response();
    };
    let Ok(submission) = serde_json::from_value::<lettersoep::LettersoepSubmission>(body) else {
        return (StatusCode::BAD_REQUEST, "malformed submission").into_response();
    };
    let verified = tokio::task::spawn_blocking(move || lettersoep::verify(lang, day, &submission)).await;
    let payload = match verified {
        Ok(Ok(payload)) => payload,
        Ok(Err(e)) => return (StatusCode::UNPROCESSABLE_ENTITY, e.message()).into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "verification failed").into_response(),
    };
    let insert = sqlx::query(
        "INSERT INTO lettersoep_stats (game, day, score, time_ms, at_max, bingo)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&game)
    .bind(day)
    .bind(payload["score"].as_i64())
    .bind(payload["time_ms"].as_i64())
    .bind(payload["at_max"].as_bool().unwrap_or(false))
    .bind(payload["bingo"].as_bool().unwrap_or(false))
    .execute(&state.pool)
    .await;
    match insert {
        Ok(_) => (StatusCode::CREATED, Json(json!({ "recorded": true }))).into_response(),
        Err(e) => {
            tracing::error!("stats insert: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "storage failed").into_response()
        }
    }
}

/// The day's global distributions, raw: the client bins them for display.
pub async fn get_stats(
    State(state): State<SharedState>,
    Path((game, day)): Path<(String, i64)>,
) -> Response {
    if lettersoep::Lang::from_game(&game).is_none() {
        return (StatusCode::NOT_FOUND, "unknown game").into_response();
    }
    let rows: Result<Vec<(i64, i64, bool)>, _> = sqlx::query_as(
        "SELECT score, time_ms, at_max FROM lettersoep_stats
         WHERE game = ? AND day = ? ORDER BY id LIMIT 10000",
    )
    .bind(&game)
    .bind(day)
    .fetch_all(&state.pool)
    .await;
    match rows {
        Ok(rows) => {
            let scores: Vec<i64> = rows.iter().map(|r| r.0).collect();
            let times_ms: Vec<i64> = rows.iter().map(|r| r.1).collect();
            let at_max = rows.iter().filter(|r| r.2).count();
            (
                StatusCode::OK,
                Json(json!({
                    "count": rows.len(),
                    "atMax": at_max,
                    "scores": scores,
                    "timesMs": times_ms,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("stats query: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

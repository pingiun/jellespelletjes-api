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
use chrono::{NaiveDate, Utc};

pub async fn lettersoep_puzzle(
    State(_state): State<SharedState>,
    Path(date): Path<String>,
) -> Response {
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
        lettersoep::cached_puzzle(date).and_then(|p| Ok(serde_json::to_string(&*p)?))
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

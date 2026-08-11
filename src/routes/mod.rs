pub mod auth;
pub mod profile;
pub mod results;

use crate::SharedState;
use axum::extract::State;
use axum::Json;

pub async fn healthz(State(state): State<SharedState>) -> Json<serde_json::Value> {
    // The horizon that matters is the mode that runs out FIRST.
    let seeded_until: Option<String> = sqlx::query_scalar(
        "SELECT MIN(m) FROM (SELECT MAX(date) AS m FROM sudoku_puzzles GROUP BY mode)",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);
    Json(serde_json::json!({ "ok": true, "sudoku_seeded_until": seeded_until }))
}

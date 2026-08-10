pub mod auth;
pub mod profile;
pub mod results;

use crate::SharedState;
use axum::extract::State;
use axum::Json;

pub async fn healthz(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let seeded_until: Option<String> =
        sqlx::query_scalar("SELECT MAX(date) FROM sudoku_puzzles")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(None);
    Json(serde_json::json!({ "ok": true, "sudoku_seeded_until": seeded_until }))
}

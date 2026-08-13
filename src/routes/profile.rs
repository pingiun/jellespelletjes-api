use crate::auth::Session;
use crate::games::{self, woordle};
use crate::{stats, SharedState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    tracing::error!("internal error: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal error" })))
}

pub async fn profile(
    State(state): State<SharedState>,
    session: Session,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT game, day, payload FROM results WHERE user_id = ?")
            .bind(session.user_id)
            .fetch_all(&state.pool)
            .await
            .map_err(internal)?;
    let imported: Vec<(String, String)> =
        sqlx::query_as("SELECT game, payload FROM imported_stats WHERE user_id = ?")
            .bind(session.user_id)
            .fetch_all(&state.pool)
            .await
            .map_err(internal)?;

    let mut today_sudoku = std::collections::HashMap::new();
    for mode in ["normal", "expert"] {
        let n: Option<i64> = sqlx::query_scalar(
            "SELECT puzzle_number FROM sudoku_puzzles
             WHERE mode = ? AND date <= date('now') ORDER BY date DESC LIMIT 1",
        )
        .bind(mode)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?;
        today_sudoku.insert(mode, n);
    }

    let mut per_game = serde_json::Map::new();
    for game in games::GAMES {
        let game_rows: Vec<stats::ResultRow> = rows
            .iter()
            .filter(|(g, _, _)| g == game)
            .map(|(_, day, payload)| stats::ResultRow {
                day: *day,
                payload: serde_json::from_str(payload).unwrap_or_default(),
            })
            .collect();
        let today_day = match *game {
            "sudokudo" => today_sudoku.get("normal").copied().flatten(),
            "sudokudo-expert" => today_sudoku.get("expert").copied().flatten(),
            g => woordle::variant(g).map(woordle::utc_day),
        };
        let imported_payload: Option<serde_json::Value> = imported
            .iter()
            .find(|(g, _)| g == game)
            .and_then(|(_, baseline)| serde_json::from_str(baseline).ok());
        let baseline = imported_payload.as_ref().map(stats::baseline_from_import);
        let mut entry = stats::game_stats(game, &game_rows, today_day, baseline);
        if let Some(payload) = imported_payload {
            entry["imported_baseline"] = payload;
        }
        per_game.insert(game.to_string(), entry);
    }

    Ok(Json(json!({
        "user": { "id": session.user_id, "email": session.email },
        "games": per_game,
    })))
}

/// GDPR: delete the account and everything attached to it.
pub async fn delete_me(
    State(state): State<SharedState>,
    session: Session,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await.map_err(internal)?;
    for table in ["results", "imported_stats", "sessions", "sso_codes"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id = ?"))
            .bind(session.user_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }
    sqlx::query("DELETE FROM login_tokens WHERE email = ?")
        .bind(&session.email)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(session.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

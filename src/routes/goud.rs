use crate::SharedState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

#[derive(Deserialize)]
pub struct WaitlistRequest {
    pub email: String,
}

/// Join the Goud waitlist. Unauthenticated; idempotent per email.
pub async fn join_waitlist(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<WaitlistRequest>,
) -> Result<StatusCode, ApiError> {
    let address = body.email.trim().to_lowercase();
    if !address.contains('@') || address.len() > 254 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid email address"));
    }
    // The trustworthy client IP is the LAST X-Forwarded-For entry: that one
    // was appended by our own reverse proxy; earlier entries are client-set.
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next_back())
        .unwrap_or("local")
        .trim()
        .to_string();
    if !state.rate_limiter.allow(&format!("waitlist:{ip}"), 10, 3600) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many signups, try later"));
    }
    sqlx::query("INSERT OR IGNORE INTO goud_waitlist (email) VALUES (?)")
        .bind(&address)
        .execute(&state.pool)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "database error"))?;
    Ok(StatusCode::NO_CONTENT)
}

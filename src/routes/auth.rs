use crate::auth::{self, Session};
use crate::{email, SharedState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Duration;
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

#[derive(Deserialize)]
pub struct MagicLinkRequest {
    pub email: String,
}

pub async fn request_magic_link(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<MagicLinkRequest>,
) -> Result<StatusCode, ApiError> {
    let address = body.email.trim().to_lowercase();
    if !address.contains('@') || address.len() > 254 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid email address"));
    }
    // Client IP via the reverse proxy; absent in local dev (one shared bucket).
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("local")
        .trim()
        .to_string();
    if !state.rate_limiter.allow(&format!("ip:{ip}"), 10, 3600)
        || !state.rate_limiter.allow(&format!("email:{address}"), 3, 3600)
    {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many login emails, try later"));
    }
    let (raw, hash) = auth::new_token();
    let expires = auth::iso(auth::now() + Duration::minutes(auth::MAGIC_LINK_MINUTES));
    sqlx::query("INSERT INTO login_tokens (token_hash, email, expires_at) VALUES (?, ?, ?)")
        .bind(&hash)
        .bind(&address)
        .bind(&expires)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    let link = format!("{}/login?token={raw}", state.config.hub_url);
    email::send_magic_link(&state, &address, &link)
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ConsumeTokenRequest {
    pub token: String,
}

pub async fn consume_magic_link(
    State(state): State<SharedState>,
    Json(body): Json<ConsumeTokenRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let hash = auth::hash_token(&body.token);
    let now = auth::iso(auth::now());
    // Atomically claim the token: only one consume can succeed.
    let claimed = sqlx::query(
        "UPDATE login_tokens SET used_at = ?
         WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(&now)
    .bind(&hash)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    if claimed.rows_affected() != 1 {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid or expired login link"));
    }
    let address: String = sqlx::query_scalar("SELECT email FROM login_tokens WHERE token_hash = ?")
        .bind(&hash)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    sqlx::query("INSERT OR IGNORE INTO users (email) VALUES (?)")
        .bind(&address)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    let (user_id, email): (i64, String) =
        sqlx::query_as("SELECT id, email FROM users WHERE email = ?")
            .bind(&address)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
    let token = auth::create_session(&state.pool, user_id, &state.config.hub_url)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "token": token, "user": { "id": user_id, "email": email } })))
}

#[derive(Deserialize)]
pub struct SsoCodeRequest {
    pub origin: String,
}

pub async fn create_sso_code(
    State(state): State<SharedState>,
    session: Session,
    Json(body): Json<SsoCodeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Only a hub session may mint SSO codes, and only for known origins.
    if session.origin != state.config.hub_url {
        return Err(err(StatusCode::FORBIDDEN, "sso codes can only be created from the hub"));
    }
    let origin = body.origin.trim_end_matches('/').to_string();
    if !state.config.allowed_origins.contains(&origin) {
        return Err(err(StatusCode::BAD_REQUEST, "unknown origin"));
    }
    let (raw, hash) = auth::new_token();
    let expires = auth::iso(auth::now() + Duration::seconds(auth::SSO_CODE_SECONDS));
    sqlx::query("INSERT INTO sso_codes (code_hash, user_id, origin, expires_at) VALUES (?, ?, ?, ?)")
        .bind(&hash)
        .bind(session.user_id)
        .bind(&origin)
        .bind(&expires)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "code": raw })))
}

#[derive(Deserialize)]
pub struct ConsumeSsoRequest {
    pub code: String,
}

pub async fn consume_sso_code(
    State(state): State<SharedState>,
    Json(body): Json<ConsumeSsoRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let hash = auth::hash_token(&body.code);
    let now = auth::iso(auth::now());
    let claimed = sqlx::query(
        "UPDATE sso_codes SET used_at = ?
         WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(&now)
    .bind(&hash)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    if claimed.rows_affected() != 1 {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid or expired code"));
    }
    let (user_id, origin): (i64, String) =
        sqlx::query_as("SELECT user_id, origin FROM sso_codes WHERE code_hash = ?")
            .bind(&hash)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let token = auth::create_session(&state.pool, user_id, &origin)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "token": token, "user": { "id": user_id, "email": email } })))
}

pub async fn me(session: Session) -> Json<serde_json::Value> {
    Json(json!({
        "user": { "id": session.user_id, "email": session.email },
        "origin": session.origin,
    }))
}

pub async fn logout(
    State(state): State<SharedState>,
    session: Session,
) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(&session.token_hash)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

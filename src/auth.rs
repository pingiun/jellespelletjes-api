use crate::SharedState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Generate a 256-bit random token; returns (raw hex token, sha256 of the hex string).
pub fn new_token() -> (String, Vec<u8>) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let raw = hex(&bytes);
    let hash = hash_token(&raw);
    (raw, hash)
}

pub fn hash_token(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn iso(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub const SESSION_DAYS: i64 = 180;
pub const MAGIC_LINK_MINUTES: i64 = 15;
pub const SSO_CODE_SECONDS: i64 = 60;

pub async fn create_session(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    origin: &str,
) -> anyhow::Result<String> {
    let (raw, hash) = new_token();
    let expires = iso(now() + Duration::days(SESSION_DAYS));
    sqlx::query("INSERT INTO sessions (token_hash, user_id, origin, expires_at) VALUES (?, ?, ?, ?)")
        .bind(&hash)
        .bind(user_id)
        .bind(origin)
        .bind(&expires)
        .execute(pool)
        .await?;
    Ok(raw)
}

/// Authenticated request context, extracted from the Authorization header.
pub struct Session {
    pub user_id: i64,
    pub email: String,
    pub origin: String,
    pub token_hash: Vec<u8>,
}

impl FromRequestParts<SharedState> for Session {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let unauthorized = (StatusCode::UNAUTHORIZED, "invalid or expired session");
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(unauthorized)?;
        let hash = hash_token(header);
        let row: Option<(i64, String, String, String)> = sqlx::query_as(
            "SELECT s.user_id, u.email, s.origin, s.expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ?",
        )
        .bind(&hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "database error"))?;
        let (user_id, email, origin, expires_at) = row.ok_or(unauthorized)?;
        if expires_at < iso(now()) {
            return Err(unauthorized);
        }
        // Sliding expiry: refresh on use (fire and forget).
        let refresh = iso(now() + Duration::days(SESSION_DAYS));
        let last = iso(now());
        let pool = state.pool.clone();
        let h = hash.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "UPDATE sessions SET expires_at = ?, last_used_at = ? WHERE token_hash = ?",
            )
            .bind(refresh)
            .bind(last)
            .bind(h)
            .execute(&pool)
            .await;
        });
        Ok(Session { user_id, email, origin, token_hash: hash })
    }
}

/// Minimal in-memory rate limiter (single-process deployment).
#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    /// Returns true if the action is allowed for `key` (at most `max` per `window_secs`).
    pub fn allow(&self, key: &str, max: usize, window_secs: u64) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let entries = buckets.entry(key.to_string()).or_default();
        let cutoff = Instant::now() - std::time::Duration::from_secs(window_secs);
        entries.retain(|t| *t > cutoff);
        if entries.len() >= max {
            return false;
        }
        entries.push(Instant::now());
        true
    }
}

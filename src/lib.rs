pub mod auth;
pub mod config;
pub mod db;
pub mod email;
pub mod games;
pub mod routes;
pub mod seed;
pub mod stats;

use axum::http::{HeaderValue, Method};
use axum::routing::{get, post, put};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
    pub http: reqwest::Client,
    pub rate_limiter: auth::RateLimiter,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    let origins: Vec<HeaderValue> = state
        .config
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([axum::http::header::AUTHORIZATION, axum::http::header::CONTENT_TYPE]);

    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/puzzle/{game}/{date}", get(routes::puzzle::lettersoep_puzzle))
        .route(
            "/stats/{game}/{day}",
            get(routes::puzzle::get_stats).post(routes::puzzle::record_stats),
        )
        .route("/auth/magic-link", post(routes::auth::request_magic_link))
        .route("/auth/magic-link/consume", post(routes::auth::consume_magic_link))
        .route("/auth/code/consume", post(routes::auth::consume_code))
        .route("/auth/code/status", post(routes::auth::code_status))
        .route("/auth/sso-code", post(routes::auth::create_sso_code))
        .route("/auth/sso-code/consume", post(routes::auth::consume_sso_code))
        .route("/me", get(routes::auth::me).delete(routes::profile::delete_me))
        .route("/logout", post(routes::auth::logout))
        .route("/results/{game}/{day}", put(routes::results::put_result))
        .route("/results", get(routes::results::list_results))
        .route("/import/{game}", post(routes::results::import_stats))
        .route("/profile", get(routes::profile::profile))
        .route("/goud/waitlist", post(routes::goud::join_waitlist))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

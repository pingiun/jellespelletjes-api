//! End-to-end flow test: magic link → hub session → SSO code → game session →
//! result submission → profile, against an in-memory database.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use jellespelletjes_api::{auth, config::Config, db, router, AppState, SharedState};
use std::sync::Arc;
use tower::ServiceExt;

async fn test_state() -> SharedState {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(),
        listen_addr: "127.0.0.1:0".into(),
        hub_url: "https://jellespelletjes.nl".into(),
        allowed_origins: vec![
            "https://jellespelletjes.nl".into(),
            "https://sudokudo.nl".into(),
            "https://woordle.nl".into(),
        ],
        resend_api_key: None, // dev mode: magic links are logged, not sent
        email_from: "test@example.com".into(),
    };
    Arc::new(AppState {
        pool,
        config,
        http: reqwest::Client::new(),
        rate_limiter: auth::RateLimiter::default(),
    })
}

async fn call(
    state: &SharedState,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = if let Some(body) = body {
        request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        request.body(Body::empty()).unwrap()
    };
    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Create a login token directly in the DB (simulating the emailed link).
async fn make_login_token(state: &SharedState, email: &str) -> String {
    let (raw, hash) = auth::new_token();
    let expires = auth::iso(auth::now() + chrono::Duration::minutes(15));
    sqlx::query("INSERT INTO login_tokens (token_hash, email, expires_at) VALUES (?, ?, ?)")
        .bind(&hash)
        .bind(email)
        .bind(&expires)
        .execute(&state.pool)
        .await
        .unwrap();
    raw
}

async fn seed_puzzle(state: &SharedState, number: i64, date: &str, solution: &str) {
    sqlx::query(
        "INSERT INTO sudoku_puzzles (mode, puzzle_number, date, generator_version, difficulty, givens, solution)
         VALUES ('normal', ?, ?, 'v2', 'beginner', ?, ?)",
    )
    .bind(number)
    .bind(date)
    .bind("0".repeat(81))
    .bind(solution)
    .execute(&state.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn full_flow() {
    let state = test_state().await;

    // healthz works and reports no seeding.
    let (status, body) = call(&state, "GET", "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sudoku_seeded_until"], serde_json::Value::Null);

    // magic link request is accepted (dev mode).
    let (status, _) = call(
        &state,
        "POST",
        "/auth/magic-link",
        None,
        Some(serde_json::json!({ "email": "jelle@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // consume a link → hub session.
    let raw = make_login_token(&state, "jelle@example.com").await;
    let (status, body) = call(
        &state,
        "POST",
        "/auth/magic-link/consume",
        None,
        Some(serde_json::json!({ "token": raw })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let hub_token = body["token"].as_str().unwrap().to_string();
    assert_eq!(body["user"]["email"], "jelle@example.com");

    // second consume of the same link fails.
    let (status, _) = call(
        &state,
        "POST",
        "/auth/magic-link/consume",
        None,
        Some(serde_json::json!({ "token": raw })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // /me with the hub session.
    let (status, body) = call(&state, "GET", "/me", Some(&hub_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["origin"], "https://jellespelletjes.nl");

    // SSO code for sudokudo.nl → game session.
    let (status, body) = call(
        &state,
        "POST",
        "/auth/sso-code",
        Some(&hub_token),
        Some(serde_json::json!({ "origin": "https://sudokudo.nl" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let code = body["code"].as_str().unwrap().to_string();
    let (status, body) = call(
        &state,
        "POST",
        "/auth/sso-code/consume",
        None,
        Some(serde_json::json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let game_token = body["token"].as_str().unwrap().to_string();

    // a game session may not mint SSO codes.
    let (status, _) = call(
        &state,
        "POST",
        "/auth/sso-code",
        Some(&game_token),
        Some(serde_json::json!({ "origin": "https://woordle.nl" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // submit today's sudoku result.
    let solution = "123456789".repeat(9);
    seed_puzzle(&state, 1, &chrono::Utc::now().format("%Y-%m-%d").to_string(), &solution).await;
    let submission = serde_json::json!({
        "solution": solution,
        "started_at_ms": 1_700_000_000_000i64,
        "finished_at_ms": 1_700_000_300_000i64,
        "generator_version": "v2",
    });
    let (status, body) = call(
        &state,
        "PUT",
        "/results/sudokudo/1",
        Some(&game_token),
        Some(submission.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["verified"], true);

    // identical re-PUT is idempotent; tampered solution is rejected.
    let (status, _) = call(&state, "PUT", "/results/sudokudo/1", Some(&game_token), Some(submission)).await;
    assert_eq!(status, StatusCode::OK);
    let bad = serde_json::json!({
        "solution": "987654321".repeat(9),
        "started_at_ms": 1_700_000_000_000i64,
        "finished_at_ms": 1_700_000_300_000i64,
        "generator_version": "v2",
    });
    let (status, _) = call(&state, "PUT", "/results/sudokudo/1", Some(&game_token), Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // import + profile.
    let (status, _) = call(
        &state,
        "POST",
        "/import/sudokudo",
        Some(&game_token),
        Some(serde_json::json!({ "gamesPlayed": 12, "gamesWon": 11 })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = call(&state, "GET", "/profile", Some(&game_token), None).await;
    assert_eq!(status, StatusCode::OK);
    // Imported baseline counts merge into the totals (1 verified + 12 imported).
    assert_eq!(body["games"]["sudokudo"]["played"], 13);
    assert_eq!(body["games"]["sudokudo"]["won"], 12);
    assert_eq!(body["games"]["sudokudo"]["current_streak"], 1);
    assert_eq!(body["games"]["sudokudo"]["imported_baseline"]["gamesPlayed"], 12);

    // logout is single sign-off by default: BOTH the game and hub sessions die.
    let (status, _) = call(&state, "POST", "/logout", Some(&game_token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&state, "GET", "/me", Some(&game_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&state, "GET", "/me", Some(&hub_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_only_this_device() {
    let state = test_state().await;
    let raw = make_login_token(&state, "so@example.com").await;
    let (_, body) = call(
        &state,
        "POST",
        "/auth/magic-link/consume",
        None,
        Some(serde_json::json!({ "token": raw })),
    )
    .await;
    let hub_token = body["token"].as_str().unwrap().to_string();
    let (_, body) = call(
        &state,
        "POST",
        "/auth/sso-code",
        Some(&hub_token),
        Some(serde_json::json!({ "origin": "https://sudokudo.nl" })),
    )
    .await;
    let (_, body) = call(
        &state,
        "POST",
        "/auth/sso-code/consume",
        None,
        Some(serde_json::json!({ "code": body["code"] })),
    )
    .await;
    let game_token = body["token"].as_str().unwrap().to_string();

    // Scoped logout ends only the game session; the hub session survives.
    let (status, _) = call(
        &state,
        "POST",
        "/logout",
        Some(&game_token),
        Some(serde_json::json!({ "only_this_device": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&state, "GET", "/me", Some(&game_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&state, "GET", "/me", Some(&hub_token), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn cross_device_login_flow() {
    let state = test_state().await;

    // Device A requests a link with its request id.
    let (status, _) = call(
        &state,
        "POST",
        "/auth/magic-link",
        None,
        Some(serde_json::json!({ "email": "cross@example.com", "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Grab the raw token by replaying insertion: instead, insert our own with a request hash.
    let (raw, hash) = auth::new_token();
    let expires = auth::iso(auth::now() + chrono::Duration::minutes(15));
    sqlx::query(
        "INSERT INTO login_tokens (token_hash, email, expires_at, request_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(&hash)
    .bind("cross@example.com")
    .bind(&expires)
    .bind(auth::hash_token("device-a-nonce"))
    .execute(&state.pool)
    .await
    .unwrap();

    // Before the link is opened elsewhere, the waiting device sees no code.
    let (status, body) = call(
        &state,
        "POST",
        "/auth/code/status",
        None,
        Some(serde_json::json!({ "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code_issued"], false);

    // Device B (no request id) opens the link: gets a code, not a session.
    let (status, body) = call(
        &state,
        "POST",
        "/auth/magic-link/consume",
        None,
        Some(serde_json::json!({ "token": raw })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cross_device"], true);
    let code = body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert!(body.get("token").is_none());

    // Now the waiting device's poll reports the issued code.
    let (_, body) = call(
        &state,
        "POST",
        "/auth/code/status",
        None,
        Some(serde_json::json!({ "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(body["code_issued"], true);

    // Wrong code fails and counts an attempt.
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let (status, _) = call(
        &state,
        "POST",
        "/auth/code/consume",
        None,
        Some(serde_json::json!({ "code": wrong, "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Wrong request id (a third device) cannot redeem the right code.
    let (status, _) = call(
        &state,
        "POST",
        "/auth/code/consume",
        None,
        Some(serde_json::json!({ "code": code, "request_id": "device-c-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Device A redeems the code and gets a session.
    let (status, body) = call(
        &state,
        "POST",
        "/auth/code/consume",
        None,
        Some(serde_json::json!({ "code": code, "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], "cross@example.com");
    let token = body["token"].as_str().unwrap().to_string();
    let (status, _) = call(&state, "GET", "/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);

    // The code is single-use.
    let (status, _) = call(
        &state,
        "POST",
        "/auth/code/consume",
        None,
        Some(serde_json::json!({ "code": code, "request_id": "device-a-nonce" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn woordle_submission_flow() {
    use jellespelletjes_api::games::woordle;
    let state = test_state().await;
    let raw = make_login_token(&state, "w@example.com").await;
    let (_, body) = call(
        &state,
        "POST",
        "/auth/magic-link/consume",
        None,
        Some(serde_json::json!({ "token": raw })),
    )
    .await;
    let token = body["token"].as_str().unwrap().to_string();

    let v = woordle::variant("woordle").unwrap();
    let day = woordle::utc_day(v);
    let solution = woordle::solution_for_day(v, day);
    let (status, body) = call(
        &state,
        "PUT",
        &format!("/results/woordle/{day}"),
        Some(&token),
        Some(serde_json::json!({ "guesses": [solution], "won": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // invalid word rejected with 422.
    let day6 = woordle::utc_day(woordle::variant("woordle6").unwrap());
    let (status, _) = call(
        &state,
        "PUT",
        &format!("/results/woordle6/{day6}"),
        Some(&token),
        Some(serde_json::json!({ "guesses": ["zzzzzz"], "won": true })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

use axum::{
    extract::{Json, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use crate::{security, AppState};

#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct ErrorMessage { error: &'static str }

#[derive(Serialize)]
struct UserResponse { email: String }

#[derive(Serialize)]
struct OkResponse { ok: bool }

fn json<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn err(status: StatusCode, message: &'static str) -> Response {
    json(status, ErrorMessage { error: message })
}

fn cookie_response<T: Serialize>(status: StatusCode, body: T, cookie: &str) -> Response {
    let mut response = json(status, body);
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn issue_session(state: &AppState, user_id: i64, email: String, status: StatusCode) -> Response {
    let (token, hashed) = security::make_session();
    let stored = state.db.execute(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at) VALUES ($1, $2, now() + INTERVAL '12 hours')",
        &[&hashed, &user_id],
    ).await;
    match stored {
        Ok(1) => cookie_response(status, UserResponse { email }, &security::session_cookie(&token)),
        _ => err(StatusCode::INTERNAL_SERVER_ERROR, "Unable to start session"),
    }
}

pub async fn register(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<Credentials>) -> Response {
    if !security::valid_origin(&headers) { return err(StatusCode::FORBIDDEN, "Invalid origin"); }
    let Some(email) = security::normalize_email(&input.email) else { return err(StatusCode::BAD_REQUEST, "Invalid email"); };
    if !security::valid_password(&input.password) { return err(StatusCode::BAD_REQUEST, "Password must be 10–128 characters"); }

    let hash = match tokio::task::spawn_blocking(move || security::password_hash(&input.password)).await {
        Ok(Ok(h)) => h,
        _ => return err(StatusCode::INTERNAL_SERVER_ERROR, "Registration unavailable"),
    };
    let inserted = state.db.query_opt(
        "INSERT INTO auth_users(email, password_hash) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING RETURNING id",
        &[&email, &hash],
    ).await;
    match inserted {
        Ok(Some(row)) => issue_session(&state, row.get::<_, i64>(0), email, StatusCode::CREATED).await,
        Ok(None) => err(StatusCode::CONFLICT, "Email already registered"),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "Registration unavailable"),
    }
}

pub async fn login(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<Credentials>) -> Response {
    if !security::valid_origin(&headers) { return err(StatusCode::FORBIDDEN, "Invalid origin"); }
    let Some(email) = security::normalize_email(&input.email) else { return err(StatusCode::UNAUTHORIZED, "Invalid email or password"); };
    if input.password.len() > 128 || input.password.is_empty() { return err(StatusCode::UNAUTHORIZED, "Invalid email or password"); }

    let failures: i64 = match state.db.query_one(
        "SELECT count(*) FROM auth_login_failures WHERE email=$1 AND created_at > now() - INTERVAL '15 minutes'",
        &[&email],
    ).await {
        Ok(row) => row.get(0),
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "Login unavailable"),
    };
    if failures >= 5 { return err(StatusCode::TOO_MANY_REQUESTS, "Too many attempts; retry later"); }

    let lookup = state.db.query_opt("SELECT id, password_hash FROM auth_users WHERE email=$1", &[&email]).await;
    let (user_id, stored_hash): (i64, String) = match lookup {
        Ok(Some(row)) => (row.get(0), row.get(1)),
        Ok(None) => (0, String::new()),
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "Login unavailable"),
    };
    let password = input.password;
    let match_password = if user_id == 0 { false } else {
        let verify = tokio::task::spawn_blocking(move || security::password_matches(&password, &stored_hash)).await;
        matches!(verify, Ok(true))
    };
    if !match_password {
        let _ = state.db.execute("INSERT INTO auth_login_failures(email) VALUES($1)", &[&email]).await;
        return err(StatusCode::UNAUTHORIZED, "Invalid email or password");
    }
    let _ = state.db.execute("DELETE FROM auth_login_failures WHERE email=$1", &[&email]).await;
    issue_session(&state, user_id, email, StatusCode::OK).await
}

pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(hashed) = security::cookie_from_headers(&headers) else { return err(StatusCode::UNAUTHORIZED, "Not authenticated"); };
    match state.db.query_opt(
        "SELECT u.email FROM auth_sessions s JOIN auth_users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.expires_at > now()",
        &[&hashed],
    ).await {
        Ok(Some(row)) => json(StatusCode::OK, UserResponse { email: row.get(0) }),
        Ok(None) => err(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable"),
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !security::valid_origin(&headers) { return err(StatusCode::FORBIDDEN, "Invalid origin"); }
    if let Some(hashed) = security::cookie_from_headers(&headers) {
        if state.db.execute("DELETE FROM auth_sessions WHERE token_hash=$1", &[&hashed]).await.is_err() {
            return err(StatusCode::INTERNAL_SERVER_ERROR, "Logout unavailable");
        }
    }
    cookie_response(StatusCode::OK, OkResponse { ok: true }, security::expire_cookie())
}

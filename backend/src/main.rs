mod auth;
mod companies;
mod security;

use std::sync::Arc;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tokio_postgres::NoTls;

#[derive(Clone)]
pub struct AppState {
    db: Arc<tokio_postgres::Client>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| {
            "host=/var/run/postgresql dbname=flwmenow user=flwme-dev".to_string()
        });

    let (db, connection) = tokio_postgres::connect(&conn, NoTls).await?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            eprintln!("database connection closed: {err}");
        }
    });

    let app = Router::new()
        .route("/api/healthz", get(|| async { "ok" }))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/companies", get(companies::list).post(companies::create))
        .route(
            "/api/companies/{company_id}",
            put(companies::update).delete(companies::remove),
        )
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .with_state(AppState { db: Arc::new(db) });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:18081").await?;

    println!("FLWME_AUTH_LISTEN=127.0.0.1:18081");

    axum::serve(listener, app).await?;

    Ok(())
}

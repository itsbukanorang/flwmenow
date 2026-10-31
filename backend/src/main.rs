mod auth;
mod ceo;
mod companies;
mod delegation;
mod executive;
mod security;

mod tasks;
use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    routing::{get, post, put},
};
use tokio_postgres::NoTls;

#[derive(Clone)]
pub struct AppState {
    db: Arc<tokio_postgres::Client>,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "host=/var/run/postgresql dbname=flwmenow user=flwme-dev".to_string());

    let (db, connection) = tokio_postgres::connect(&conn, NoTls).await?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            eprintln!("database connection closed: {err}");
        }
    });

    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()?;
    let state = AppState {
        db: Arc::new(db),
        http,
    };

    tasks::start_workers(state.clone());

    let app = Router::new()
        .route("/api/healthz", get(|| async { "ok" }))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route(
            "/api/companies",
            get(companies::list).post(companies::create),
        )
        .route(
            "/api/companies/{company_id}",
            put(companies::update).delete(companies::remove),
        )
        .route("/api/ceo/chat", post(ceo::chat))
        .route("/api/ceo/history", get(ceo::history))
        .route("/api/ceo/clear", post(ceo::clear_history))
        .route("/api/executives/cto/run", post(executive::run_cto))
        .route(
            "/api/delegations/active",
            get(delegation::active_delegation),
        )
        .route(
            "/api/delegations/cto/run",
            post(delegation::run_cto_delegation),
        )
        .layer(axum::extract::DefaultBodyLimit::max(65_536))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/{task_id}", get(tasks::detail))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:18081").await?;

    println!("FLWME_RUNTIME_LISTEN=127.0.0.1:18081");

    axum::serve(listener, app).await?;

    Ok(())
}

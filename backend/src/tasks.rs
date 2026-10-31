use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};

use serde_json::Value;
use tokio_postgres::Row;

use crate::{
    AppState,
    delegation::{self, DelegatedTask},
    security,
};

#[derive(Deserialize)]
pub struct TaskListQuery {
    company_id: i64,
}

#[derive(Clone, Debug, Serialize)]
struct TaskRecord {
    task_id: String,
    company_id: i64,
    assigned_to: String,
    title: String,
    objective: String,
    context: Option<String>,
    constraints: Vec<String>,
    status: String,
    attempt_count: i32,
    result: Option<Value>,
    last_error: Option<String>,
    run_id: Option<String>,
    created_at: String,
    created_at_ms: i64,
    started_at: Option<String>,
    completed_at: Option<String>,
    updated_at_ms: i64,
}

#[derive(Serialize)]
struct TaskListOutput {
    tasks: Vec<TaskRecord>,
}

#[derive(Serialize)]
struct ErrorOutput {
    error: String,
}

struct ClaimedTask {
    task_id: String,
    company_id: i64,
    user_id: i64,
    thread_id: i64,
    assigned_to: String,
    objective: String,
    context: Option<String>,
    constraints: Vec<String>,
}

fn json_response<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    response
}

fn error(status: StatusCode, message: &str) -> Response {
    json_response(
        status,
        ErrorOutput {
            error: message.to_string(),
        },
    )
}

async fn current_user_id(state: &AppState, headers: &HeaderMap) -> Result<Option<i64>, ()> {
    let Some(token_hash) = security::cookie_from_headers(headers) else {
        return Ok(None);
    };

    state
        .db
        .query_opt(
            r#"
            SELECT user_id
            FROM auth_sessions
            WHERE token_hash = $1
              AND expires_at > now()
            "#,
            &[&token_hash],
        )
        .await
        .map(|row| row.map(|value| value.get::<_, i64>(0)))
        .map_err(|_| ())
}

async fn company_owned(state: &AppState, company_id: i64, user_id: i64) -> Result<bool, ()> {
    state
        .db
        .query_opt(
            r#"
            SELECT 1
            FROM companies
            WHERE id = $1
              AND owner_user_id = $2
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map(|row| row.is_some())
        .map_err(|_| ())
}

fn parse_constraints(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn parse_result(raw: Option<String>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str::<Value>(&value).ok())
}

fn task_from_row(row: &Row) -> TaskRecord {
    TaskRecord {
        task_id: row.get("task_id"),

        company_id: row.get("company_id"),

        assigned_to: row.get("assigned_to"),

        title: row.get("title"),

        objective: row.get("objective"),

        context: row.get("context"),

        constraints: parse_constraints(row.get("constraints_json")),

        status: row.get("status"),

        attempt_count: row.get("attempt_count"),

        result: parse_result(row.get("result_json")),

        last_error: row.get("last_error"),

        run_id: row.get("run_id"),

        created_at: row.get("created_at"),

        created_at_ms: row.get("created_at_ms"),

        started_at: row.get("started_at"),

        completed_at: row.get("completed_at"),

        updated_at_ms: row.get("updated_at_ms"),
    }
}

const TASK_SELECT: &str = r#"
SELECT
    t.task_id,
    t.company_id,
    t.assigned_to,
    t.title,
    t.objective,
    t.context,
    t.constraints_json,
    t.status,
    t.attempt_count,
    t.result_json,
    t.last_error,

    latest_run.run_id,

    t.created_at::text
        AS created_at,

    (
        EXTRACT(
            EPOCH FROM t.created_at
        ) * 1000
    )::bigint
        AS created_at_ms,

    t.started_at::text
        AS started_at,

    t.completed_at::text
        AS completed_at,

    (
        EXTRACT(
            EPOCH FROM t.updated_at
        ) * 1000
    )::bigint
        AS updated_at_ms

FROM tasks t

LEFT JOIN LATERAL (
    SELECT run_id
    FROM executive_runs er
    WHERE er.task_id = t.task_id
    ORDER BY er.created_at DESC
    LIMIT 1
) latest_run ON true
"#;

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TaskListQuery>,
) -> Response {
    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(value)) => value,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    match company_owned(&state, query.company_id, user_id).await {
        Ok(true) => {}

        Ok(false) => {
            return error(StatusCode::NOT_FOUND, "Company not found");
        }

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Company lookup unavailable",
            );
        }
    }

    let sql = format!(
        "{TASK_SELECT}
         WHERE t.company_id = $1
           AND t.user_id = $2
         ORDER BY t.created_at DESC
         LIMIT 100"
    );

    let rows = match state.db.query(&sql, &[&query.company_id, &user_id]).await {
        Ok(value) => value,

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Tasks unavailable");
        }
    };

    json_response(
        StatusCode::OK,
        TaskListOutput {
            tasks: rows.iter().map(task_from_row).collect(),
        },
    )
}

pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Response {
    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(value)) => value,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    let sql = format!(
        "{TASK_SELECT}
         WHERE t.task_id = $1
           AND t.user_id = $2
         LIMIT 1"
    );

    match state.db.query_opt(&sql, &[&task_id, &user_id]).await {
        Ok(Some(row)) => json_response(StatusCode::OK, task_from_row(&row)),

        Ok(None) => error(StatusCode::NOT_FOUND, "Task not found"),

        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Task unavailable"),
    }
}

async fn claim_task(state: &AppState) -> Result<Option<ClaimedTask>, ()> {
    let row = state
        .db
        .query_opt(
            r#"
            UPDATE tasks
            SET
                status = 'running',
                attempt_count =
                    attempt_count + 1,
                started_at =
                    COALESCE(
                        started_at,
                        now()
                    ),
                updated_at = now()
            WHERE task_id = (
                SELECT task_id
                FROM tasks
                WHERE status = 'queued'
                ORDER BY created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING
                task_id,
                company_id,
                user_id,
                thread_id,
                assigned_to,
                objective,
                context,
                constraints_json
            "#,
            &[],
        )
        .await
        .map_err(|_| ())?;

    let Some(row) = row else {
        return Ok(None);
    };

    let task_id: String = row.get("task_id");

    let _ = state
        .db
        .execute(
            r#"
            INSERT INTO task_events (
                task_id,
                event_type,
                detail
            )
            VALUES (
                $1,
                'running',
                'Execution started'
            )
            "#,
            &[&task_id],
        )
        .await;

    Ok(Some(ClaimedTask {
        task_id,

        company_id: row.get("company_id"),

        user_id: row.get("user_id"),

        thread_id: row.get("thread_id"),

        assigned_to: row.get("assigned_to"),

        objective: row.get("objective"),

        context: row.get("context"),

        constraints: parse_constraints(row.get("constraints_json")),
    }))
}

async fn mark_completed(state: &AppState, task: &ClaimedTask, result: &Value) {
    let result_json = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());

    let _ = state
        .db
        .execute(
            r#"
            UPDATE tasks
            SET
                status = 'completed',
                result_json = $2,
                last_error = NULL,
                completed_at = now(),
                updated_at = now()
            WHERE task_id = $1
            "#,
            &[&task.task_id, &result_json],
        )
        .await;

    let _ = state
        .db
        .execute(
            r#"
            INSERT INTO task_events (
                task_id,
                event_type,
                detail
            )
            VALUES (
                $1,
                'completed',
                'Executive task completed'
            )
            "#,
            &[&task.task_id],
        )
        .await;

    let notification = format!(
        "Boss, the task I delegated to {} has been completed. Open the task to review the result.",
        task.assigned_to,
    );

    let _ = state
        .db
        .execute(
            r#"
            INSERT INTO chat_messages (
                thread_id,
                company_id,
                user_id,
                role,
                content,
                model,
                task_id
            )
            VALUES (
                $1,
                $2,
                $3,
                'assistant',
                $4,
                'flwme-task-runtime',
                $5
            )
            "#,
            &[
                &task.thread_id,
                &task.company_id,
                &task.user_id,
                &notification,
                &task.task_id,
            ],
        )
        .await;
}

async fn mark_failed(state: &AppState, task: &ClaimedTask, message: &str) {
    let clean = message.chars().take(2000).collect::<String>();

    let _ = state
        .db
        .execute(
            r#"
            UPDATE tasks
            SET
                status = 'failed',
                last_error = $2,
                completed_at = now(),
                updated_at = now()
            WHERE task_id = $1
            "#,
            &[&task.task_id, &clean],
        )
        .await;

    let _ = state
        .db
        .execute(
            r#"
            INSERT INTO task_events (
                task_id,
                event_type,
                detail
            )
            VALUES (
                $1,
                'failed',
                $2
            )
            "#,
            &[&task.task_id, &clean],
        )
        .await;

    let notification = format!(
        "Boss, the task delegated to {} could not be completed. Open the task to review its status.",
        task.assigned_to,
    );

    let _ = state
        .db
        .execute(
            r#"
            INSERT INTO chat_messages (
                thread_id,
                company_id,
                user_id,
                role,
                content,
                model,
                task_id
            )
            VALUES (
                $1,
                $2,
                $3,
                'assistant',
                $4,
                'flwme-task-runtime',
                $5
            )
            "#,
            &[
                &task.thread_id,
                &task.company_id,
                &task.user_id,
                &notification,
                &task.task_id,
            ],
        )
        .await;
}

async fn process_task(state: &AppState, task: ClaimedTask) {
    if task.assigned_to != "CTO" {
        mark_failed(state, &task, "Unsupported executive assignment").await;

        return;
    }

    let delegated = DelegatedTask {
        objective: task.objective.clone(),

        context: task.context.clone(),

        constraints: task.constraints.clone(),
    };

    match delegation::execute_cto_task(
        state,
        &task.task_id,
        task.company_id,
        task.user_id,
        delegated,
    )
    .await
    {
        Ok(result) => {
            mark_completed(state, &task, &result).await;
        }

        Err(message) => {
            mark_failed(state, &task, &message).await;
        }
    }
}

async fn worker_loop(state: AppState) {
    loop {
        match claim_task(&state).await {
            Ok(Some(task)) => {
                process_task(&state, task).await;
            }

            Ok(None) => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }

            Err(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

pub fn start_workers(state: AppState) {
    tokio::spawn(async move {
        let _ = state
            .db
            .execute(
                r#"
                UPDATE executive_runs
                SET
                    status = 'failed',
                    retry_state = 'exhausted',
                    last_error =
                        'Runtime restarted before completion',
                    updated_at = now()
                WHERE status = 'executing'
                "#,
                &[],
            )
            .await;

        let _ = state
            .db
            .execute(
                r#"
                UPDATE tasks
                SET
                    status = 'queued',
                    updated_at = now()
                WHERE status = 'running'
                "#,
                &[],
            )
            .await;

        for _ in 0..4 {
            let worker_state = state.clone();

            tokio::spawn(worker_loop(worker_state));
        }

        eprintln!("TASK_WORKER_POOL_STARTED workers=4");
    });
}

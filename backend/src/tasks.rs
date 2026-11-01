use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};

use serde_json::{Value, json};
use tokio_postgres::Row;

use crate::{
    AppState,
    delegation::{self, DelegatedTask},
    executive::ExecutiveRole,
    security,
};

const LANGGRAPH_ORGANIZATION_PLAN_URL: &str = "http://127.0.0.1:18083/v1/orchestrations/plan";

#[derive(Deserialize)]
pub struct TaskListQuery {
    company_id: i64,
}

#[derive(Clone, Debug, Serialize)]
struct TaskRecord {
    task_id: String,
    company_id: i64,
    assigned_to: String,
    executives: Vec<String>,
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

fn parse_executives(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn role_from_code(value: &str) -> Option<ExecutiveRole> {
    match value {
        "CTO" => Some(ExecutiveRole::Cto),
        "COO" => Some(ExecutiveRole::Coo),
        "CFO" => Some(ExecutiveRole::Cfo),
        "CMO" => Some(ExecutiveRole::Cmo),
        _ => None,
    }
}

#[derive(Serialize)]
struct OrganizationPlanRequest {
    task_id: String,
    active_executives: Vec<String>,
    requested_batches: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct OrganizationPlanResponse {
    batches: Vec<Vec<String>>,
}

fn task_from_row(row: &Row) -> TaskRecord {
    TaskRecord {
        task_id: row.get("task_id"),

        company_id: row.get("company_id"),

        assigned_to: row.get("assigned_to"),

        executives: parse_executives(row.get("executives_json")),

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

    CASE
        WHEN orchestration.executives_json IS NOT NULL
        THEN orchestration.executives_json::text
        ELSE jsonb_build_array(
            t.assigned_to
        )::text
    END AS executives_json,

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

LEFT JOIN task_orchestrations orchestration
    ON orchestration.task_id =
        t.task_id

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

    // FLWME_ALK_LEARNING_EVIDENCE_V1
    //
    // A completed executive run produces durable raw learning evidence.
    // This record is intentionally OBSERVED + PENDING only.
    //
    // It is not trusted organizational memory and must not be promoted
    // without later evaluation + verification gates.
    if task.assigned_to != "ORGANIZATION" {
        let learning_write = state
            .db
            .execute(
                r#"
            INSERT INTO learning_evidence (
                schema_version,
                company_id,
                user_id,
                task_id,
                run_id,
                agent,
                objective,
                output_json,
                evidence_json,
                verification_status,
                learning_status,
                source_event
            )
            SELECT
                'flwme.learning.evidence.v1',
                er.company_id,
                er.user_id,
                er.task_id,
                er.run_id,
                er.executive,
                $2,
                $3::text::jsonb,
                '[]'::jsonb,
                'pending',
                'observed',
                'executive_task_completion'
            FROM executive_runs er
            WHERE er.task_id = $1
              AND er.company_id = $4
              AND er.user_id = $5
            ORDER BY er.created_at DESC
            LIMIT 1
            ON CONFLICT (
                run_id,
                source_event
            )
            DO NOTHING
            "#,
                &[
                    &task.task_id,
                    &task.objective,
                    &result_json,
                    &task.company_id,
                    &task.user_id,
                ],
            )
            .await;

        match learning_write {
            Ok(1) => {
                eprintln!("LEARNING_EVIDENCE_OBSERVED task_id={}", task.task_id);
            }

            Ok(0) => {
                eprintln!("LEARNING_EVIDENCE_NOT_WRITTEN task_id={}", task.task_id);
            }

            Ok(_) => {
                eprintln!(
                    "LEARNING_EVIDENCE_UNEXPECTED_WRITE_COUNT task_id={}",
                    task.task_id
                );
            }

            Err(err) => {
                eprintln!(
                    "LEARNING_EVIDENCE_WRITE_FAILED task_id={} error={}",
                    task.task_id, err
                );
            }
        }
    } else {
        eprintln!(
            "MULTI_AGENT_LEARNING_EVIDENCE_DEFERRED task_id={}",
            task.task_id,
        );
    }

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

async fn organization_source_plan(
    state: &AppState,
    task_id: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let row = state
        .db
        .query_opt(
            r#"
            SELECT
                active_roster_json::text,
                requested_batches_json::text
            FROM task_orchestrations
            WHERE task_id = $1
            "#,
            &[&task_id],
        )
        .await
        .map_err(|_| "Organization plan unavailable".to_string())?
        .ok_or_else(|| "Organization plan missing".to_string())?;

    let active_raw: String = row.get(0);

    let batches_raw: String = row.get(1);

    let active_executives = serde_json::from_str::<Vec<String>>(&active_raw)
        .map_err(|_| "Invalid organization roster".to_string())?;

    let requested_batches = serde_json::from_str::<Vec<Vec<String>>>(&batches_raw)
        .map_err(|_| "Invalid organization plan".to_string())?;

    Ok((active_executives, requested_batches))
}

async fn set_organization_state(
    state: &AppState,
    task_id: &str,
    status: &str,
    resolved_batches: Option<&Vec<Vec<String>>>,
) {
    let resolved_json = resolved_batches.and_then(|value| serde_json::to_string(value).ok());

    let _ = state
        .db
        .execute(
            r#"
            UPDATE task_orchestrations
            SET
                status = $2,
                resolved_batches_json =
                    COALESCE(
                        $3::jsonb,
                        resolved_batches_json
                    ),
                updated_at = now()
            WHERE task_id = $1
            "#,
            &[&task_id, &status, &resolved_json],
        )
        .await;
}

async fn execute_organization_task(
    state: &AppState,
    task: &ClaimedTask,
    delegated: &DelegatedTask,
) -> Result<Value, String> {
    let (active_executives, requested_batches) =
        organization_source_plan(state, &task.task_id).await?;

    let response = state
        .http
        .post(LANGGRAPH_ORGANIZATION_PLAN_URL)
        .json(&OrganizationPlanRequest {
            task_id: task.task_id.clone(),
            active_executives,
            requested_batches,
        })
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "LangGraph organization planner unavailable".to_string())?;

    if !response.status().is_success() {
        set_organization_state(state, &task.task_id, "failed", None).await;

        return Err("LangGraph rejected organization plan".to_string());
    }

    let plan = response
        .json::<OrganizationPlanResponse>()
        .await
        .map_err(|_| "Invalid LangGraph organization plan".to_string())?;

    if plan.batches.is_empty() {
        return Err("LangGraph returned empty organization plan".to_string());
    }

    set_organization_state(state, &task.task_id, "running", Some(&plan.batches)).await;

    let mut aggregated: Vec<Value> = Vec::new();

    // Batches execute sequentially.
    // Members inside one batch execute concurrently.
    for batch in &plan.batches {
        if batch.is_empty() {
            return Err("LangGraph returned empty execution batch".to_string());
        }

        let mut jobs = tokio::task::JoinSet::new();

        for executive_code in batch {
            let role = role_from_code(executive_code)
                .ok_or_else(|| "LangGraph returned unsupported executive".to_string())?;

            let child_state = state.clone();

            let child_task_id = task.task_id.clone();

            let child_task = delegated.clone();

            let company_id = task.company_id;

            let user_id = task.user_id;

            let executive = executive_code.clone();

            jobs.spawn(async move {
                let result = delegation::execute_executive_task(
                    &child_state,
                    role,
                    &child_task_id,
                    company_id,
                    user_id,
                    child_task,
                )
                .await;

                (executive, result)
            });
        }

        while let Some(joined) = jobs.join_next().await {
            let (executive, result) =
                joined.map_err(|_| "Executive execution task failed".to_string())?;

            match result {
                Ok(output) => {
                    aggregated.push(json!({
                        "executive":
                            executive,
                        "result":
                            output,
                    }));
                }

                Err(message) => {
                    jobs.abort_all();

                    set_organization_state(state, &task.task_id, "failed", Some(&plan.batches))
                        .await;

                    return Err(format!("{} execution failed: {}", executive, message,));
                }
            }
        }
    }

    set_organization_state(state, &task.task_id, "completed", Some(&plan.batches)).await;

    Ok(json!({
        "orchestration":
            "langgraph",
        "execution":
            "multi-agent",
        "batches":
            plan.batches,
        "results":
            aggregated,
    }))
}

async fn process_task(state: &AppState, task: ClaimedTask) {
    let delegated = DelegatedTask {
        objective: task.objective.clone(),
        context: task.context.clone(),
        constraints: task.constraints.clone(),
    };

    let execution = if task.assigned_to == "ORGANIZATION" {
        execute_organization_task(state, &task, &delegated).await
    } else {
        let Some(role) = role_from_code(&task.assigned_to) else {
            mark_failed(state, &task, "Unsupported executive assignment").await;

            return;
        };

        delegation::execute_executive_task(
            state,
            role,
            &task.task_id,
            task.company_id,
            task.user_id,
            delegated,
        )
        .await
    };

    match execution {
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

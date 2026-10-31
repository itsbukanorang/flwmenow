use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use rand::{RngCore, rngs::OsRng};

use serde::{Deserialize, Serialize};

use serde_json::{Value, json};

use crate::{AppState, executive, security};

const LANGGRAPH_START_URL: &str = "http://127.0.0.1:18083/v1/delegations/cto/start";

const LANGGRAPH_RESUME_URL: &str = "http://127.0.0.1:18083/v1/delegations/cto/resume";

const CTO_URL: &str = "http://127.0.0.1:18081/api/executives/cto/run";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct DelegatedTask {
    pub(crate) objective: String,

    #[serde(default)]
    pub(crate) context: Option<String>,

    #[serde(default)]
    pub(crate) constraints: Vec<String>,
}

#[derive(Deserialize)]
pub struct DelegationInput {
    company_id: i64,
    task: DelegatedTask,
}

#[derive(Serialize)]
struct StartRequest {
    run_id: String,
    company_id: i64,
    user_id: i64,
    executive: &'static str,
    task: DelegatedTask,
}

#[derive(Deserialize)]
struct StartResponse {
    action: String,
    executive: String,
    run_id: String,
    attempt: i32,
    status: String,
    retry_state: String,
    verifier_state: String,
}

#[derive(Serialize)]
struct ResumeRequest {
    run_id: String,
    ok: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ResumeResponse {
    run_id: String,
    executive: String,
    status: String,
    attempt: i32,
    plan: Vec<String>,
    retry_state: String,
    verifier_state: String,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Serialize)]
struct DelegationOutput {
    run_id: String,
    orchestration: &'static str,
    executive: String,
    status: String,
    attempt: i32,
    plan: Vec<String>,
    retry_state: String,
    verifier_state: String,
    result: Value,
}

#[derive(Deserialize)]
pub struct ActiveDelegationQuery {
    company_id: i64,
}

#[derive(Serialize)]
struct ActiveDelegationOutput {
    active: bool,
    run_id: Option<String>,
    executive: Option<String>,
    status: Option<String>,
}

#[derive(Serialize)]
struct ErrorOutput {
    error: String,
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

fn valid_text(value: &str, max: usize) -> bool {
    let length = value.trim().chars().count();

    length > 0
        && length <= max
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

fn validate_task(task: &DelegatedTask) -> Result<(), &'static str> {
    if !valid_text(&task.objective, 8_000) {
        return Err("Task objective must be 1–8000 characters");
    }

    if let Some(context) = task.context.as_deref() {
        if !valid_text(context, 8_000) {
            return Err("Task context must be 1–8000 characters");
        }
    }

    if task.constraints.len() > 16 {
        return Err("Task supports at most 16 constraints");
    }

    if task
        .constraints
        .iter()
        .any(|value| !valid_text(value, 1_000))
    {
        return Err("Each constraint must be 1–1000 characters");
    }

    Ok(())
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

fn make_run_id() -> String {
    let mut bytes = [0u8; 16];

    OsRng.fill_bytes(&mut bytes);

    format!("run_{}", hex::encode(bytes),)
}

async fn set_run_state(
    state: &AppState,
    run_id: &str,
    status: &str,
    attempt: i32,
    retry_state: &str,
    verifier_state: &str,
    last_error: Option<&str>,
) {
    let _ = state
        .db
        .execute(
            r#"
            UPDATE executive_runs
            SET
                status = $2,
                attempt = $3,
                retry_state = $4,
                verifier_state = $5,
                last_error = $6,
                updated_at = now()
            WHERE run_id = $1
            "#,
            &[
                &run_id,
                &status,
                &attempt,
                &retry_state,
                &verifier_state,
                &last_error,
            ],
        )
        .await;
}

pub async fn active_delegation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ActiveDelegationQuery>,
) -> Response {
    if query.company_id <= 0 {
        return error(StatusCode::BAD_REQUEST, "Invalid company");
    }

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

    let row = match state
        .db
        .query_opt(
            r#"
            SELECT
                run_id,
                executive,
                status
            FROM executive_runs
            WHERE company_id = $1
              AND user_id = $2
              AND status IN (
                  'created',
                  'routed',
                  'executing'
              )
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            &[&query.company_id, &user_id],
        )
        .await
    {
        Ok(value) => value,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Delegation state unavailable",
            );
        }
    };

    match row {
        Some(value) => json_response(
            StatusCode::OK,
            ActiveDelegationOutput {
                active: true,
                run_id: Some(value.get::<_, String>("run_id")),
                executive: Some(value.get::<_, String>("executive")),
                status: Some(value.get::<_, String>("status")),
            },
        ),

        None => json_response(
            StatusCode::OK,
            ActiveDelegationOutput {
                active: false,
                run_id: None,
                executive: None,
                status: None,
            },
        ),
    }
}

pub async fn run_cto_delegation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DelegationInput>,
) -> Response {
    if !security::valid_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "Invalid origin");
    }

    if input.company_id <= 0 {
        return error(StatusCode::BAD_REQUEST, "Invalid company");
    }

    if let Err(message) = validate_task(&input.task) {
        return error(StatusCode::BAD_REQUEST, message);
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(value)) => value,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    match company_owned(&state, input.company_id, user_id).await {
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

    let run_id = make_run_id();

    if state
        .db
        .execute(
            r#"
            INSERT INTO executive_runs (
                run_id,
                company_id,
                user_id,
                executive,
                orchestration,
                status
            )
            VALUES (
                $1,
                $2,
                $3,
                'CTO',
                'langgraph',
                'created'
            )
            "#,
            &[&run_id, &input.company_id, &user_id],
        )
        .await
        .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unable to create executive run",
        );
    }

    let start_response = match state
        .http
        .post(LANGGRAPH_START_URL)
        .json(&StartRequest {
            run_id: run_id.clone(),
            company_id: input.company_id,
            user_id,
            executive: "CTO",
            task: input.task.clone(),
        })
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(value) => value,

        Err(_) => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                0,
                "exhausted",
                "not_started",
                Some("LangGraph start unavailable"),
            )
            .await;

            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "LangGraph runtime unavailable",
            );
        }
    };

    if !start_response.status().is_success() {
        set_run_state(
            &state,
            &run_id,
            "failed",
            0,
            "exhausted",
            "not_started",
            Some("LangGraph rejected delegation"),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "LangGraph rejected delegation");
    }

    let start = match start_response.json::<StartResponse>().await {
        Ok(value) => value,

        Err(_) => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                0,
                "exhausted",
                "not_started",
                Some("Invalid LangGraph start response"),
            )
            .await;

            return error(StatusCode::BAD_GATEWAY, "Invalid LangGraph response");
        }
    };

    if start.run_id != run_id
        || start.executive != "CTO"
        || start.action != "execute_cto"
        || start.status != "awaiting_executive"
    {
        set_run_state(
            &state,
            &run_id,
            "failed",
            start.attempt,
            "exhausted",
            &start.verifier_state,
            Some("Unexpected LangGraph route"),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "Unexpected LangGraph route");
    }

    set_run_state(
        &state,
        &run_id,
        "executing",
        start.attempt,
        &start.retry_state,
        &start.verifier_state,
        None,
    )
    .await;

    let cookie = match headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value.to_string(),

        None => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                start.attempt,
                "exhausted",
                &start.verifier_state,
                Some("Session cookie unavailable"),
            )
            .await;

            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }
    };

    // LangGraph made the delegation decision.
    //
    // Rust is only the control-plane execution adapter.
    // Reuse the existing API-03 CTO endpoint so there is
    // exactly one CTO model runtime.
    let cto_response = match state
        .http
        .post(CTO_URL)
        .header(reqwest::header::ORIGIN, "https://flwme.kiyt.io")
        .header(reqwest::header::COOKIE, cookie)
        .json(&json!({
            "company_id":
                input.company_id,
            "task":
                input.task,
        }))
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
    {
        Ok(value) => value,

        Err(_) => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                start.attempt,
                "exhausted",
                &start.verifier_state,
                Some("CTO execution unavailable"),
            )
            .await;

            return error(StatusCode::BAD_GATEWAY, "CTO execution unavailable");
        }
    };

    let cto_status = cto_response.status();

    let cto_body = match cto_response.json::<Value>().await {
        Ok(value) => value,

        Err(_) => {
            json!({
                "error":
                    "Invalid CTO response"
            })
        }
    };

    let (resume_ok, resume_output, resume_error) = if cto_status.is_success() {
        (true, Some(cto_body), None)
    } else {
        (
            false,
            None,
            Some(format!("CTO execution HTTP {}", cto_status.as_u16(),)),
        )
    };

    let resume_response = match state
        .http
        .post(LANGGRAPH_RESUME_URL)
        .json(&ResumeRequest {
            run_id: run_id.clone(),
            ok: resume_ok,
            output: resume_output,
            error: resume_error,
        })
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(value) => value,

        Err(_) => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                start.attempt,
                "exhausted",
                &start.verifier_state,
                Some("LangGraph resume unavailable"),
            )
            .await;

            return error(StatusCode::BAD_GATEWAY, "LangGraph resume unavailable");
        }
    };

    if !resume_response.status().is_success() {
        set_run_state(
            &state,
            &run_id,
            "failed",
            start.attempt,
            "exhausted",
            &start.verifier_state,
            Some("LangGraph resume rejected"),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "LangGraph resume rejected");
    }

    let finished = match resume_response.json::<ResumeResponse>().await {
        Ok(value) => value,

        Err(_) => {
            set_run_state(
                &state,
                &run_id,
                "failed",
                start.attempt,
                "exhausted",
                &start.verifier_state,
                Some("Invalid LangGraph final response"),
            )
            .await;

            return error(StatusCode::BAD_GATEWAY, "Invalid LangGraph final response");
        }
    };

    if finished.run_id != run_id || finished.executive != "CTO" {
        set_run_state(
            &state,
            &run_id,
            "failed",
            finished.attempt,
            "exhausted",
            &finished.verifier_state,
            Some("LangGraph run identity mismatch"),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "LangGraph run identity mismatch");
    }

    if finished.status != "completed" {
        let message = finished.error.as_deref().unwrap_or("Executive run failed");

        set_run_state(
            &state,
            &run_id,
            "failed",
            finished.attempt,
            &finished.retry_state,
            &finished.verifier_state,
            Some(message),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "Executive run failed");
    }

    let Some(result) = finished.result else {
        set_run_state(
            &state,
            &run_id,
            "failed",
            finished.attempt,
            "exhausted",
            &finished.verifier_state,
            Some("Executive result missing"),
        )
        .await;

        return error(StatusCode::BAD_GATEWAY, "Executive result missing");
    };

    set_run_state(
        &state,
        &run_id,
        "completed",
        finished.attempt,
        &finished.retry_state,
        &finished.verifier_state,
        None,
    )
    .await;

    json_response(
        StatusCode::OK,
        DelegationOutput {
            run_id,
            orchestration: "langgraph",
            executive: "CTO".to_string(),
            status: "completed".to_string(),
            attempt: finished.attempt,
            plan: finished.plan,
            retry_state: finished.retry_state,
            verifier_state: finished.verifier_state,
            result,
        },
    )
}

pub(crate) async fn execute_cto_task(
    state: &AppState,
    task_id: &str,
    company_id: i64,
    user_id: i64,
    task: DelegatedTask,
) -> Result<Value, String> {
    let run_id = make_run_id();

    state
        .db
        .execute(
            r#"
            INSERT INTO executive_runs (
                run_id,
                task_id,
                company_id,
                user_id,
                executive,
                orchestration,
                status
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                'CTO',
                'langgraph',
                'created'
            )
            "#,
            &[&run_id, &task_id, &company_id, &user_id],
        )
        .await
        .map_err(|_| "Unable to create executive run".to_string())?;

    let start_response = state
        .http
        .post(LANGGRAPH_START_URL)
        .json(&StartRequest {
            run_id: run_id.clone(),

            company_id,

            user_id,

            executive: "CTO",

            task: task.clone(),
        })
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "LangGraph runtime unavailable".to_string())?;

    if !start_response.status().is_success() {
        set_run_state(
            state,
            &run_id,
            "failed",
            0,
            "exhausted",
            "not_started",
            Some("LangGraph rejected delegation"),
        )
        .await;

        return Err("LangGraph rejected delegation".to_string());
    }

    let start = start_response
        .json::<StartResponse>()
        .await
        .map_err(|_| "Invalid LangGraph response".to_string())?;

    if start.run_id != run_id
        || start.executive != "CTO"
        || start.action != "execute_cto"
        || start.status != "awaiting_executive"
    {
        set_run_state(
            state,
            &run_id,
            "failed",
            start.attempt,
            "exhausted",
            &start.verifier_state,
            Some("Unexpected LangGraph route"),
        )
        .await;

        return Err("Unexpected LangGraph route".to_string());
    }

    set_run_state(
        state,
        &run_id,
        "executing",
        start.attempt,
        &start.retry_state,
        &start.verifier_state,
        None,
    )
    .await;

    let exec_task = executive::ExecutiveTask {
        objective: task.objective.clone(),

        context: task.context.clone(),

        constraints: task.constraints.clone(),
    };

    let execution = executive::execute_cto_for_user(state, company_id, user_id, &exec_task).await;

    let resume_request = match execution {
        Ok(output) => ResumeRequest {
            run_id: run_id.clone(),

            ok: true,

            output: Some(output),

            error: None,
        },

        Err(message) => ResumeRequest {
            run_id: run_id.clone(),

            ok: false,

            output: None,

            error: Some(message),
        },
    };

    let resume_response = state
        .http
        .post(LANGGRAPH_RESUME_URL)
        .json(&resume_request)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "LangGraph resume unavailable".to_string())?;

    if !resume_response.status().is_success() {
        set_run_state(
            state,
            &run_id,
            "failed",
            start.attempt,
            "exhausted",
            &start.verifier_state,
            Some("LangGraph resume rejected"),
        )
        .await;

        return Err("LangGraph resume rejected".to_string());
    }

    let finished = resume_response
        .json::<ResumeResponse>()
        .await
        .map_err(|_| "Invalid LangGraph final response".to_string())?;

    if finished.run_id != run_id || finished.executive != "CTO" {
        return Err("LangGraph run identity mismatch".to_string());
    }

    if finished.status != "completed" {
        let message = finished
            .error
            .unwrap_or_else(|| "Executive run failed".to_string());

        set_run_state(
            state,
            &run_id,
            "failed",
            finished.attempt,
            &finished.retry_state,
            &finished.verifier_state,
            Some(&message),
        )
        .await;

        return Err(message);
    }

    let result = finished
        .result
        .ok_or_else(|| "Executive result missing".to_string())?;

    set_run_state(
        state,
        &run_id,
        "completed",
        finished.attempt,
        &finished.retry_state,
        &finished.verifier_state,
        None,
    )
    .await;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_task_validation() {
        let task = DelegatedTask {
            objective: "Review architecture".to_string(),
            context: None,
            constraints: vec!["Do not execute".to_string()],
        };

        assert!(validate_task(&task).is_ok());
    }

    #[test]
    fn empty_task_rejected() {
        let task = DelegatedTask {
            objective: " ".to_string(),
            context: None,
            constraints: vec![],
        };

        assert!(validate_task(&task).is_err());
    }

    #[test]
    fn run_ids_are_unique() {
        let first = make_run_id();

        let second = make_run_id();

        assert_ne!(first, second);

        assert!(first.starts_with("run_"));
    }
}

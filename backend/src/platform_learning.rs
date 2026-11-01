use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{AppState, security};

#[derive(Serialize)]
struct ErrorOutput {
    error: &'static str,
}

#[derive(Serialize)]
struct AccessOutput {
    allowed: bool,
}

#[derive(Serialize)]
struct LearningSummary {
    observed: i64,
    evaluated: i64,
    verified: i64,
    candidates: i64,
    promoted: i64,
    rejected: i64,
}

#[derive(Serialize)]
struct LearningEvidence {
    id: i64,
    company_id: i64,
    user_id: i64,
    task_id: String,
    run_id: String,
    agent: String,
    verification_status: String,
    learning_status: String,
    source_event: String,
    created_at: String,
}

#[derive(Serialize)]
struct LearningOverview {
    summary: LearningSummary,
    evidence: Vec<LearningEvidence>,
}

fn json<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    response
}

fn error(status: StatusCode, message: &'static str) -> Response {
    json(status, ErrorOutput { error: message })
}

fn parse_platform_owner_user_id(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok().filter(|value| *value > 0)
}

fn configured_platform_owner_user_id() -> Option<i64> {
    std::env::var("FLWME_PLATFORM_OWNER_USER_ID")
        .ok()
        .as_deref()
        .and_then(parse_platform_owner_user_id)
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

async fn require_platform_owner(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(owner_user_id) = configured_platform_owner_user_id() else {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Platform owner is not configured",
        ));
    };

    let user_id = match current_user_id(state, headers).await {
        Ok(Some(value)) => value,

        Ok(None) => {
            return Err(error(StatusCode::UNAUTHORIZED, "Not authenticated"));
        }

        Err(_) => {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to verify platform owner",
            ));
        }
    };

    if user_id != owner_user_id {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Platform owner access required",
        ));
    }

    Ok(())
}

pub async fn access(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_platform_owner(&state, &headers).await {
        return response;
    }

    json(StatusCode::OK, AccessOutput { allowed: true })
}

pub async fn overview(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_platform_owner(&state, &headers).await {
        return response;
    }

    let summary_row = match state
        .db
        .query_one(
            r#"
                SELECT
                    count(*) FILTER (
                        WHERE learning_status =
                            'observed'
                    )::bigint,

                    count(*) FILTER (
                        WHERE learning_status =
                            'evaluated'
                    )::bigint,

                    count(*) FILTER (
                        WHERE verification_status =
                            'verified'
                    )::bigint,

                    count(*) FILTER (
                        WHERE learning_status =
                            'candidate'
                    )::bigint,

                    count(*) FILTER (
                        WHERE learning_status =
                            'promoted'
                    )::bigint,

                    count(*) FILTER (
                        WHERE learning_status =
                            'rejected'
                           OR verification_status =
                            'rejected'
                    )::bigint

                FROM learning_evidence
                "#,
            &[],
        )
        .await
    {
        Ok(row) => row,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to load learning summary",
            );
        }
    };

    let rows = match state
        .db
        .query(
            r#"
                SELECT
                    id,
                    company_id,
                    user_id,
                    task_id,
                    run_id,
                    agent,
                    verification_status,
                    learning_status,
                    source_event,
                    to_char(
                        created_at
                            AT TIME ZONE 'UTC',
                        'YYYY-MM-DD"T"HH24:MI:SS"Z"'
                    )

                FROM learning_evidence

                ORDER BY
                    created_at DESC,
                    id DESC

                LIMIT 100
                "#,
            &[],
        )
        .await
    {
        Ok(rows) => rows,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to load learning evidence",
            );
        }
    };

    let evidence = rows
        .into_iter()
        .map(|row| LearningEvidence {
            id: row.get(0),
            company_id: row.get(1),
            user_id: row.get(2),
            task_id: row.get(3),
            run_id: row.get(4),
            agent: row.get(5),
            verification_status: row.get(6),
            learning_status: row.get(7),
            source_event: row.get(8),
            created_at: row.get(9),
        })
        .collect();

    json(
        StatusCode::OK,
        LearningOverview {
            summary: LearningSummary {
                observed: summary_row.get(0),
                evaluated: summary_row.get(1),
                verified: summary_row.get(2),
                candidates: summary_row.get(3),
                promoted: summary_row.get(4),
                rejected: summary_row.get(5),
            },
            evidence,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::parse_platform_owner_user_id;

    #[test]
    fn platform_owner_id_validation() {
        assert_eq!(parse_platform_owner_user_id("7"), Some(7),);

        assert_eq!(parse_platform_owner_user_id(" 7 "), Some(7),);

        assert_eq!(parse_platform_owner_user_id("0"), None,);

        assert_eq!(parse_platform_owner_user_id("-1"), None,);

        assert_eq!(parse_platform_owner_user_id("abc"), None,);
    }
}

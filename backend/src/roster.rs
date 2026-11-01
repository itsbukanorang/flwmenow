use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};

use crate::{AppState, security};

const EXECUTIVES: [&str; 4] = ["CTO", "COO", "CFO", "CMO"];

#[derive(Deserialize)]
pub struct RosterQuery {
    company_id: i64,
}

#[derive(Deserialize)]
pub struct RosterInput {
    company_id: i64,

    active_executives: Vec<String>,
}

#[derive(Serialize)]
struct RosterOutput {
    company_id: i64,

    active_executives: Vec<String>,
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

fn valid_roles(roles: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();

    roles
        .iter()
        .all(|role| EXECUTIVES.contains(&role.as_str()) && seen.insert(role.as_str()))
}

async fn read_roster(state: &AppState, company_id: i64) -> Result<Vec<String>, ()> {
    let rows = state
        .db
        .query(
            r#"
            SELECT executive
            FROM company_executive_roster
            WHERE company_id = $1
              AND active = TRUE
            ORDER BY
                CASE executive
                    WHEN 'CTO' THEN 1
                    WHEN 'CFO' THEN 2
                    WHEN 'CMO' THEN 3
                    WHEN 'COO' THEN 4
                    ELSE 99
                END
            "#,
            &[&company_id],
        )
        .await
        .map_err(|_| ())?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<_, String>("executive"))
        .collect())
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RosterQuery>,
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

    match read_roster(&state, query.company_id).await {
        Ok(active_executives) => json_response(
            StatusCode::OK,
            RosterOutput {
                company_id: query.company_id,
                active_executives,
            },
        ),

        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Executive roster unavailable",
        ),
    }
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RosterInput>,
) -> Response {
    if !security::valid_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "Invalid origin");
    }

    if input.company_id <= 0 {
        return error(StatusCode::BAD_REQUEST, "Invalid company");
    }

    if !valid_roles(&input.active_executives) {
        return error(StatusCode::BAD_REQUEST, "Invalid executive roster");
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

    let active = |role: &str| input.active_executives.iter().any(|value| value == role);

    let result = state
        .db
        .execute(
            r#"
            INSERT INTO company_executive_roster (
                company_id,
                executive,
                active,
                updated_at
            )
            VALUES
                ($1, 'CTO', $2, now()),
                ($1, 'COO', $3, now()),
                ($1, 'CFO', $4, now()),
                ($1, 'CMO', $5, now())
            ON CONFLICT (
                company_id,
                executive
            )
            DO UPDATE SET
                active =
                    EXCLUDED.active,
                updated_at =
                    now()
            "#,
            &[
                &input.company_id,
                &active("CTO"),
                &active("COO"),
                &active("CFO"),
                &active("CMO"),
            ],
        )
        .await;

    if result.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unable to update executive roster",
        );
    }

    json_response(
        StatusCode::OK,
        RosterOutput {
            company_id: input.company_id,

            active_executives: EXECUTIVES
                .iter()
                .filter(|role| active(role))
                .map(|role| (*role).to_string())
                .collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_validation() {
        assert!(valid_roles(&["CTO".to_string(), "CFO".to_string(),],));

        assert!(valid_roles(&[]));

        assert!(!valid_roles(&["CEO".to_string(),],));

        assert!(!valid_roles(&["CTO".to_string(), "CTO".to_string(),],));
    }
}

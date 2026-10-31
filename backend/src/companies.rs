use axum::{
    extract::{Json, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{security, AppState};

#[derive(Deserialize)]
pub struct CreateCompany {
    name: String,
}

#[derive(Serialize, Clone)]
pub struct CompanyResponse {
    id: i64,
    name: String,
    status: String,
    created_at: String,
}

#[derive(Serialize)]
struct CompanyList {
    companies: Vec<CompanyResponse>,
}

#[derive(Serialize)]
struct ErrorMessage {
    error: &'static str,
}

#[derive(Serialize)]
struct OkMessage {
    ok: bool,
}

fn json<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    response
}

fn err(status: StatusCode, message: &'static str) -> Response {
    json(status, ErrorMessage { error: message })
}

fn normalize_company_name(raw: &str) -> Option<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");

    if !(2..=120).contains(&name.chars().count()) {
        return None;
    }

    if name.chars().any(|c| c.is_control()) {
        return None;
    }

    Some(name)
}

async fn current_user_id(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<i64>, ()> {
    let Some(token_hash) = security::cookie_from_headers(headers) else {
        return Ok(None);
    };

    state
        .db
        .query_opt(
            "
            SELECT user_id
            FROM auth_sessions
            WHERE token_hash = $1
              AND expires_at > now()
            ",
            &[&token_hash],
        )
        .await
        .map(|row| row.map(|r| r.get::<_, i64>(0)))
        .map_err(|_| ())
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,
        Ok(None) => return err(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to load companies",
            )
        }
    };

    let rows = match state
        .db
        .query(
            r#"
            SELECT
                id,
                name,
                status,
                to_char(
                    created_at AT TIME ZONE 'UTC',
                    'YYYY-MM-DD"T"HH24:MI:SS"Z"'
                )
            FROM companies
            WHERE owner_user_id = $1
            ORDER BY created_at DESC, id DESC
            "#,
            &[&user_id],
        )
        .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to load companies",
            )
        }
    };

    let companies = rows
        .into_iter()
        .map(|row| CompanyResponse {
            id: row.get(0),
            name: row.get(1),
            status: row.get(2),
            created_at: row.get(3),
        })
        .collect();

    json(StatusCode::OK, CompanyList { companies })
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateCompany>,
) -> Response {
    if !security::valid_origin(&headers) {
        return err(StatusCode::FORBIDDEN, "Invalid origin");
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,
        Ok(None) => return err(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to create company",
            )
        }
    };

    let Some(name) = normalize_company_name(&input.name) else {
        return err(
            StatusCode::BAD_REQUEST,
            "Company name must be 2–120 characters",
        );
    };

    let name_key = name.to_lowercase();

    let row = match state
        .db
        .query_opt(
            r#"
            INSERT INTO companies (
                owner_user_id,
                name,
                name_key
            )
            VALUES ($1, $2, $3)
            ON CONFLICT (owner_user_id, name_key)
            DO NOTHING
            RETURNING
                id,
                name,
                status,
                to_char(
                    created_at AT TIME ZONE 'UTC',
                    'YYYY-MM-DD"T"HH24:MI:SS"Z"'
                )
            "#,
            &[&user_id, &name, &name_key],
        )
        .await
    {
        Ok(row) => row,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to create company",
            )
        }
    };

    match row {
        Some(row) => json(
            StatusCode::CREATED,
            CompanyResponse {
                id: row.get(0),
                name: row.get(1),
                status: row.get(2),
                created_at: row.get(3),
            },
        ),
        None => err(StatusCode::CONFLICT, "Company already exists"),
    }
}

pub async fn update(
    Path(company_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateCompany>,
) -> Response {
    if !security::valid_origin(&headers) {
        return err(StatusCode::FORBIDDEN, "Invalid origin");
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,
        Ok(None) => return err(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to update company",
            )
        }
    };

    let Some(name) = normalize_company_name(&input.name) else {
        return err(
            StatusCode::BAD_REQUEST,
            "Company name must be 2–120 characters",
        );
    };

    let name_key = name.to_lowercase();

    match state
        .db
        .query_opt(
            "
            SELECT id
            FROM companies
            WHERE owner_user_id = $1
              AND name_key = $2
              AND id <> $3
            ",
            &[&user_id, &name_key, &company_id],
        )
        .await
    {
        Ok(Some(_)) => {
            return err(StatusCode::CONFLICT, "Company already exists");
        }
        Ok(None) => {}
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to update company",
            )
        }
    }

    let row = match state
        .db
        .query_opt(
            r#"
            UPDATE companies
            SET
                name = $3,
                name_key = $4
            WHERE id = $1
              AND owner_user_id = $2
            RETURNING
                id,
                name,
                status,
                to_char(
                    created_at AT TIME ZONE 'UTC',
                    'YYYY-MM-DD"T"HH24:MI:SS"Z"'
                )
            "#,
            &[&company_id, &user_id, &name, &name_key],
        )
        .await
    {
        Ok(row) => row,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to update company",
            )
        }
    };

    match row {
        Some(row) => json(
            StatusCode::OK,
            CompanyResponse {
                id: row.get(0),
                name: row.get(1),
                status: row.get(2),
                created_at: row.get(3),
            },
        ),
        None => err(StatusCode::NOT_FOUND, "Company not found"),
    }
}

pub async fn remove(
    Path(company_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !security::valid_origin(&headers) {
        return err(StatusCode::FORBIDDEN, "Invalid origin");
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,
        Ok(None) => return err(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to delete company",
            )
        }
    };

    let row = match state
        .db
        .query_opt(
            "
            DELETE FROM companies
            WHERE id = $1
              AND owner_user_id = $2
            RETURNING id
            ",
            &[&company_id, &user_id],
        )
        .await
    {
        Ok(row) => row,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to delete company",
            )
        }
    };

    match row {
        Some(_) => json(StatusCode::OK, OkMessage { ok: true }),
        None => err(StatusCode::NOT_FOUND, "Company not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn company_name_validation() {
        assert_eq!(
            normalize_company_name("  Acme   Corporation  ").as_deref(),
            Some("Acme Corporation")
        );

        assert!(normalize_company_name("").is_none());
        assert!(normalize_company_name("A").is_none());
        assert!(normalize_company_name(&"x".repeat(121)).is_none());
    }
}

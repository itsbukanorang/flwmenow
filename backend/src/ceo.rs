use std::time::Duration;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{AppState, security};

const META_URL: &str = "https://api.meta.ai/v1/chat/completions";
const META_MODEL: &str = "muse-spark-1.3";

const CEO_SYSTEM_PROMPT: &str = r#"
You are the CEO of FLWME, an agentic AI business operating system.

Your job is to communicate naturally with the user, understand their goal,
use provided company context correctly, and decide what the organization
should do next.

Rules:
- Never fabricate completed work, tool execution, memory, evidence, or status.
- Never claim a task was executed unless FLWME runtime provides verified evidence.
- Never invent company memory that was not supplied in context.
- Keep tenant/company information isolated.
- For normal conversation, answer directly and clearly.
- Delegation and tool execution are handled by FLWME runtime and will be
  provided to you through explicit runtime tools in later execution stages.
- If information is missing, say what is missing instead of pretending to know.
"#;

#[derive(Deserialize)]
pub struct ChatInput {
    company_id: i64,
    message: String,
}

#[derive(Serialize)]
struct ChatOutput {
    reply: String,
    model: String,
    cached_tokens: u64,
}

#[derive(Serialize)]
struct ErrorOutput {
    error: String,
}

#[derive(Deserialize)]
struct MetaResponse {
    model: Option<String>,
    choices: Vec<MetaChoice>,
    usage: Option<MetaUsage>,
}

#[derive(Deserialize)]
struct MetaChoice {
    message: MetaMessage,
}

#[derive(Deserialize)]
struct MetaMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct MetaUsage {
    prompt_tokens_details: Option<PromptTokenDetails>,
}

#[derive(Deserialize)]
struct PromptTokenDetails {
    cached_tokens: Option<u64>,
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
        .map(|row| row.map(|r| r.get::<_, i64>(0)))
        .map_err(|_| ())
}

pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ChatInput>,
) -> Response {
    if !security::valid_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "Invalid origin");
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,
        Ok(None) => return error(StatusCode::UNAUTHORIZED, "Not authenticated"),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable"),
    };

    let message = input.message.trim();

    if message.is_empty() || message.chars().count() > 16_000 {
        return error(
            StatusCode::BAD_REQUEST,
            "Message must be 1–16000 characters",
        );
    }

    let company = match state
        .db
        .query_opt(
            r#"
            SELECT name
            FROM companies
            WHERE id = $1
              AND owner_user_id = $2
            "#,
            &[&input.company_id, &user_id],
        )
        .await
    {
        Ok(Some(row)) => row.get::<_, String>(0),
        Ok(None) => return error(StatusCode::NOT_FOUND, "Company not found"),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Company lookup unavailable",
            );
        }
    };

    let api_key = match std::env::var("MODEL_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return error(StatusCode::SERVICE_UNAVAILABLE, "CEO provider unavailable"),
    };

    let company_context = format!(
        "Current company: {company}. This context belongs only to this company and must not be mixed with any other tenant."
    );

    let payload = json!({
        "model": META_MODEL,
        "prompt_cache_retention": "in_memory",
        "messages": [
            {
                "role": "system",
                "content": CEO_SYSTEM_PROMPT
            },
            {
                "role": "developer",
                "content": company_context
            },
            {
                "role": "user",
                "content": message
            }
        ]
    });

    let provider_response = match state
        .http
        .post(META_URL)
        .bearer_auth(api_key)
        .json(&payload)
        .timeout(Duration::from_secs(90))
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            eprintln!(
                "CEO_PROVIDER_REQUEST_FAILED kind={}",
                if err.is_timeout() {
                    "timeout"
                } else {
                    "network"
                }
            );

            return error(StatusCode::BAD_GATEWAY, "CEO provider request failed");
        }
    };

    let provider_status = provider_response.status();

    let body = match provider_response.bytes().await {
        Ok(body) => body,
        Err(_) => return error(StatusCode::BAD_GATEWAY, "CEO provider response failed"),
    };

    if !provider_status.is_success() {
        eprintln!(
            "CEO_PROVIDER_HTTP_ERROR status={}",
            provider_status.as_u16()
        );

        return error(StatusCode::BAD_GATEWAY, "CEO provider unavailable");
    }

    let parsed: MetaResponse = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "CEO provider returned invalid response",
            );
        }
    };

    let MetaResponse {
        model,
        choices,
        usage,
    } = parsed;

    let Some(choice) = choices.into_iter().next() else {
        return error(StatusCode::BAD_GATEWAY, "CEO returned no answer");
    };

    let reply = choice
        .message
        .content
        .unwrap_or_default()
        .trim()
        .to_string();

    if reply.is_empty() {
        return error(StatusCode::BAD_GATEWAY, "CEO returned empty answer");
    }

    let cached_tokens = usage
        .and_then(|usage| usage.prompt_tokens_details)
        .and_then(|details| details.cached_tokens)
        .unwrap_or(0);

    json_response(
        StatusCode::OK,
        ChatOutput {
            reply,
            model: model.unwrap_or_else(|| META_MODEL.to_string()),
            cached_tokens,
        },
    )
}

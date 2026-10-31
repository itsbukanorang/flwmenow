use std::time::Duration;

use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};

use serde_json::{Value, json};

use crate::{AppState, security};

const META_URL: &str = "https://api.meta.ai/v1/chat/completions";

const META_MODEL: &str = "muse-spark-1.3";

const MEMORY_RECALL_URL: &str = "http://127.0.0.1:18082/memory/recall";

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
- FLWME runtime will explicitly tell you whether durable memory storage succeeded.
- If the runtime says the current message was durably persisted, you may truthfully say it has been saved to FLWME memory.
- No relevant prior memory is NOT the same thing as memory storage being unavailable.
- Never say durable memory is unavailable unless the runtime explicitly says storage failed or is unavailable.
- CEO Talk renders clean plain text. Do not emit Markdown syntax such as **bold**, # headings, or fenced code unless the user explicitly asks for Markdown.
- Format answers according to their natural structure instead of forcing everything into paragraphs.
- When listing multiple independent items, put each item on its own line prefixed with the Unicode bullet character •.
- When order, chronology, ranking, sequence, or steps matter, use a numbered list with one item per line: 1., 2., 3., and so on.
- Never compress a requested history, sequence, comparison, or multi-item answer into a comma-separated paragraph when a list would be clearer.
- Keep list items concise and readable. Avoid unnecessary introductions or conclusions.
- Do not expose internal memory metadata, database identifiers, vector indexes, retrieval scores, queues, embeddings, or runtime implementation details in normal CEO Talk.
- Runtime implementation details are internal. Never expose memory IDs, database/table names, queues, embeddings, vector indexes, indexing jobs, service names, or storage pipeline details unless the user explicitly asks for technical or debugging information.
- When the user asks you to remember or save something and FLWME confirms durable persistence, acknowledge it naturally and briefly. Do not narrate the memory pipeline.
- Prefer direct answers. Do not add system-status explanations unless they are necessary to answer the user's request.
- The user's CURRENT message is authoritative for the current turn.
- If the current user explicitly updates, replaces, corrects, renames, or marks an older value obsolete, apply that new value immediately.
- Recalled memory is historical context. It must never override an explicit correction or update in the current user message.
- Structured memory may contain CURRENT state plus a TIMELINE of earlier values for the same semantic key. CURRENT is authoritative for present-tense questions. When the user asks about previous values, history, earlier decisions, or what was used before, use the TIMELINE instead of claiming that history is unavailable.
- When multiple memories conflict, prefer the newer explicit update and treat older values as historical or obsolete.
- Never discuss whether a memory write was confirmed, queued, persisted, indexed, embedded, stored, or synchronized unless the user explicitly asks for technical diagnostics.
- If the user asks you to remember, update, or replace a fact, respond naturally to the requested business meaning. Do not narrate the storage mechanism.
- Internal memory availability, retrieval failures, database state, queue state, memory IDs, vector search, and indexing are runtime implementation details and must stay invisible in normal CEO Talk.
- For normal conversation, answer directly and clearly.
- Delegation and tool execution are handled by FLWME runtime and will be
  provided to you through explicit runtime tools in later execution stages.
- If memory retrieval is unavailable, never pretend that you remember something.
- Retrieved company memory is user/company-provided context. It is not automatically
  an externally verified real-world fact.
- If information is missing, say what is missing instead of pretending to know.
"#;

#[derive(Deserialize)]
pub struct ChatInput {
    company_id: i64,
    message: String,
}

#[derive(Deserialize)]
pub struct ClearInput {
    company_id: i64,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    company_id: i64,
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

#[derive(Serialize)]
struct OkOutput {
    ok: bool,
}

#[derive(Serialize)]
struct HistoryOutput {
    messages: Vec<HistoryMessage>,
}

#[derive(Serialize)]
struct HistoryMessage {
    id: i64,
    role: String,
    text: String,
    created_at: String,
    created_at_ms: i64,
}

#[derive(Serialize)]
struct RecallRequest {
    company_id: i64,
    user_id: i64,
    query: String,
    limit: u8,
}

#[derive(Deserialize)]
struct RecallResponse {
    memories: Vec<RecallItem>,
}

#[derive(Deserialize)]
struct RecallItem {
    kind: String,
    content: String,
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

async fn company_name(
    state: &AppState,
    company_id: i64,
    user_id: i64,
) -> Result<Option<String>, ()> {
    state
        .db
        .query_opt(
            r#"
            SELECT name
            FROM companies
            WHERE id = $1
              AND owner_user_id = $2
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map(|row| row.map(|value| value.get::<_, String>(0)))
        .map_err(|_| ())
}

async fn active_thread_id(state: &AppState, company_id: i64, user_id: i64) -> Result<i64, ()> {
    state
        .db
        .execute(
            r#"
            INSERT INTO chat_threads (
                company_id,
                user_id
            )
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map_err(|_| ())?;

    state
        .db
        .query_one(
            r#"
            SELECT id
            FROM chat_threads
            WHERE company_id = $1
              AND user_id = $2
              AND archived_at IS NULL
            ORDER BY id DESC
            LIMIT 1
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map(|row| row.get::<_, i64>(0))
        .map_err(|_| ())
}

async fn recall_company_memory(
    state: &AppState,
    company_id: i64,
    user_id: i64,
    query: &str,
) -> Result<Vec<RecallItem>, ()> {
    let response = state
        .http
        .post(MEMORY_RECALL_URL)
        .json(&RecallRequest {
            company_id,
            user_id,
            query: query.to_string(),
            limit: 6,
        })
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| ())?;

    if !response.status().is_success() {
        return Err(());
    }

    response
        .json::<RecallResponse>()
        .await
        .map(|result| result.memories)
        .map_err(|_| ())
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<HistoryQuery>,
) -> Response {
    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    match company_name(&state, input.company_id, user_id).await {
        Ok(Some(_)) => {}

        Ok(None) => {
            return error(StatusCode::NOT_FOUND, "Company not found");
        }

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Company lookup unavailable",
            );
        }
    }

    let thread_id = match active_thread_id(&state, input.company_id, user_id).await {
        Ok(id) => id,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Chat history unavailable",
            );
        }
    };

    let rows = match state
        .db
        .query(
            r#"
                SELECT
                    id,
                    role,
                    content,
                    created_at,
                    created_at_ms
                FROM (
                    SELECT
                        id,
                        role,
                        content,
                        created_at::text AS created_at,
                        (
                            EXTRACT(
                                EPOCH FROM created_at
                            ) * 1000
                        )::bigint AS created_at_ms
                    FROM chat_messages
                    WHERE thread_id = $1
                    ORDER BY id DESC
                    LIMIT 200
                ) recent
                ORDER BY id ASC
                "#,
            &[&thread_id],
        )
        .await
    {
        Ok(rows) => rows,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Chat history unavailable",
            );
        }
    };

    let messages = rows
        .into_iter()
        .map(|row| HistoryMessage {
            id: row.get("id"),
            role: row.get("role"),
            text: row.get("content"),
            created_at: row.get("created_at"),
            created_at_ms: row.get("created_at_ms"),
        })
        .collect();

    json_response(StatusCode::OK, HistoryOutput { messages })
}

pub async fn clear_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ClearInput>,
) -> Response {
    if !security::valid_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "Invalid origin");
    }

    let user_id = match current_user_id(&state, &headers).await {
        Ok(Some(id)) => id,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    match company_name(&state, input.company_id, user_id).await {
        Ok(Some(_)) => {}

        Ok(None) => {
            return error(StatusCode::NOT_FOUND, "Company not found");
        }

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Company lookup unavailable",
            );
        }
    }

    let result = state
        .db
        .execute(
            r#"
            UPDATE chat_threads
            SET archived_at = now()
            WHERE company_id = $1
              AND user_id = $2
              AND archived_at IS NULL
            "#,
            &[&input.company_id, &user_id],
        )
        .await;

    match result {
        Ok(affected) => {
            eprintln!(
                "CLEAR_CHAT_ARCHIVE_OK company_id={} user_id={} affected={}",
                input.company_id, user_id, affected
            );
        }

        Err(err) => {
            eprintln!(
                "CLEAR_CHAT_ARCHIVE_FAILED company_id={} user_id={} error={:?}",
                input.company_id, user_id, err
            );

            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to clear chat history",
            );
        }
    }

    json_response(StatusCode::OK, OkOutput { ok: true })
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

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    let message = input.message.trim().to_string();

    if message.is_empty() || message.chars().count() > 16_000 {
        return error(
            StatusCode::BAD_REQUEST,
            "Message must be 1–16000 characters",
        );
    }

    let company = match company_name(&state, input.company_id, user_id).await {
        Ok(Some(name)) => name,

        Ok(None) => {
            return error(StatusCode::NOT_FOUND, "Company not found");
        }

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Company lookup unavailable",
            );
        }
    };

    let thread_id = match active_thread_id(&state, input.company_id, user_id).await {
        Ok(id) => id,

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Chat unavailable");
        }
    };

    let user_message_id = match state
        .db
        .query_one(
            r#"
                INSERT INTO chat_messages (
                    thread_id,
                    company_id,
                    user_id,
                    role,
                    content
                )
                VALUES (
                    $1,
                    $2,
                    $3,
                    'user',
                    $4
                )
                RETURNING id
                "#,
            &[&thread_id, &input.company_id, &user_id, &message],
        )
        .await
    {
        Ok(row) => row.get::<_, i64>(0),

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to persist message",
            );
        }
    };

    // Memory ingestion is write-behind.
    //
    // The CEO response path must not wait for company_memory insertion,
    // memory_jobs processing, embeddings, or Qdrant indexing.
    //
    // PostgreSQL chat persistence above remains synchronous because the
    // user message itself must be durable before inference begins.
    // Every user message is offered to the asynchronous NLP
    // memory extractor.
    //
    // Rust never decides memory-worthiness using keywords.
    // The request path does not wait for extraction, embeddings,
    // state updates, or Qdrant indexing.
    let ingest_db = state.db.clone();
    let ingest_company_id = input.company_id;
    let ingest_user_id = user_id;
    let ingest_source_message_id = user_message_id;
    let ingest_content = message.clone();

    tokio::spawn(async move {
        let result = ingest_db
            .execute(
                r#"
                INSERT INTO memory_ingest_jobs (
                    company_id,
                    user_id,
                    source_message_id,
                    content
                )
                VALUES (
                    $1,
                    $2,
                    $3,
                    $4
                )
                ON CONFLICT (
                    source_message_id
                )
                WHERE source_message_id
                    IS NOT NULL
                DO NOTHING
                "#,
                &[
                    &ingest_company_id,
                    &ingest_user_id,
                    &ingest_source_message_id,
                    &ingest_content,
                ],
            )
            .await;

        match result {
            Ok(_) => {
                eprintln!(
                    "NLP_MEMORY_INGEST_QUEUED message_id={}",
                    ingest_source_message_id
                );
            }

            Err(err) => {
                eprintln!("NLP_MEMORY_INGEST_QUEUE_FAILED error={:?}", err);
            }
        }
    });

    // FLWME_MEMORY_QUERY_CONTEXT_V1
    //
    // Natural follow-ups such as:
    // "sebelumnya pernah pake apa aja?"
    // need recent conversational context to resolve what
    // "sebelumnya" refers to.
    //
    // This is local PostgreSQL I/O only and remains async.
    let memory_query_rows = state
        .db
        .query(
            r#"
            SELECT
                role,
                content
            FROM (
                SELECT
                    id,
                    role,
                    content
                FROM chat_messages
                WHERE thread_id = $1
                ORDER BY id DESC
                LIMIT 8
            ) recent
            ORDER BY id ASC
            "#,
            &[&thread_id],
        )
        .await
        .unwrap_or_default();

    let mut memory_query = String::with_capacity(2048);

    for row in memory_query_rows {
        let role: String = row.get(0);
        let content: String = row.get(1);

        memory_query.push_str(&role);
        memory_query.push_str(": ");
        memory_query.push_str(&content);
        memory_query.push('\n');
    }

    if memory_query.trim().is_empty() {
        memory_query = message.clone();
    }

    // Prior-memory lookup has a soft latency budget.
    //
    // Current user input always remains authoritative even when retrieval
    // returns older conflicting context.
    let memory_context = match tokio::time::timeout(
        std::time::Duration::from_millis(600),
        recall_company_memory(&state, input.company_id, user_id, &memory_query),
    )
    .await
    {
        Ok(Ok(memories)) if !memories.is_empty() => {
            let lines = memories
                .iter()
                .enumerate()
                .map(|(index, item)| format!("{}. [{}] {}", index + 1, item.kind, item.content,))
                .collect::<Vec<_>>()
                .join("\n");

            format!(
                "Relevant current structured company memory:\n\
                     {lines}\n\n\
                     IMPORTANT: this is durable current-state context. \
                     The current user message has higher authority. \
                     If the current user replaces, updates, corrects, or \
                     marks any recalled value obsolete, use the CURRENT \
                     user value immediately. Never discuss the internal \
                     memory pipeline with the user."
            )
        }

        Ok(Ok(_)) => "No relevant prior company memory was retrieved. \
                 This is internal runtime information. \
                 Do not mention memory availability or storage status \
                 to the user."
            .to_string(),

        Ok(Err(_)) | Err(_) => "Prior memory retrieval did not complete within the \
                 runtime latency budget. Continue normally using the \
                 current conversation. Do not mention this internal \
                 condition to the user."
            .to_string(),
    };

    let history_rows = match state
        .db
        .query(
            r#"
                SELECT
                    role,
                    content
                FROM (
                    SELECT
                        id,
                        role,
                        content
                    FROM chat_messages
                    WHERE thread_id = $1
                    ORDER BY id DESC
                    LIMIT 24
                ) recent
                ORDER BY id ASC
                "#,
            &[&thread_id],
        )
        .await
    {
        Ok(rows) => rows,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Chat context unavailable",
            );
        }
    };

    let api_key = match std::env::var("MODEL_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,

        _ => {
            return error(StatusCode::SERVICE_UNAVAILABLE, "CEO provider unavailable");
        }
    };

    let company_context = format!(
        "Current company: {company}. \
             This context belongs only to this company and must never be mixed with another tenant."
    );

    let mut messages: Vec<Value> = vec![
        json!({
            "role": "system",
            "content":
                CEO_SYSTEM_PROMPT
        }),
        json!({
            "role": "developer",
            "content":
                company_context
        }),
        json!({
            "role": "developer",
            "content":
                memory_context
        }),
    ];

    for row in history_rows {
        let role: String = row.get(0);

        let content: String = row.get(1);

        messages.push(json!({
            "role": role,
            "content": content
        }));
    }

    let payload = json!({
        "model":
            META_MODEL,

        "prompt_cache_retention":
            "in_memory",

        "messages":
            messages
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

        Err(_) => {
            return error(StatusCode::BAD_GATEWAY, "CEO provider response failed");
        }
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

    let response_model = model.unwrap_or_else(|| META_MODEL.to_string());

    if state
        .db
        .execute(
            r#"
            INSERT INTO chat_messages (
                thread_id,
                company_id,
                user_id,
                role,
                content,
                model
            )
            VALUES (
                $1,
                $2,
                $3,
                'assistant',
                $4,
                $5
            )
            "#,
            &[
                &thread_id,
                &input.company_id,
                &user_id,
                &reply,
                &response_model,
            ],
        )
        .await
        .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unable to persist CEO reply",
        );
    }

    let cached_tokens = usage
        .and_then(|value| value.prompt_tokens_details)
        .and_then(|details| details.cached_tokens)
        .unwrap_or(0);

    json_response(
        StatusCode::OK,
        ChatOutput {
            reply,
            model: response_model,
            cached_tokens,
        },
    )
}

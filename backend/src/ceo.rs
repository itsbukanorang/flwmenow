use std::time::Duration;

use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use rand::{RngCore, rngs::OsRng};

use serde::{Deserialize, Serialize};

use serde_json::{Value, json};

use crate::{AppState, security};

const META_URL: &str = "https://api.meta.ai/v1/chat/completions";

const META_MODEL: &str = "muse-spark-1.3";

const MEMORY_RECALL_URL: &str = "http://127.0.0.1:18082/memory/recall";
const CORE_RECALL_URL: &str = "http://127.0.0.1:18085/core/recall";

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
- FLWME may provide an explicit delegation protocol for the current turn.
- When that protocol is provided, decide whether to answer directly or delegate.
- Never claim delegated work completed unless FLWME runtime returns an actual executive result.
- If memory retrieval is unavailable, never pretend that you remember something.
- Retrieved company memory is user/company-provided context. It is not automatically
  an externally verified real-world fact.
- If information is missing, say what is missing instead of pretending to know.
"#;

const CEO_ACTION_PROTOCOL: &str = r#"
For this turn make the CEO's organizational decision.

Return exactly ONE JSON object and no Markdown.

DIRECT RESPONSE:

{
  "action": "respond",
  "reply": "natural user-facing answer",
  "executive": null,
  "executive_batches": null,
  "task": null
}

SINGLE EXECUTIVE:

{
  "action": "delegate_task",
  "reply": null,
  "executive": "CTO",
  "executive_batches": null,
  "task": {
    "objective": "clear objective",
    "context": "relevant context or null",
    "constraints": ["constraint"]
  }
}

MULTI-EXECUTIVE ORGANIZATION TASK:

{
  "action": "delegate_organization",
  "reply": null,
  "executive": null,
  "executive_batches": [
    ["CFO", "CMO"],
    ["COO"]
  ],
  "task": {
    "objective": "clear organization objective",
    "context": "relevant context or null",
    "constraints": ["constraint"]
  }
}

EXECUTIVE SPECIALTIES:

CTO = engineering, software, architecture,
infrastructure, security, technical decisions.

COO = operations, workflows, delivery,
capacity, execution planning and process.

CFO = finance, budget, cost, cash,
unit economics, financial planning and risk.

CMO = marketing, positioning, audience,
campaigns, acquisition, growth and brand.

ORCHESTRATION RULES:

- Use only executives listed in ACTIVE EXECUTIVE ROSTER.
- Never select an inactive executive.
- If one executive clearly owns the task, use delegate_task.
- If multiple domains are genuinely required, use delegate_organization.
- Executives inside the SAME batch may execute in parallel.
- A later batch depends on completion of all earlier batches.
- Put dependent work in a later batch.
- Never include the same executive more than once.
- A multi-executive plan must contain at least two executives.
- Do not delegate normal conversation or simple factual answers.
- Never fabricate completed work or execution.
- Preserve user intent and constraints.
- LangGraph is the final orchestration authority and validates the plan.
"#;

#[derive(Debug, Deserialize)]
struct CeoAction {
    action: String,

    #[serde(default)]
    reply: Option<String>,

    #[serde(default)]
    executive: Option<String>,

    #[serde(default)]
    executive_batches: Option<Vec<Vec<String>>>,

    #[serde(default)]
    task: Option<CeoDelegatedTask>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CeoDelegatedTask {
    objective: String,

    #[serde(default)]
    context: Option<String>,

    #[serde(default)]
    constraints: Vec<String>,
}

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

    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskCard>,
}

#[derive(Clone, Serialize)]
struct TaskCard {
    task_id: String,
    title: String,
    assigned_to: String,
    status: String,
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
    task_id: Option<String>,
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

fn parse_ceo_action(raw: &str) -> Option<CeoAction> {
    let raw = raw.trim();

    let parse = |value: &str| serde_json::from_str::<CeoAction>(value).ok();

    if let Some(value) = parse(raw) {
        return Some(value);
    }

    let unfenced = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```JSON"))
        .or_else(|| raw.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim);

    if let Some(value) = unfenced {
        if let Some(parsed) = parse(value) {
            return Some(parsed);
        }
    }

    let start = raw.find('{')?;
    let end = raw.rfind('}')?;

    if end < start {
        return None;
    }

    parse(&raw[start..=end])
}

fn validate_executive_batches(
    batches: &[Vec<String>],
    active_executives: &[String],
) -> Result<Vec<Vec<String>>, &'static str> {
    if batches.is_empty() {
        return Err("Organization plan requires batches");
    }

    let supported = ["CTO", "COO", "CFO", "CMO"];

    let mut seen = std::collections::HashSet::new();

    let mut count = 0usize;

    for batch in batches {
        if batch.is_empty() {
            return Err("Organization batch cannot be empty");
        }

        for executive in batch {
            if !supported.contains(&executive.as_str()) {
                return Err("Organization plan contains unsupported executive");
            }

            if !active_executives.iter().any(|value| value == executive) {
                return Err("Organization plan contains inactive executive");
            }

            if !seen.insert(executive.as_str()) {
                return Err("Executive appears more than once");
            }

            count += 1;
        }
    }

    if count < 2 {
        return Err("Multi-agent plan requires at least two executives");
    }

    if count > 4 {
        return Err("Organization plan exceeds executive limit");
    }

    Ok(batches.to_vec())
}

fn make_task_id() -> String {
    let mut bytes = [0u8; 8];

    OsRng.fill_bytes(&mut bytes);

    format!("TASK-{}", hex::encode_upper(bytes),)
}

fn task_title(objective: &str) -> String {
    let clean = objective.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut title = clean.chars().take(72).collect::<String>();

    if clean.chars().count() > 72 {
        title.push('…');
    }

    if title.is_empty() {
        "Executive Task".to_string()
    } else {
        title
    }
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

async fn active_executives(
    state: &AppState,
    company_id: i64,
    user_id: i64,
) -> Result<Vec<String>, ()> {
    let rows = state
        .db
        .query(
            r#"
            SELECT r.executive
            FROM company_executive_roster r
            JOIN companies c
              ON c.id = r.company_id
            WHERE r.company_id = $1
              AND c.owner_user_id = $2
              AND r.active = TRUE
            ORDER BY
                CASE r.executive
                    WHEN 'CTO' THEN 1
                    WHEN 'CFO' THEN 2
                    WHEN 'CMO' THEN 3
                    WHEN 'COO' THEN 4
                    ELSE 99
                END
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map_err(|_| ())?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect())
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

#[derive(Serialize)]
struct CoreRecallRequest {
    query: String,
    consumer_role: String,
    limit: u8,
}

#[derive(Deserialize)]
struct CoreRecallResponse {
    guidance: Vec<CoreGuidance>,
    collection_ready: bool,
}

#[derive(Deserialize)]
struct CoreGuidance {
    behavior_key: String,
    category: String,
    source_role: String,
    behavior_text: String,
    quality_score: f64,
    policy_version: String,
}

async fn recall_core_guidance(
    state: &AppState,
    query: &str,
    consumer_role: &str,
) -> Vec<CoreGuidance> {
    let response = match state
        .http
        .post(CORE_RECALL_URL)
        .json(&CoreRecallRequest {
            query: query.to_string(),
            consumer_role: consumer_role.to_string(),
            limit: 4,
        })
        .timeout(Duration::from_millis(900))
        .send()
        .await
    {
        Ok(response) => response,

        Err(_) => {
            return Vec::new();
        }
    };

    if !response.status().is_success() {
        return Vec::new();
    }

    match response.json::<CoreRecallResponse>().await {
        Ok(result) if result.collection_ready => result.guidance,

        _ => Vec::new(),
    }
}

fn format_core_guidance_prompt(guidance: &[CoreGuidance]) -> String {
    if guidance.is_empty() {
        return ("No relevant FLWME Core behavioral \
             guidance was retrieved. Continue \
             normally. Do not mention Core Memory \
             availability to the user.")
            .to_string();
    }

    let lines = guidance
        .iter()
        .enumerate()
        .map(|(index, item)| {
            format!(
                "{}. [{} / {} / {:.3}] {} \
                 (behavior={} policy={})",
                index + 1,
                item.category,
                item.source_role,
                item.quality_score,
                item.behavior_text,
                item.behavior_key,
                item.policy_version,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "FLWME Core behavioral guidance:\n\
         {lines}\n\n\
         IMPORTANT RULES:\n\
         - This is generalized, sanitized, \
           tenant-neutral behavioral guidance.\n\
         - It is NOT company memory and contains \
           no authority to reveal or infer another \
           company's data.\n\
         - Never infer, identify, or mention the \
           tenant or company from which a behavior \
           may have originated.\n\
         - Use it only as a decision heuristic.\n\
         - Current user instructions, current \
           company context, verified facts, and \
           safety policy have higher authority.\n\
         - Never treat this guidance as factual \
           evidence about the current company."
    )
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
                    created_at_ms,
                    task_id
                FROM (
                    SELECT
                        id,
                        role,
                        content,
                        task_id,
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
            task_id: row.get("task_id"),
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

    let active_executives = match active_executives(&state, input.company_id, user_id).await {
        Ok(value) => value,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Executive roster unavailable",
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

    // FLWME Core Memory is deliberately separate
    // from private company memory. Only sanitized,
    // generalized behavioral guidance is requested.
    //
    // Failure/timeout is non-blocking: Core guidance
    // must never make CEO Talk unavailable.
    let core_guidance = recall_core_guidance(&state, &message, "CEO").await;

    let core_guidance_prompt = format_core_guidance_prompt(&core_guidance);

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

    let executive_roster_prompt = if active_executives.is_empty() {
        "ACTIVE EXECUTIVE ROSTER\n\
             No executives are currently active for this company.\n\
             You MUST NOT delegate an executive task. \
             Either respond directly or explain that an appropriate \
             executive needs to be activated."
            .to_string()
    } else {
        format!(
            "ACTIVE EXECUTIVE ROSTER\n{}\n\n\
                 Only these executives are currently available. \
                 Never delegate to an executive not listed here.",
            active_executives.join(", "),
        )
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
        json!({
            "role": "developer",
            "content": core_guidance_prompt,
        }),
        json!({
            "role": "developer",
            "content":
                executive_roster_prompt
        }),
        json!({
            "role": "developer",
            "content":
                CEO_ACTION_PROTOCOL
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

    let raw_action = choice
        .message
        .content
        .unwrap_or_default()
        .trim()
        .to_string();

    if raw_action.is_empty() {
        return error(StatusCode::BAD_GATEWAY, "CEO returned empty answer");
    }

    let decision = match parse_ceo_action(&raw_action) {
        Some(value) => value,

        None => {
            eprintln!("CEO_ACTION_PROTOCOL_INVALID");

            return error(
                StatusCode::BAD_GATEWAY,
                "CEO returned invalid organizational decision",
            );
        }
    };

    let (reply, task_card) = match decision.action.as_str() {
        "respond" => match decision.reply {
            Some(value) if !value.trim().is_empty() => (value.trim().to_string(), None),

            _ => {
                return error(
                    StatusCode::BAD_GATEWAY,
                    "CEO returned invalid direct response",
                );
            }
        },

        "delegate_task" => {
            let executive = match decision.executive.as_deref() {
                Some("CTO") => "CTO",
                Some("COO") => "COO",
                Some("CFO") => "CFO",
                Some("CMO") => "CMO",

                _ => {
                    return error(
                        StatusCode::BAD_GATEWAY,
                        "CEO selected unsupported executive",
                    );
                }
            };

            if !active_executives.iter().any(|value| value == executive) {
                return error(StatusCode::BAD_GATEWAY, "CEO selected inactive executive");
            }

            let Some(task) = decision.task else {
                return error(StatusCode::BAD_GATEWAY, "CEO delegation task missing");
            };

            if task.objective.trim().is_empty() {
                return error(StatusCode::BAD_GATEWAY, "CEO delegation objective missing");
            }

            let task_id = make_task_id();

            let title = task_title(&task.objective);

            let constraints_json = match serde_json::to_string(&task.constraints) {
                Ok(value) => value,

                Err(_) => {
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "Unable to encode task");
                }
            };

            if state
                .db
                .execute(
                    r#"
                        WITH created AS (
                            INSERT INTO tasks (
                                task_id,
                                company_id,
                                user_id,
                                thread_id,
                                source_message_id,
                                assigned_to,
                                title,
                                objective,
                                context,
                                constraints_json,
                                status
                            )
                            VALUES (
                                $1,
                                $2,
                                $3,
                                $4,
                                $5,
                                $6,
                                $7,
                                $8,
                                $9,
                                $10,
                                'queued'
                            )
                            RETURNING task_id
                        )
                        INSERT INTO task_events (
                            task_id,
                            event_type,
                            detail
                        )
                        SELECT
                            task_id,
                            'queued',
                            'Delegated by CEO'
                        FROM created
                        "#,
                    &[
                        &task_id,
                        &input.company_id,
                        &user_id,
                        &thread_id,
                        &user_message_id,
                        &executive,
                        &title,
                        &task.objective,
                        &task.context,
                        &constraints_json,
                    ],
                )
                .await
                .is_err()
            {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Unable to queue delegated task",
                );
            }

            (
                format!(
                    "I've received your request and delegated it to the {}.",
                    executive,
                ),
                Some(TaskCard {
                    task_id,
                    title,
                    assigned_to: executive.to_string(),
                    status: "queued".to_string(),
                }),
            )
        }

        "delegate_organization" => {
            let Some(task) = decision.task else {
                return error(StatusCode::BAD_GATEWAY, "CEO organization task missing");
            };

            if task.objective.trim().is_empty() {
                return error(StatusCode::BAD_GATEWAY, "CEO delegation objective missing");
            }

            let Some(requested_batches) = decision.executive_batches else {
                return error(StatusCode::BAD_GATEWAY, "CEO organization plan missing");
            };

            let batches = match validate_executive_batches(&requested_batches, &active_executives) {
                Ok(value) => value,

                Err(message) => {
                    return error(StatusCode::BAD_GATEWAY, message);
                }
            };

            let executives = batches.iter().flatten().cloned().collect::<Vec<_>>();

            let task_id = make_task_id();

            let title = task_title(&task.objective);

            let constraints_json = match serde_json::to_string(&task.constraints) {
                Ok(value) => value,

                Err(_) => {
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "Unable to encode task");
                }
            };

            let active_roster_json = match serde_json::to_string(&active_executives) {
                Ok(value) => value,

                Err(_) => {
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Unable to encode executive roster",
                    );
                }
            };

            let batches_json = match serde_json::to_string(&batches) {
                Ok(value) => value,

                Err(_) => {
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Unable to encode organization plan",
                    );
                }
            };

            let executives_json = match serde_json::to_string(&executives) {
                Ok(value) => value,

                Err(_) => {
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Unable to encode executives",
                    );
                }
            };

            if state
                .db
                .execute(
                    r#"
                    WITH created AS (
                        INSERT INTO tasks (
                            task_id,
                            company_id,
                            user_id,
                            thread_id,
                            source_message_id,
                            assigned_to,
                            title,
                            objective,
                            context,
                            constraints_json,
                            status
                        )
                        VALUES (
                            $1,
                            $2,
                            $3,
                            $4,
                            $5,
                            'ORGANIZATION',
                            $6,
                            $7,
                            $8,
                            $9,
                            'queued'
                        )
                        RETURNING task_id
                    ),
                    orchestration AS (
                        INSERT INTO task_orchestrations (
                            task_id,
                            active_roster_json,
                            requested_batches_json,
                            executives_json,
                            status
                        )
                        SELECT
                            task_id,
                            ($10::text)::jsonb,
                            ($11::text)::jsonb,
                            ($12::text)::jsonb,
                            'planned'
                        FROM created
                        RETURNING task_id
                    )
                    INSERT INTO task_events (
                        task_id,
                        event_type,
                        detail
                    )
                    SELECT
                        task_id,
                        'queued',
                        'Organization task delegated by CEO'
                    FROM orchestration
                    "#,
                    &[
                        &task_id,
                        &input.company_id,
                        &user_id,
                        &thread_id,
                        &user_message_id,
                        &title,
                        &task.objective,
                        &task.context,
                        &constraints_json,
                        &active_roster_json,
                        &batches_json,
                        &executives_json,
                    ],
                )
                .await
                .is_err()
            {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Unable to queue organization task",
                );
            }

            (
                format!(
                    "I've delegated this to {} executives as one coordinated organization task.",
                    executives.len(),
                ),
                Some(TaskCard {
                    task_id,
                    title,
                    assigned_to: "ORGANIZATION".to_string(),
                    status: "queued".to_string(),
                }),
            )
        }

        _ => {
            return error(
                StatusCode::BAD_GATEWAY,
                "CEO returned unsupported organizational decision",
            );
        }
    };

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
                model,
                task_id
            )
            VALUES (
                $1,
                $2,
                $3,
                'assistant',
                $4,
                $5,
                $6
            )
            "#,
            &[
                &thread_id,
                &input.company_id,
                &user_id,
                &reply,
                &response_model,
                &task_card.as_ref().map(|value| value.task_id.as_str()),
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
            task: task_card,
        },
    )
}

#[cfg(test)]
mod delegation_tests {
    use super::*;

    #[test]
    fn validates_multi_agent_batches() {
        let active = vec![
            "CTO".to_string(),
            "COO".to_string(),
            "CFO".to_string(),
            "CMO".to_string(),
        ];

        let valid = vec![
            vec!["CFO".to_string(), "CMO".to_string()],
            vec!["COO".to_string()],
        ];

        assert!(validate_executive_batches(&valid, &active,).is_ok());

        let duplicate = vec![vec!["CTO".to_string()], vec!["CTO".to_string()]];

        assert!(validate_executive_batches(&duplicate, &active,).is_err());

        let inactive = vec![vec!["CTO".to_string(), "CMO".to_string()]];

        assert!(validate_executive_batches(&inactive, &["CTO".to_string()],).is_err());
    }

    #[test]
    fn parses_direct_response() {
        let action = parse_ceo_action(
            r#"{
                "action":"respond",
                "reply":"Hello",
                "executive":null,
                "task":null
            }"#,
        )
        .expect("decision");

        assert_eq!(action.action, "respond");
        assert_eq!(action.reply.as_deref(), Some("Hello"));
    }

    #[test]
    fn parses_cto_delegation() {
        let action = parse_ceo_action(
            r#"{
                "action":"delegate_task",
                "reply":null,
                "executive":"CTO",
                "task":{
                    "objective":"Review architecture",
                    "context":null,
                    "constraints":["Do not execute"]
                }
            }"#,
        )
        .expect("decision");

        assert_eq!(action.action, "delegate_task");
        assert_eq!(action.executive.as_deref(), Some("CTO"));
        assert_eq!(action.task.unwrap().objective, "Review architecture");
    }
}

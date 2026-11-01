use std::time::Duration;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AppState, security};

const BASETEN_CHAT_URL: &str = "https://inference.baseten.co/v1/chat/completions";

const BASETEN_MODEL: &str = "meta-models/muse-spark-open-1026";

const MEMORY_RECALL_URL: &str = "http://127.0.0.1:18082/memory/recall";
const CORE_RECALL_URL: &str = "http://127.0.0.1:18085/core/recall";

const CTO_SYSTEM_PROMPT: &str = r#"
You are FLWME's Chief Technology Officer.

You receive a typed executive task from the FLWME runtime.

Your responsibility at this stage is executive reasoning only:
- understand the technical objective;
- use the supplied company context;
- use supplied relevant company memory when applicable;
- identify risks;
- recommend the next technical actions;
- decide whether real worker execution will eventually be required.

Important rules:
- Never claim that code, deployment, browsing, files, tools, tests,
  infrastructure changes, or other real-world work has been executed.
- You do not have worker tools in this stage.
- You do not delegate tasks yourself.
- LangGraph will handle delegation in a later runtime stage.
- Never fabricate evidence or completed work.
- Never invent company facts or memory.
- Current task input has higher authority than historical memory.
- Keep all tenant/company context isolated.
- Do not reveal internal database IDs or memory implementation details.
- Think internally, but the final answer MUST be exactly one JSON object.
- Do not wrap the final JSON in Markdown.

The final JSON object MUST use exactly this shape:

{
  "assessment": "string",
  "recommendations": ["string"],
  "risks": ["string"],
  "requires_worker": true,
  "worker_brief": "string or null"
}

If no worker execution is needed, requires_worker must be false and
worker_brief must be null.
"#;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ExecutiveTask {
    pub(crate) objective: String,

    #[serde(default)]
    pub(crate) context: Option<String>,

    #[serde(default)]
    pub(crate) constraints: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CtoRunInput {
    company_id: i64,
    task: ExecutiveTask,
}

#[derive(Debug, Deserialize, Serialize)]
struct CtoResult {
    assessment: String,
    recommendations: Vec<String>,
    risks: Vec<String>,
    requires_worker: bool,
    worker_brief: Option<String>,
}

#[derive(Serialize)]
struct CtoRunOutput {
    executive: &'static str,
    role: &'static str,
    model: String,
    memory_items_used: usize,
    result: CtoResult,
}

#[derive(Serialize)]
struct ErrorOutput {
    error: String,
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
struct BasetenResponse {
    model: Option<String>,
    choices: Vec<BasetenChoice>,
}

#[derive(Deserialize)]
struct BasetenChoice {
    message: BasetenMessage,
}

#[derive(Deserialize)]
struct BasetenMessage {
    content: Option<String>,
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

fn valid_text(value: &str, max_chars: usize) -> bool {
    let length = value.trim().chars().count();

    length > 0
        && length <= max_chars
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

fn validate_task(task: &ExecutiveTask) -> Result<(), &'static str> {
    if !valid_text(&task.objective, 8_000) {
        return Err("Task objective must be 1–8000 characters");
    }

    if let Some(context) = &task.context {
        if !valid_text(context, 8_000) {
            return Err("Task context must be 1–8000 characters");
        }
    }

    if task.constraints.len() > 16 {
        return Err("Task supports at most 16 constraints");
    }

    if task.constraints.iter().any(|item| !valid_text(item, 1_000)) {
        return Err("Each constraint must be 1–1000 characters");
    }

    Ok(())
}

fn validate_result(result: CtoResult) -> Option<CtoResult> {
    if !valid_text(&result.assessment, 8_000) {
        return None;
    }

    if result.recommendations.len() > 16
        || result
            .recommendations
            .iter()
            .any(|value| !valid_text(value, 2_000))
    {
        return None;
    }

    if result.risks.len() > 16 || result.risks.iter().any(|value| !valid_text(value, 2_000)) {
        return None;
    }

    match (result.requires_worker, result.worker_brief.as_deref()) {
        (true, Some(value)) if valid_text(value, 4_000) => {}

        (true, _) => {
            return None;
        }

        (false, None) => {}

        (false, Some(_)) => {
            return None;
        }
    }

    Some(result)
}

fn parse_cto_result(raw: &str) -> Option<CtoResult> {
    let raw = raw.trim();

    if let Ok(result) = serde_json::from_str::<CtoResult>(raw) {
        return validate_result(result);
    }

    let unfenced = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```JSON"))
        .or_else(|| raw.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim);

    if let Some(value) = unfenced {
        if let Ok(result) = serde_json::from_str::<CtoResult>(value) {
            return validate_result(result);
        }
    }

    // Muse Spark Open reasons internally. If provider text
    // surrounds the final object with reasoning text, only
    // accept a complete JSON object from the outermost braces.
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;

    if end < start {
        return None;
    }

    serde_json::from_str::<CtoResult>(&raw[start..=end])
        .ok()
        .and_then(validate_result)
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

async fn company_context(
    state: &AppState,
    company_id: i64,
    user_id: i64,
) -> Result<Option<(String, String)>, ()> {
    state
        .db
        .query_opt(
            r#"
            SELECT
                name,
                status
            FROM companies
            WHERE id = $1
              AND owner_user_id = $2
            "#,
            &[&company_id, &user_id],
        )
        .await
        .map(|row| row.map(|value| (value.get::<_, String>(0), value.get::<_, String>(1))))
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
        .map(|value| value.memories)
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

pub(crate) async fn execute_cto_for_user(
    state: &AppState,
    company_id: i64,
    user_id: i64,
    task: &ExecutiveTask,
) -> Result<Value, String> {
    if let Err(message) = validate_task(task) {
        return Err(message.to_string());
    }

    let (company_name, company_status) = company_context(state, company_id, user_id)
        .await
        .map_err(|_| "Company context unavailable".to_string())?
        .ok_or_else(|| "Company not found".to_string())?;

    let mut recall_query = task.objective.clone();

    if let Some(context) = task.context.as_deref() {
        recall_query.push('\n');
        recall_query.push_str(context);
    }

    let memories = recall_company_memory(state, company_id, user_id, &recall_query)
        .await
        .map_err(|_| "Executive memory unavailable".to_string())?;

    let memory_context = if memories.is_empty() {
        "No relevant company memory was retrieved.".to_string()
    } else {
        memories
            .iter()
            .enumerate()
            .map(|(index, item)| format!("{}. [{}]\n{}", index + 1, item.kind, item.content,))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    // Core behavioral guidance is global only
    // after sanitization/generalization. It is
    // never private company memory.
    //
    // Core recall is intentionally best-effort:
    // an unavailable Core service must not block
    // executive execution.
    let core_guidance = recall_core_guidance(state, &task.objective, "CTO").await;

    let core_guidance_prompt = format_core_guidance_prompt(&core_guidance);

    let baseten_key = std::env::var("BASETEN_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Executive provider unavailable".to_string())?;

    let company_prompt = format!(
        "COMPANY CONTEXT\n\
             Name: {company_name}\n\
             Status: {company_status}\n\n\
             This context belongs only to this company. \
             Never mix it with another tenant."
    );

    let memory_prompt = format!(
        "RELEVANT COMPANY MEMORY\n\
             {memory_context}\n\n\
             Treat CURRENT memory state as authoritative historical context, \
             but the current typed task has higher authority if it explicitly \
             changes or corrects an older value."
    );

    let task_json =
        serde_json::to_string(task).map_err(|_| "Unable to encode executive task".to_string())?;

    let messages: Vec<Value> = vec![
        json!({
            "role": "system",
            "content":
                CTO_SYSTEM_PROMPT,
        }),
        json!({
            "role": "developer",
            "content":
                company_prompt,
        }),
        json!({
            "role": "developer",
            "content":
                memory_prompt,
        }),
        json!({
            "role": "developer",
            "content": core_guidance_prompt,
        }),
        json!({
            "role": "user",
            "content":
                format!(
                    "TYPED EXECUTIVE TASK\n{}",
                    task_json,
                ),
        }),
    ];

    let payload = json!({
        "model":
            BASETEN_MODEL,
        "messages":
            messages,
    });

    let response = state
        .http
        .post(BASETEN_CHAT_URL)
        .bearer_auth(baseten_key)
        .json(&payload)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|_| "Executive provider request failed".to_string())?;

    if !response.status().is_success() {
        return Err("Executive provider unavailable".to_string());
    }

    let parsed = response
        .json::<BasetenResponse>()
        .await
        .map_err(|_| "Executive provider returned invalid response".to_string())?;

    let BasetenResponse { model, choices } = parsed;

    let choice = choices
        .into_iter()
        .next()
        .ok_or_else(|| "Executive returned no answer".to_string())?;

    let raw = choice.message.content.unwrap_or_default();

    let result = parse_cto_result(&raw)
        .ok_or_else(|| "Executive returned invalid structured result".to_string())?;

    Ok(json!({
        "executive":
            "CTO",
        "role":
            "Chief Technology Officer",
        "model":
            model.unwrap_or_else(
                ||
                    BASETEN_MODEL
                        .to_string()
            ),
        "memory_items_used":
            memories.len(),
        "result":
            result,
    }))
}

pub async fn run_cto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CtoRunInput>,
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
        Ok(Some(id)) => id,

        Ok(None) => {
            return error(StatusCode::UNAUTHORIZED, "Not authenticated");
        }

        Err(_) => {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Session unavailable");
        }
    };

    let (company_name, company_status) =
        match company_context(&state, input.company_id, user_id).await {
            Ok(Some(value)) => value,

            Ok(None) => {
                return error(StatusCode::NOT_FOUND, "Company not found");
            }

            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Company context unavailable",
                );
            }
        };

    let mut recall_query = input.task.objective.clone();

    if let Some(context) = input.task.context.as_deref() {
        recall_query.push('\n');
        recall_query.push_str(context);
    }

    let memories =
        match recall_company_memory(&state, input.company_id, user_id, &recall_query).await {
            Ok(value) => value,

            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Executive memory unavailable",
                );
            }
        };

    let memory_context = if memories.is_empty() {
        "No relevant company memory was retrieved.".to_string()
    } else {
        memories
            .iter()
            .enumerate()
            .map(|(index, item)| format!("{}. [{}]\n{}", index + 1, item.kind, item.content,))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    // Separate tenant-neutral Core behavioral
    // guidance from private company memory.
    let core_guidance = recall_core_guidance(&state, &input.task.objective, "CTO").await;

    let core_guidance_prompt = format_core_guidance_prompt(&core_guidance);

    let baseten_key = match std::env::var("BASETEN_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,

        _ => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Executive provider unavailable",
            );
        }
    };

    let company_prompt = format!(
        "COMPANY CONTEXT\n\
         Name: {company_name}\n\
         Status: {company_status}\n\n\
         This context belongs only to this company. \
         Never mix it with another tenant."
    );

    let memory_prompt = format!(
        "RELEVANT COMPANY MEMORY\n\
         {memory_context}\n\n\
         Treat CURRENT memory state as authoritative historical context, \
         but the current typed task has higher authority if it explicitly \
         changes or corrects an older value."
    );

    let task_json = match serde_json::to_string(&input.task) {
        Ok(value) => value,

        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to encode executive task",
            );
        }
    };

    let messages: Vec<Value> = vec![
        json!({
            "role": "system",
            "content": CTO_SYSTEM_PROMPT,
        }),
        json!({
            "role": "developer",
            "content": company_prompt,
        }),
        json!({
            "role": "developer",
            "content": memory_prompt,
        }),
        json!({
            "role": "developer",
            "content": core_guidance_prompt,
        }),
        json!({
            "role": "user",
            "content": format!(
                "TYPED EXECUTIVE TASK\n{}",
                task_json,
            ),
        }),
    ];

    let payload = json!({
        "model": BASETEN_MODEL,
        "messages": messages,
    });

    let provider_response = match state
        .http
        .post(BASETEN_CHAT_URL)
        .bearer_auth(baseten_key)
        .json(&payload)
        .timeout(Duration::from_secs(120))
        .send()
        .await
    {
        Ok(response) => response,

        Err(err) => {
            eprintln!(
                "CTO_PROVIDER_REQUEST_FAILED kind={}",
                if err.is_timeout() {
                    "timeout"
                } else {
                    "network"
                }
            );

            return error(StatusCode::BAD_GATEWAY, "Executive provider request failed");
        }
    };

    let provider_status = provider_response.status();

    let body = match provider_response.bytes().await {
        Ok(value) => value,

        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "Executive provider response failed",
            );
        }
    };

    if !provider_status.is_success() {
        eprintln!(
            "CTO_PROVIDER_HTTP_ERROR status={}",
            provider_status.as_u16(),
        );

        return error(StatusCode::BAD_GATEWAY, "Executive provider unavailable");
    }

    let parsed: BasetenResponse = match serde_json::from_slice(&body) {
        Ok(value) => value,

        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "Executive provider returned invalid response",
            );
        }
    };

    let BasetenResponse { model, choices } = parsed;

    let Some(choice) = choices.into_iter().next() else {
        return error(StatusCode::BAD_GATEWAY, "Executive returned no answer");
    };

    let raw = choice.message.content.unwrap_or_default();

    let Some(result) = parse_cto_result(&raw) else {
        eprintln!("CTO_STRUCTURED_RESULT_INVALID");

        return error(
            StatusCode::BAD_GATEWAY,
            "Executive returned invalid structured result",
        );
    };

    json_response(
        StatusCode::OK,
        CtoRunOutput {
            executive: "CTO",
            role: "Chief Technology Officer",
            model: model.unwrap_or_else(|| BASETEN_MODEL.to_string()),
            memory_items_used: memories.len(),
            result,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_task_validation() {
        let valid = ExecutiveTask {
            objective: "Review our deployment architecture".to_string(),
            context: Some("Production uses Rust".to_string()),
            constraints: vec!["Do not deploy".to_string()],
        };

        assert!(validate_task(&valid).is_ok());

        let invalid = ExecutiveTask {
            objective: "".to_string(),
            context: None,
            constraints: vec![],
        };

        assert!(validate_task(&invalid).is_err());
    }

    #[test]
    fn structured_result_parses() {
        let raw = r#"
        {
          "assessment":
            "Architecture is viable.",
          "recommendations":
            ["Keep boundaries typed."],
          "risks":
            ["Provider failure."],
          "requires_worker": true,
          "worker_brief":
            "Run implementation tests."
        }
        "#;

        let result = parse_cto_result(raw).expect("valid CTO result");

        assert!(result.requires_worker);

        assert_eq!(result.recommendations.len(), 1);
    }

    #[test]
    fn rejects_fabricated_worker_shape() {
        let raw = r#"
        {
          "assessment": "Looks good.",
          "recommendations": [],
          "risks": [],
          "requires_worker": true,
          "worker_brief": null
        }
        "#;

        assert!(parse_cto_result(raw).is_none());
    }

    #[test]
    fn fenced_json_is_tolerated() {
        let raw = r#"```json
{
  "assessment": "Review complete.",
  "recommendations": [],
  "risks": [],
  "requires_worker": false,
  "worker_brief": null
}
```"#;

        assert!(parse_cto_result(raw).is_some());
    }
}

use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

const ORIGIN: &str = "https://api.x.ai/v1";
const MODEL_ENV: &str = "BOXOLOGY_XAI_MODEL";
const KEY_ENV: &str = "BOXOLOGY_XAI_API_KEY";
const MAX_BODY: u64 = 1024 * 1024;
type Failure = Box<CompletionFailure>;

#[rustfmt::skip]
boxology::contract! {
    pub enum MessageRole { System, User, Assistant, Tool }
    pub enum FinishReason { Stop, ToolCalls, Length }
    pub enum CompletionFailureClass { Configuration, Input, Local, Transient, Permanent, Malformed }
    pub struct ToolCall { pub id: String, pub name: String, pub arguments_json: String }
    pub struct CompletionMessage { pub role: MessageRole, pub content: Option<String>, pub tool_call_id: Option<String>, pub name: Option<String>, pub tool_calls: Vec<ToolCall> }
    pub struct ToolDefinition { pub name: String, pub description: String, pub input_schema_json: String }
    pub struct TokenUsage { pub input_tokens: u64, pub output_tokens: u64, pub total_tokens: u64 }
    pub struct CompletionRequest { pub messages: Vec<CompletionMessage>, pub tools: Vec<ToolDefinition>, pub max_output_tokens: Option<u64> }
    pub struct CompletionResult { pub content: Option<String>, pub tool_calls: Vec<ToolCall>, pub finish_reason: FinishReason, pub usage: TokenUsage }
    pub struct CompletionFailure { pub class: CompletionFailureClass, pub code: String, pub message: String, pub retryable: bool, pub retry_after_seconds: Option<u64> }
    pub struct CompletionOutcome { pub completion: Option<CompletionResult>, pub failure: Option<CompletionFailure> }
    #[error]
    pub enum CompleteError { Internal }
    #[capability]
    pub async fn complete(request: CompletionRequest) -> Result<CompletionOutcome, CompleteError>;
}

pub struct XaiCompletionService {
    client: Client,
    model: String,
    key: String,
    origin: String,
}

impl XaiCompletionService {
    pub fn from_env() -> Result<Self, Box<CompletionFailure>> {
        let missing = || configuration("config_missing");
        Self::build(
            std::env::var(MODEL_ENV).map_err(|_| missing())?,
            std::env::var(KEY_ENV).map_err(|_| missing())?,
            ORIGIN.into(),
        )
    }

    fn build(model: String, key: String, origin: String) -> Result<Self, Failure> {
        if [&model, &key]
            .into_iter()
            .any(|value| value.is_empty() || value.chars().any(char::is_whitespace))
        {
            return Err(configuration("config_invalid"));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(origin == ORIGIN)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| simple(CompletionFailureClass::Local, "client_unavailable", false))?;
        Ok(Self {
            client,
            model,
            key,
            origin,
        })
    }

    #[cfg(test)]
    fn test(origin: String) -> Self {
        Self::build("grok-test".into(), "local-test-key".into(), origin).unwrap()
    }
}

#[boxology::implementation]
impl XaiCompletionService {
    pub async fn complete(
        &self,
        _context: boxology::CallContext,
        request: CompletionRequest,
    ) -> Result<CompletionOutcome, CompleteError> {
        let body = match encode(&self.model, request) {
            Ok(value) => value,
            Err(error) => return Ok(outcome(Err(error))),
        };
        let client = self.client.clone();
        let key = self.key.clone();
        let origin = self.origin.clone();
        let result = tokio::task::spawn_blocking(move || {
            match client
                .post(format!("{origin}/chat/completions"))
                .bearer_auth(key)
                .json(&body)
                .send()
            {
                Ok(response) => decode(response),
                Err(_) => Err(simple(CompletionFailureClass::Transient, "transport", true)),
            }
        })
        .await
        .map_err(|_| CompleteError::Internal)?;
        Ok(outcome(result))
    }
}

fn encode(model: &str, request: CompletionRequest) -> Result<Value, Failure> {
    if request.messages.is_empty() || request.max_output_tokens == Some(0) {
        return Err(input());
    }
    let follows_tool = matches!(
        request.messages.last().map(|message| &message.role),
        Some(MessageRole::Tool)
    );
    let mut outstanding = Vec::<(String, String)>::new();
    let messages = request
        .messages
        .into_iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
                MessageRole::Unknown { .. } => return Err(input()),
            };
            if role != "tool" && !outstanding.is_empty() {
                return Err(input());
            }
            let tool_calls = message
                .tool_calls
                .into_iter()
                .map(|call| {
                    let arguments: Value =
                        serde_json::from_str(&call.arguments_json).map_err(|_| input())?;
                    if call.id.is_empty() || call.name.is_empty() || !arguments.is_object() {
                        return Err(input());
                    }
                    if role == "assistant" {
                        if outstanding.iter().any(|(id, _)| id == &call.id) {
                            return Err(input());
                        }
                        outstanding.push((call.id.clone(), call.name.clone()));
                    }
                    Ok(json!({"id":call.id,"type":"function","function":{
                        "name":call.name,"arguments":call.arguments_json}}))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let valid = match role {
                "system" | "user" => {
                    message.content.is_some()
                        && message.tool_call_id.is_none()
                        && message.name.is_none()
                        && tool_calls.is_empty()
                }
                "assistant" => {
                    message.tool_call_id.is_none()
                        && message.name.is_none()
                        && (message.content.is_some() || !tool_calls.is_empty())
                }
                "tool" => {
                    message.content.is_some()
                        && message.tool_call_id.as_ref().is_some_and(|v| !v.is_empty())
                        && message.name.as_ref().is_none_or(|v| !v.is_empty())
                        && tool_calls.is_empty()
                }
                _ => false,
            };
            if !valid {
                return Err(input());
            }
            if role == "tool" {
                let id = message.tool_call_id.as_ref().expect("validated tool id");
                let Some(index) = outstanding.iter().position(|(pending, _)| pending == id) else {
                    return Err(input());
                };
                if message
                    .name
                    .as_ref()
                    .is_some_and(|name| name != &outstanding[index].1)
                {
                    return Err(input());
                }
                outstanding.remove(index);
            }
            let mut value = json!({"role": role, "content": message.content});
            if let Some(id) = message.tool_call_id {
                value["tool_call_id"] = json!(id);
            }
            if let Some(name) = message.name {
                value["name"] = json!(name);
            }
            if !tool_calls.is_empty() {
                value["tool_calls"] = json!(tool_calls);
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !outstanding.is_empty() {
        return Err(input());
    }
    let tools = request
        .tools
        .into_iter()
        .map(|tool| {
            let schema: Value =
                serde_json::from_str(&tool.input_schema_json).map_err(|_| input())?;
            if tool.name.is_empty() || !schema.is_object() {
                return Err(input());
            }
            Ok(json!({"type":"function","function":{"name":tool.name,
                "description":tool.description,"parameters":schema}}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut value = json!({"model": model, "messages": messages});
    if !tools.is_empty() {
        value["tools"] = json!(tools);
        value["tool_choice"] = json!(if follows_tool { "none" } else { "required" });
        value["parallel_tool_calls"] = json!(false);
    }
    if let Some(max) = request.max_output_tokens {
        value["max_tokens"] = json!(max);
    }
    Ok(value)
}

fn decode(mut response: Response) -> Result<CompletionResult, Failure> {
    let status = response.status().as_u16();
    let retry = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    match status {
        429 => {
            return Err(fail(
                CompletionFailureClass::Transient,
                "rate_limited",
                true,
                retry,
            ));
        }
        401 | 403 => {
            return Err(permanent("authorization"));
        }
        500..=599 => {
            return Err(simple(
                CompletionFailureClass::Transient,
                "provider_unavailable",
                true,
            ));
        }
        200..=299 => {}
        _ => {
            return Err(permanent("provider_rejected"));
        }
    }
    let mut body = Vec::new();
    response
        .by_ref()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|_| simple(CompletionFailureClass::Transient, "transport", true))?;
    if body.len() as u64 > MAX_BODY {
        return Err(malformed());
    }
    let wire: Value = serde_json::from_slice(&body).map_err(|_| malformed())?;
    let Some([choice]) = wire["choices"].as_array().map(Vec::as_slice) else {
        return Err(malformed());
    };
    if choice["index"].as_u64() != Some(0)
        || choice.pointer("/message/role").and_then(Value::as_str) != Some("assistant")
    {
        return Err(malformed());
    }
    let content = match choice.pointer("/message/content") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.is_empty() => None,
        Some(Value::String(value)) => Some(value.clone()),
        _ => return Err(malformed()),
    };
    let finish_reason = match choice["finish_reason"].as_str() {
        Some("stop") => FinishReason::Stop,
        Some("tool_calls") => FinishReason::ToolCalls,
        Some("length") => FinishReason::Length,
        _ => return Err(malformed()),
    };
    let raw_calls = match choice.pointer("/message/tool_calls") {
        None => &[][..],
        Some(Value::Array(calls)) => calls.as_slice(),
        _ => return Err(malformed()),
    };
    let mut tool_calls = Vec::<ToolCall>::new();
    for call in raw_calls {
        let id = call["id"].as_str().ok_or_else(malformed)?;
        let name = call
            .pointer("/function/name")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        let raw_arguments = call
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        let arguments: Value = serde_json::from_str(raw_arguments).map_err(|_| malformed())?;
        if call["type"].as_str() != Some("function")
            || id.is_empty()
            || name.is_empty()
            || !arguments.is_object()
            || tool_calls.iter().any(|prior| prior.id == id)
        {
            return Err(malformed());
        }
        tool_calls.push(ToolCall {
            id: id.into(),
            name: name.into(),
            arguments_json: raw_arguments.into(),
        });
    }
    if matches!(finish_reason, FinishReason::ToolCalls) != !tool_calls.is_empty() {
        return Err(malformed());
    }
    Ok(CompletionResult {
        content,
        tool_calls,
        finish_reason,
        usage: TokenUsage {
            input_tokens: wire
                .pointer("/usage/prompt_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(malformed)?,
            output_tokens: wire
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(malformed)?,
            total_tokens: wire
                .pointer("/usage/total_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(malformed)?,
        },
    })
}

fn input() -> Failure {
    simple(CompletionFailureClass::Input, "input_invalid", false)
}
fn malformed() -> Failure {
    simple(
        CompletionFailureClass::Malformed,
        "response_malformed",
        false,
    )
}
fn simple(class: CompletionFailureClass, code: &str, retryable: bool) -> Failure {
    fail(class, code, retryable, None)
}
fn permanent(code: &str) -> Failure {
    simple(CompletionFailureClass::Permanent, code, false)
}
fn configuration(code: &str) -> Failure {
    simple(CompletionFailureClass::Configuration, code, false)
}
fn fail(
    class: CompletionFailureClass,
    code: &str,
    retryable: bool,
    retry_after_seconds: Option<u64>,
) -> Failure {
    Box::new(CompletionFailure {
        class,
        code: code.into(),
        message: format!("model completion {code}"),
        retryable,
        retry_after_seconds,
    })
}
fn outcome(result: Result<CompletionResult, Failure>) -> CompletionOutcome {
    let (completion, failure) = match result {
        Ok(completion) => (Some(completion), None),
        Err(failure) => (None, Some(*failure)),
    };
    CompletionOutcome {
        completion,
        failure,
    }
}

pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}

#[cfg(test)]
mod tests;

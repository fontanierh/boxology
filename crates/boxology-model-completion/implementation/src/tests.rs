use super::*;
use boxology_contract::{CallContext, Caller, CancelToken, TraceContext};
use std::{
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    pin::pin,
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
};

fn server(status: u16, retry: Option<u64>, body: Vec<u8>, short: bool) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let join = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 8192];
        let size = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(request.contains("Bearer local-test-key"));
        let json: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(json["messages"][0]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            json["messages"][0]["tool_calls"].as_array().unwrap().len(),
            2
        );
        assert_eq!(json["messages"][1]["tool_call_id"], "c1");
        assert!(json["messages"][1].get("name").is_none());
        let retry = retry.map_or(String::new(), |v| format!("Retry-After: {v}\r\n"));
        let header = format!(
            "HTTP/1.1 {status} Test\r\n{retry}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len() + usize::from(short)
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    });
    (origin, join)
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("pending"),
    }
}
#[rustfmt::skip]
fn context() -> CallContext { CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None) }
#[rustfmt::skip]
fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            CompletionMessage { role: MessageRole::Assistant, content: None, tool_call_id: None, name: None, tool_calls: vec![
                ToolCall { id: "c1".into(), name: "read".into(), arguments_json: "{}".into() },
                ToolCall { id: "c2".into(), name: "write".into(), arguments_json: r#"{"path":"x"}"#.into() },
            ]},
            CompletionMessage { role: MessageRole::Tool, content: Some("file contents".into()), tool_call_id: Some("c1".into()), name: None, tool_calls: vec![] },
            CompletionMessage { role: MessageRole::Tool, content: Some("written".into()), tool_call_id: Some("c2".into()), name: Some("write".into()), tool_calls: vec![] },
        ],
        tools: vec![ToolDefinition { name: "read".into(), description: "Read".into(), input_schema_json: r#"{"type":"object"}"#.into() }],
        max_output_tokens: Some(64),
    }
}
fn response(message: Value, reason: &str) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"choices":[{"index":0,"message":message,"finish_reason":reason}],
        "usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}),
    )
    .unwrap()
}
fn invoke(status: u16, retry: Option<u64>, body: Vec<u8>, short: bool) -> CompletionOutcome {
    let (origin, join) = server(status, retry, body, short);
    let service = XaiCompletionService::test(origin);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let outcome = runtime
        .block_on(service.complete(context(), request()))
        .unwrap();
    join.join().unwrap();
    outcome
}

#[test]
fn generated_fake_carries_deterministic_tool_call_and_failure() {
    use boxology_generated_contract::test_support::ModelCompletionFake;
    let fake = ModelCompletionFake::new().with_complete(|_, request| async move {
        if request.messages.is_empty() {
            Ok(outcome(Err(input())))
        } else {
            Ok(outcome(Ok(CompletionResult {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "read".into(),
                    arguments_json: "{}".into(),
                }],
                finish_reason: FinishReason::ToolCalls,
                usage: TokenUsage {
                    input_tokens: 2,
                    output_tokens: 3,
                    total_tokens: 5,
                },
            })))
        }
    });
    let result = ready(fake.handle().complete(context(), request())).unwrap();
    assert_eq!(result.completion.unwrap().tool_calls[0].name, "read");
    let empty = CompletionRequest {
        messages: vec![],
        tools: vec![],
        max_output_tokens: None,
    };
    let failure = ready(fake.handle().complete(context(), empty)).unwrap();
    assert_eq!(failure.failure.unwrap().code, "input_invalid");
}

#[test]
fn transcript_correlation_rejects_orphan_mismatch_duplicate_and_incomplete() {
    let valid = request();
    let mut orphan = valid.clone();
    orphan.messages.remove(0);
    let mut mismatch = valid.clone();
    mismatch.messages[1].name = Some("write".into());
    let mut duplicate = valid.clone();
    let repeated = duplicate.messages[0].tool_calls[0].clone();
    duplicate.messages[0].tool_calls.push(repeated);
    let mut incomplete = valid;
    incomplete.messages.pop();
    for request in [orphan, mismatch, duplicate, incomplete] {
        assert_eq!(
            encode("grok-test", request).unwrap_err().code,
            "input_invalid"
        );
    }
}

#[test]
#[rustfmt::skip]
fn parallel_results_project_exactly_and_length_finishes() {
    let encoded = encode("grok-test", request()).unwrap();
    assert_eq!(encoded["messages"][1], json!({"role":"tool","content":"file contents","tool_call_id":"c1"}));
    assert_eq!(encoded["messages"][2], json!({"role":"tool","content":"written","tool_call_id":"c2","name":"write"}));
    let result = invoke(200, None, response(json!({"role":"assistant","content":"cut"}), "length"), false);
    assert!(matches!(result.completion.unwrap().finish_reason, FinishReason::Length));
}

#[test]
#[rustfmt::skip]
fn local_http_maps_tool_call_and_rate_limit_without_leaking_or_retrying() {
    let body = response(
        json!({"role":"assistant","content":null,"tool_calls":[
        {"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}}]}),
        "tool_calls",
    );
    let result = invoke(200, None, body, false);
    assert!(result.completion.is_some());
    let duplicate = response(json!({"role":"assistant","content":null,"tool_calls":[
        {"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}},
        {"id":"c1","type":"function","function":{"name":"write","arguments":"{}"}}]}), "tool_calls");
    assert_eq!(invoke(200, None, duplicate, false).failure.unwrap().code, "response_malformed");
    let limited = invoke(429, Some(7), b"provider secret".to_vec(), false);
    let limited = limited.failure.unwrap();
    assert_eq!(limited.code, "rate_limited");
    assert_eq!(limited.retry_after_seconds, Some(7));
    assert!(!limited.message.contains("provider secret"));
    let truncated = invoke(200, None, b"{}".to_vec(), true).failure.unwrap();
    assert_eq!(
        (truncated.code.as_str(), truncated.retryable),
        ("transport", true)
    );
}

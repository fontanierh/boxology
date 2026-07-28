use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc, time::Instant};

use boxology_contract::{
    CallContext, CapabilityDescriptor, CapabilityId, DecodeRole, Detail, ErasedCallError,
    ErasedCallTarget, OpaqueTree, SlotValue, TypeDescriptor, ValueRef,
};
use boxology_runtime::RemoteImportTarget;
use http::{HeaderValue, Method, Request, Uri, Version, header::CONTENT_TYPE, uri::Authority};

use crate::{
    conformance::conform_capability,
    encoder::{WireCallError, encode_request},
    semantic::decode_tree,
    syntax::{DEFAULT_DEPTH_LIMIT, SyntaxLimits, parse},
};

/// Configuration for an HTTP typed-call target.
#[derive(Clone)]
pub struct HttpClientConfig {
    origin: ClientOrigin,
    limits: ResponseLimits,
}

impl HttpClientConfig {
    /// Creates configuration for an origin-only HTTP base URL.
    pub fn new(origin: impl AsRef<str>) -> Result<Self, Detail> {
        Ok(Self {
            origin: ClientOrigin::parse(origin.as_ref())?,
            limits: ResponseLimits::default(),
        })
    }

    /// Replaces the inclusive response byte and syntax-depth limits.
    pub fn with_response_limits(
        mut self,
        max_response_bytes: usize,
        max_decode_depth: usize,
    ) -> Self {
        self.limits = ResponseLimits {
            max_bytes: max_response_bytes,
            max_depth: max_decode_depth,
        };
        self
    }
}

/// A reusable HTTP target accepted directly by generated typed handles.
#[derive(Clone)]
pub struct HttpClientTarget {
    origin: ClientOrigin,
    limits: ResponseLimits,
    capabilities: Arc<BTreeMap<CapabilityId, &'static CapabilityDescriptor>>,
    executor: ClientExecutor,
}

impl HttpClientTarget {
    /// Selects the exact contract capabilities callable through this target.
    pub fn new<I>(config: HttpClientConfig, capabilities: I) -> Result<Self, Detail>
    where
        I: IntoIterator<Item = &'static CapabilityDescriptor>,
    {
        let selected: Vec<_> = capabilities.into_iter().collect();
        let mut by_id = BTreeMap::new();
        for descriptor in &selected {
            if by_id.insert(descriptor.id().clone(), *descriptor).is_some() {
                return Err(Detail::new("http_duplicate_capability"));
            }
        }
        for descriptor in selected {
            conform_capability(descriptor)?;
        }
        Ok(Self {
            origin: config.origin,
            limits: config.limits,
            capabilities: Arc::new(by_id),
            executor: ClientExecutor::new()?,
        })
    }
}

impl ErasedCallTarget for HttpClientTarget {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        let Some(descriptor) = self.capabilities.get(capability).copied() else {
            return Box::pin(std::future::ready(Err(ErasedCallError::ContractViolation(
                Detail::new("http_client_capability"),
            ))));
        };
        let request =
            match prepare_request(&self.origin, descriptor, &context, &input, Instant::now()) {
                Ok(request) => request,
                Err(error) => return Box::pin(std::future::ready(Err(error))),
            };
        Box::pin(async move {
            self.executor
                .execute(
                    request,
                    &context,
                    descriptor.output(),
                    descriptor.error(),
                    self.limits,
                )
                .await
        })
    }
}

impl RemoteImportTarget for HttpClientTarget {
    fn supports_capability(&self, capability: &CapabilityId) -> bool {
        self.capabilities.contains_key(capability)
    }
}

#[derive(Clone)]
pub(crate) struct ClientOrigin(Authority);

impl ClientOrigin {
    pub(crate) fn parse(value: &str) -> Result<Self, Detail> {
        let invalid = || Detail::new("http_base_url");
        if value.contains(['@', '?', '#']) {
            return Err(invalid());
        }
        let uri: Uri = value.parse().map_err(|_| invalid())?;
        if !uri
            .scheme_str()
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http"))
            || !matches!(uri.path(), "" | "/")
        {
            return Err(invalid());
        }
        let authority = uri.authority().cloned().ok_or_else(invalid)?;
        let text = authority.as_str();
        let (port, valid_host) = if let Some(bracketed) = text.strip_prefix('[') {
            let (host, tail) = bracketed.split_once(']').ok_or_else(invalid)?;
            (
                tail.strip_prefix(':'),
                host.parse::<std::net::Ipv6Addr>().is_ok()
                    && (tail.is_empty() || tail.starts_with(':')),
            )
        } else {
            let (host, port) = text
                .rsplit_once(':')
                .map_or((text, None), |(host, port)| (host, Some(port)));
            (
                port,
                host.parse::<std::net::Ipv4Addr>().is_ok() || valid_dns(host),
            )
        };
        if !valid_host
            || port.is_some_and(|port| {
                port.is_empty()
                    || !port.bytes().all(|byte| byte.is_ascii_digit())
                    || port.parse::<u16>().is_err()
            })
            || text.contains(':') && authority.host().contains(':') && !text.starts_with('[')
        {
            return Err(invalid());
        }
        Ok(Self(authority))
    }
}

fn valid_dns(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.split('.').all(|label| {
            let bytes = label.as_bytes();
            (1..=63).contains(&bytes.len())
                && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        })
}

pub(crate) fn prepare_request(
    origin: &ClientOrigin,
    capability: &CapabilityDescriptor,
    context: &CallContext,
    input: &SlotValue,
    now: Instant,
) -> Result<Request<Vec<u8>>, ErasedCallError> {
    let uri: Uri = format!(
        "http://{}/rpc/{}/{}",
        origin.0,
        capability.id().box_id(),
        capability.name()
    )
    .parse()
    .map_err(request_error)?;

    let timeout = match context.deadline() {
        None => None,
        Some(deadline) => {
            let remaining = deadline.remaining_at(now);
            if remaining.is_zero() {
                return Err(ErasedCallError::Deadline);
            }
            let millis = remaining.as_nanos().div_ceil(1_000_000).min(9_999_999_999);
            Some(HeaderValue::from_str(&millis.to_string()).map_err(request_error)?)
        }
    };
    let idempotency = context
        .idempotency_key()
        .map(|key| {
            let value = key.as_str().as_bytes();
            if !(1..=256).contains(&value.len())
                || !value
                    .iter()
                    .all(|byte| matches!(byte, 0x21..=0x7e) && *byte != b',')
            {
                return Err(ErasedCallError::ContractViolation(Detail::new(
                    "http_idempotency_key",
                )));
            }
            HeaderValue::from_bytes(value).map_err(request_error)
        })
        .transpose()?;
    let traceparent = context
        .trace()
        .traceparent()
        .and_then(|value| HeaderValue::from_str(value).ok());
    let tracestate = traceparent.as_ref().and_then(|_| {
        context
            .trace()
            .tracestate()
            .and_then(|value| HeaderValue::from_str(value).ok())
    });
    let body = encode_request(input, capability.input()).map_err(request_error)?;
    let mut request = Request::new(body);
    *request.method_mut() = Method::POST;
    *request.version_mut() = Version::HTTP_11;
    *request.uri_mut() = uri;
    request
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (name, value) in [
        ("boxology-timeout-ms", timeout),
        ("traceparent", traceparent),
        ("tracestate", tracestate),
        ("idempotency-key", idempotency),
    ] {
        if let Some(value) = value {
            request.headers_mut().insert(name, value);
        }
    }
    Ok(request)
}

fn request_error(_: impl std::fmt::Debug) -> ErasedCallError {
    ErasedCallError::ContractViolation(Detail::new("http_request"))
}

#[derive(Clone)]
struct ClientExecutor {
    client: reqwest::Client,
}

impl ClientExecutor {
    fn new() -> Result<Self, Detail> {
        reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map(|client| Self { client })
            .map_err(|_| Detail::new("http_client"))
    }

    async fn execute(
        &self,
        request: Request<Vec<u8>>,
        context: &CallContext,
        output: &TypeDescriptor,
        domain_error: &TypeDescriptor,
        limits: ResponseLimits,
    ) -> Result<SlotValue, ErasedCallError> {
        race_operation(
            self.execute_operation(request, output, domain_error, limits),
            context,
        )
        .await
    }

    async fn execute_operation(
        &self,
        request: Request<Vec<u8>>,
        output: &TypeDescriptor,
        domain_error: &TypeDescriptor,
        limits: ResponseLimits,
    ) -> Result<SlotValue, ErasedCallError> {
        let (parts, body) = request.into_parts();
        let url = reqwest::Url::parse(&parts.uri.to_string()).map_err(request_error)?;
        let mut request = reqwest::Request::new(parts.method, url);
        *request.version_mut() = parts.version;
        *request.headers_mut() = parts.headers;
        *request.body_mut() = Some(body.into());

        let mut response = self
            .client
            .execute(request)
            .await
            .map_err(|_| ErasedCallError::Unavailable(Detail::new("http_transport")))?;
        let status = response.status().as_u16();
        let content_types: Vec<Vec<u8>> = response
            .headers()
            .get_all(CONTENT_TYPE)
            .iter()
            .map(|value| value.as_bytes().to_vec())
            .collect();
        let cap = u64::try_from(limits.max_bytes).unwrap_or(u64::MAX);
        if response.content_length().is_some_and(|length| length > cap) {
            return invalid();
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| invalid_error())? {
            if chunk.len() > limits.max_bytes - body.len() {
                return invalid();
            }
            body.extend_from_slice(&chunk);
        }
        let content_types: Vec<&[u8]> = content_types.iter().map(Vec::as_slice).collect();
        classify_response(status, &content_types, &body, output, domain_error, limits)
    }
}

async fn race_operation<F>(
    operation: F,
    context: &CallContext,
) -> Result<SlotValue, ErasedCallError>
where
    F: Future<Output = Result<SlotValue, ErasedCallError>>,
{
    let deadline = async {
        match context.deadline() {
            Some(deadline) => tokio::time::sleep_until(deadline.instant().into()).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(operation, deadline);
    tokio::select! {
        biased;
        result = &mut operation => result,
        () = &mut deadline => Err(ErasedCallError::Deadline),
        () = context.cancellation().cancelled() => Err(ErasedCallError::Cancelled),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResponseLimits {
    pub(crate) max_bytes: usize,
    pub(crate) max_depth: usize,
}

impl Default for ResponseLimits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_depth: DEFAULT_DEPTH_LIMIT,
        }
    }
}

pub(crate) fn classify_response(
    status: u16,
    content_types: &[&[u8]],
    body: &[u8],
    output: &TypeDescriptor,
    domain_error: &TypeDescriptor,
    limits: ResponseLimits,
) -> Result<SlotValue, ErasedCallError> {
    if content_types.len() != 1 || !content_types[0].eq_ignore_ascii_case(b"application/json") {
        return invalid();
    }
    let tree = parse(body, SyntaxLimits(limits.max_bytes, limits.max_depth))
        .map_err(|_| invalid_error())?;
    match status {
        200 => decode_result(tree, output),
        422 => decode_domain(tree, domain_error),
        _ => decode_call(status, tree),
    }
}

fn decode_result(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<SlotValue, ErasedCallError> {
    let value = one(one(tree, "result")?, "value")?;
    decode_tree(value, descriptor, DecodeRole::ConsumerOutput).map_err(|_| invalid_error())
}

fn decode_domain(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<SlotValue, ErasedCallError> {
    let error = one(tree, "error")?;
    let OpaqueTree::Object(mut fields) = error else {
        return invalid();
    };
    if fields.len() != 2 {
        return invalid();
    }
    let kind = take(&mut fields, "kind")?;
    if kind != OpaqueTree::String("domain".into()) {
        return invalid();
    }
    let value = take(&mut fields, "value")?;
    let decoded =
        decode_tree(value, descriptor, DecodeRole::ConsumerOutput).map_err(|_| invalid_error())?;
    let SlotValue::Value(value) = decoded else {
        return invalid();
    };
    let ValueRef::Enum { tag, payload } = value.view() else {
        return invalid();
    };
    Err(ErasedCallError::Domain {
        error_tag: tag.into(),
        payload: payload.clone(),
    })
}

fn decode_call(status: u16, tree: OpaqueTree) -> Result<SlotValue, ErasedCallError> {
    let error = one(tree, "error")?;
    let OpaqueTree::Object(mut fields) = error else {
        return invalid();
    };
    if fields.len() != 3 {
        return invalid();
    }
    if take(&mut fields, "kind")? != OpaqueTree::String("call".into()) {
        return invalid();
    }
    let OpaqueTree::String(code) = take(&mut fields, "code")? else {
        return invalid();
    };
    let OpaqueTree::String(message) = take(&mut fields, "message")? else {
        return invalid();
    };
    let Some(wire) = WireCallError::ALL
        .into_iter()
        .find(|wire| wire.spec().1 == code)
    else {
        return invalid();
    };
    let (expected_status, pinned_code, pinned_message) = wire.spec();
    if status != expected_status || message != pinned_message {
        return invalid();
    }
    let detail = || Detail::new(pinned_code).with_message(pinned_message);
    Err(match wire {
        WireCallError::InvalidRequest | WireCallError::PayloadTooLarge => {
            ErasedCallError::ContractViolation(detail())
        }
        WireCallError::UnknownBox
        | WireCallError::UnknownCapability
        | WireCallError::Unavailable => ErasedCallError::Unavailable(detail()),
        WireCallError::DeadlineExceeded => ErasedCallError::Deadline,
        WireCallError::MethodNotAllowed
        | WireCallError::UnsupportedMediaType
        | WireCallError::InvalidUpstreamResponse => ErasedCallError::InvalidResponse(detail()),
        WireCallError::Internal => ErasedCallError::Internal(detail()),
    })
}

fn one(tree: OpaqueTree, key: &str) -> Result<OpaqueTree, ErasedCallError> {
    let OpaqueTree::Object(mut fields) = tree else {
        return invalid();
    };
    if fields.len() != 1 {
        return invalid();
    }
    take(&mut fields, key)
}

fn take(fields: &mut Vec<(String, OpaqueTree)>, key: &str) -> Result<OpaqueTree, ErasedCallError> {
    let Some(index) = fields.iter().position(|(name, _)| name == key) else {
        return invalid();
    };
    Ok(fields.swap_remove(index).1)
}

fn invalid<T>() -> Result<T, ErasedCallError> {
    Err(invalid_error())
}
fn invalid_error() -> ErasedCallError {
    ErasedCallError::InvalidResponse(Detail::new("http_response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use boxology_contract::{
        BoxId, Caller, CancelToken, CapabilityId, CapabilityName, CapabilityShape, ContractValue,
        Deadline, ExposureLevel, FieldDescriptor, Idempotency, IdempotencyKey, TraceContext,
        VariantDescriptor, VariantPayload,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
        time::timeout,
    };

    fn capability() -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new("box-1").unwrap(),
                CapabilityName::new("cap_name").unwrap(),
            ),
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
    }

    fn context(
        deadline: Option<Deadline>,
        traceparent: Option<&str>,
        tracestate: Option<&str>,
        key: Option<&str>,
    ) -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            deadline,
            CancelToken::new(),
            TraceContext::new(
                traceparent.map(str::to_owned),
                tracestate.map(str::to_owned),
            ),
            key.map(|value| IdempotencyKey::new(value).unwrap()),
        )
    }
    fn request(context: &CallContext, now: Instant) -> Result<Request<Vec<u8>>, ErasedCallError> {
        prepare_request(
            &ClientOrigin::parse("http://example.com").unwrap(),
            &capability(),
            context,
            &SlotValue::Value(ContractValue::string("hello")),
            now,
        )
    }

    fn limits(body: &[u8]) -> ResponseLimits {
        ResponseLimits {
            max_bytes: body.len(),
            max_depth: 128,
        }
    }

    #[test]
    fn origins_accept_and_normalize_only_http_origins() {
        for accepted in [
            "http://example.com",
            "http://example.com/",
            "http://localhost",
            "HTTP://example.com:0",
            "http://127.0.0.1:65535",
            "http://[::1]",
            "http://[2001:db8::1]:8080/",
        ] {
            assert!(ClientOrigin::parse(accepted).is_ok(), "{accepted}");
        }
        for rejected in [
            "https://example.com",
            "ftp://example.com",
            "example.com",
            "//example.com",
            "http://",
            "http://user@example.com",
            "http://user:pass@example.com",
            "http://example.com/prefix",
            "http://example.com/api/",
            "http://example.com?",
            "http://example.com#",
            "http://example.com:65536",
            "http://example.com:",
            "http://example.com:+80",
            "http://example.com:-80",
            "http://2001:db8::1",
            "http://[not-ip]",
            "http://[gggg::1]",
            "http://[::1]evil",
            "http://exa!mple.com",
            "http://foo_bar",
            "http://-",
            "http://-example.com",
            "http://example-.com",
            "http://example..com",
        ] {
            let error = ClientOrigin::parse(rejected)
                .err()
                .unwrap_or_else(|| panic!("accepted {rejected}"));
            assert_eq!(error, Detail::new("http_base_url"), "{rejected}");
            assert!(!format!("{error:?}").contains(rejected));
        }
        assert!(ClientOrigin::parse(&format!("http://{}.com", "a".repeat(63))).is_ok());
        assert!(ClientOrigin::parse(&format!("http://{}.com", "a".repeat(64))).is_err());
    }

    #[test]
    fn request_line_headers_and_body_are_exact() {
        let now = Instant::now();
        let request = prepare_request(
            &ClientOrigin::parse("http://example.com:8042/").unwrap(),
            &capability(),
            &context(None, None, Some("orphan=state"), None),
            &SlotValue::Value(ContractValue::string("hello")),
            now,
        )
        .unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.version(), Version::HTTP_11);
        assert_eq!(request.uri(), "http://example.com:8042/rpc/box-1/cap_name");
        assert_eq!(request.headers().len(), 1);
        assert_eq!(request.headers()[CONTENT_TYPE], "application/json");
        assert_eq!(request.body(), br#""hello""#);
        assert_eq!(request.uri().path(), "/rpc/box-1/cap_name");
        assert_eq!(request.uri().query(), None);
    }

    #[test]
    fn deadlines_are_ceiled_nonzero_and_clamped() {
        let now = Instant::now();
        assert!(
            request(&context(None, None, None, None), now)
                .unwrap()
                .headers()
                .get("boxology-timeout-ms")
                .is_none()
        );
        assert!(matches!(
            request(&context(Some(Deadline::at(now)), None, None, None), now),
            Err(ErasedCallError::Deadline)
        ));
        for (nanos, expected) in [
            (1, "1"),
            (999_999, "1"),
            (1_000_000, "1"),
            (1_000_001, "2"),
            (9_999_999_999_000_000, "9999999999"),
            (9_999_999_999_000_001, "9999999999"),
            (31_536_000_000_000_000, "9999999999"),
        ] {
            let ctx = context(
                Some(Deadline::at(now + Duration::from_nanos(nanos))),
                None,
                None,
                None,
            );
            let value = request(&ctx, now).unwrap().headers()["boxology-timeout-ms"]
                .to_str()
                .unwrap()
                .to_owned();
            assert_eq!(value, expected);
            assert_ne!(value, "0");
            assert!(!value.starts_with('0'));
        }
    }

    #[test]
    fn tracing_is_best_effort_and_parent_gates_state() {
        let now = Instant::now();
        for (parent, state, expected_parent, expected_state) in [
            (None, None, None, None),
            (None, Some("state=x"), None, None),
            (Some("opaque-parent"), None, Some("opaque-parent"), None),
            (
                Some("opaque-parent"),
                Some("state=x"),
                Some("opaque-parent"),
                Some("state=x"),
            ),
            (Some("bad\nparent"), Some("state=x"), None, None),
            (
                Some("opaque-parent"),
                Some("bad\nstate"),
                Some("opaque-parent"),
                None,
            ),
        ] {
            let request = request(&context(None, parent, state, None), now).unwrap();
            assert_eq!(
                request
                    .headers()
                    .get("traceparent")
                    .and_then(|v| v.to_str().ok()),
                expected_parent
            );
            assert_eq!(
                request
                    .headers()
                    .get("tracestate")
                    .and_then(|v| v.to_str().ok()),
                expected_state
            );
        }
    }

    #[test]
    fn idempotency_and_encoding_failures_are_redacted_contract_errors() {
        let now = Instant::now();
        assert!(IdempotencyKey::new("").is_err());
        for valid in ["!", &"x".repeat(256)] {
            let request = request(&context(None, None, None, Some(valid)), now).unwrap();
            assert_eq!(request.headers()["idempotency-key"], valid);
        }
        for invalid in [
            "x".repeat(257),
            "has space".into(),
            "control\n".into(),
            "has,comma".into(),
            "nonascii-é".into(),
        ] {
            let error = request(&context(None, None, None, Some(&invalid)), now).unwrap_err();
            assert!(
                matches!(error, ErasedCallError::ContractViolation(ref detail) if detail.code() == "http_idempotency_key")
            );
            assert!(!format!("{error:?}").contains(&invalid));
        }
        let error = prepare_request(
            &ClientOrigin::parse("http://example.com").unwrap(),
            &capability(),
            &context(None, None, None, None),
            &SlotValue::Missing,
            now,
        )
        .unwrap_err();
        assert!(
            matches!(error, ErasedCallError::ContractViolation(ref detail) if detail == &Detail::new("http_request"))
        );
        assert!(!format!("{error:?}").contains("missing"));
    }
    fn classify(status: u16, body: &[u8]) -> Result<SlotValue, ErasedCallError> {
        classify_response(
            status,
            &[b"application/json"],
            body,
            &output(),
            &domain(),
            limits(body),
        )
    }
    fn output() -> TypeDescriptor {
        TypeDescriptor::structure(vec![FieldDescriptor::new(
            "known",
            TypeDescriptor::string(),
            None,
        )])
        .unwrap()
    }
    fn domain() -> TypeDescriptor {
        TypeDescriptor::enumeration(vec![VariantDescriptor::new(
            "known",
            VariantPayload::Unit,
            None,
        )])
        .unwrap()
    }
    fn category(error: ErasedCallError) -> &'static str {
        match error {
            ErasedCallError::Domain { .. } => "domain",
            ErasedCallError::Deadline => "deadline",
            ErasedCallError::Unavailable(_) => "unavailable",
            ErasedCallError::ContractViolation(_) => "contract",
            ErasedCallError::InvalidResponse(_) => "invalid",
            ErasedCallError::Internal(_) => "internal",
            _ => "other",
        }
    }
    fn call(code: &str, message: &str) -> Vec<u8> {
        format!(r#"{{"error":{{"message":"{message}","code":"{code}","kind":"call"}}}}"#)
            .into_bytes()
    }
    fn assert_invalid(status: u16, body: &[u8]) {
        assert!(matches!(
            classify(status, body),
            Err(ErasedCallError::InvalidResponse(_))
        ));
    }

    async fn raw_server(
        fragments: Vec<Vec<u8>>,
        watch_second: bool,
    ) -> (String, JoinHandle<(Vec<u8>, bool)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .expect("client did not connect")
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut block = [0; 1024];
                let read = timeout(Duration::from_secs(5), stream.read(&mut block))
                    .await
                    .expect("client request stalled")
                    .unwrap();
                assert_ne!(read, 0, "client closed before completing request");
                request.extend_from_slice(&block[..read]);
                let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
                    continue;
                };
                let head = std::str::from_utf8(&request[..end]).unwrap();
                let length = head.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                });
                if request.len() >= end + 4 + length.unwrap_or(0) {
                    break;
                }
            }
            for fragment in fragments {
                stream.write_all(&fragment).await.unwrap();
                tokio::task::yield_now().await;
            }
            stream.shutdown().await.unwrap();
            let second = watch_second
                && timeout(Duration::from_millis(200), listener.accept())
                    .await
                    .is_ok();
            (request, second)
        });
        (format!("http://{address}"), task)
    }

    async fn bounded_execute(
        executor: &ClientExecutor,
        request: Request<Vec<u8>>,
        context: &CallContext,
        limits: ResponseLimits,
    ) -> Result<SlotValue, ErasedCallError> {
        timeout(
            Duration::from_secs(5),
            executor.execute(request, context, &output(), &domain(), limits),
        )
        .await
        .expect("client operation stalled")
    }

    async fn direct(
        base: &str,
        path: &str,
        body: &[u8],
        max_bytes: usize,
    ) -> Result<SlotValue, ErasedCallError> {
        let request = Request::builder()
            .method(Method::POST)
            .version(Version::HTTP_11)
            .uri(format!("{base}{path}"))
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .unwrap();
        let context = context(None, None, None, None);
        bounded_execute(
            &ClientExecutor::new().unwrap(),
            request,
            &context,
            ResponseLimits {
                max_bytes,
                max_depth: 128,
            },
        )
        .await
    }

    #[test]
    fn default_client_response_limits_are_exact() {
        let config = HttpClientConfig::new("http://example.com").unwrap();
        assert_eq!(config.limits.max_bytes, 8 * 1024 * 1024);
        assert_eq!(config.limits.max_depth, 128);
    }

    fn race_context(deadline: Option<Instant>, cancellation: CancelToken) -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            deadline.map(Deadline::at),
            cancellation,
            TraceContext::empty(),
            None,
        )
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn http_response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {status}\r\n{headers}\r\n").into_bytes();
        response.extend_from_slice(body);
        response
    }

    #[tokio::test(start_paused = true)]
    async fn race_preserves_results_and_polls_operation_first() {
        let cancellation = CancelToken::new();
        cancellation.cancel();
        let context = race_context(Some(tokio::time::Instant::now().into_std()), cancellation);
        let value = SlotValue::Value(ContractValue::string("done"));
        assert_eq!(
            race_operation(std::future::ready(Ok(value.clone())), &context).await,
            Ok(value)
        );
        let expected = ErasedCallError::Unavailable(Detail::new("http_transport"));
        assert_eq!(
            race_operation(std::future::ready(Err(expected.clone())), &context).await,
            Err(expected)
        );

        let context = race_context(
            Some(tokio::time::Instant::now().into_std()),
            CancelToken::new(),
        );
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = DropFlag(dropped.clone());
        let pending = async move {
            let _guard = guard;
            std::future::pending().await
        };
        assert_eq!(
            race_operation(pending, &context).await,
            Err(ErasedCallError::Deadline)
        );
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn race_deadline_precedes_cancellation_and_newly_ready_operation_wins() {
        let cancellation = CancelToken::new();
        cancellation.cancel();
        let context = race_context(Some(tokio::time::Instant::now().into_std()), cancellation);
        assert_eq!(
            race_operation(std::future::pending(), &context).await,
            Err(ErasedCallError::Deadline)
        );

        let (first_poll_sent, first_poll_seen) = tokio::sync::oneshot::channel();
        let deadline = tokio::time::Instant::now().into_std() + Duration::from_secs(1);
        let task = tokio::spawn(async move {
            let context = race_context(Some(deadline), CancelToken::new());
            let mut first = Some(first_poll_sent);
            let operation = std::future::poll_fn(move |_| {
                if let Some(sent) = first.take() {
                    sent.send(()).unwrap();
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(Ok(SlotValue::Value(ContractValue::string("same-turn"))))
                }
            });
            race_operation(operation, &context).await
        });
        timeout(Duration::from_secs(1), first_poll_seen)
            .await
            .expect("operation was not first-polled")
            .unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(matches!(task.await.unwrap(), Ok(SlotValue::Value(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn race_observes_later_deadline_and_cancellation_and_drops_operations() {
        let deadline = tokio::time::Instant::now().into_std() + Duration::from_secs(5);
        let deadline_task = tokio::spawn(async move {
            let context = race_context(Some(deadline), CancelToken::new());
            race_operation(std::future::pending(), &context).await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(deadline_task.await.unwrap(), Err(ErasedCallError::Deadline));

        let cancellation = CancelToken::new();
        let trigger = cancellation.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let observed = dropped.clone();
        let cancel_task = tokio::spawn(async move {
            let context = race_context(None, cancellation);
            let guard = DropFlag(observed);
            race_operation(
                async move {
                    let _guard = guard;
                    std::future::pending().await
                },
                &context,
            )
            .await
        });
        tokio::task::yield_now().await;
        trigger.cancel();
        assert_eq!(cancel_task.await.unwrap(), Err(ErasedCallError::Cancelled));
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stalled_real_response_is_cancelled_promptly_without_diagnostics() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (head_sent, head_seen) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut block = [0; 1024];
                let read = stream.read(&mut block).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&block[..read]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            head_sent.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let cancellation = CancelToken::new();
        let context = race_context(None, cancellation.clone());
        let request = Request::builder()
            .uri(format!("http://{address}/SENTINEL-PATH"))
            .body(b"SENTINEL-BODY".to_vec())
            .unwrap();
        let executor = ClientExecutor::new().unwrap();
        let output = output();
        let domain = domain();
        let operation = executor.execute(
            request,
            &context,
            &output,
            &domain,
            ResponseLimits::default(),
        );
        tokio::pin!(operation);
        timeout(Duration::from_secs(1), async {
            tokio::select! {
                result = &mut operation => panic!("completed before stall: {result:?}"),
                result = head_seen => result.unwrap(),
            }
        })
        .await
        .expect("server did not flush the response head");
        tokio::task::yield_now().await;
        std::future::poll_fn(|cx| match operation.as_mut().poll(cx) {
            std::task::Poll::Ready(result) => panic!("body did not stall: {result:?}"),
            std::task::Poll::Pending => std::task::Poll::Ready(()),
        })
        .await;
        cancellation.cancel();
        let error = timeout(Duration::from_secs(1), operation)
            .await
            .expect("cancellation did not return promptly")
            .unwrap_err();
        assert_eq!(error, ErasedCallError::Cancelled);
        assert!(!format!("{error:?}").contains("SENTINEL"));
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[test]
    fn result_and_domain_payloads_are_consumer_tolerant() {
        let result = classify(
            200,
            br#"{"result":{"value":{"known":"yes","future":"ignored"}}}"#,
        )
        .unwrap();
        assert!(matches!(result, SlotValue::Value(_)));
        assert!(matches!(
            classify(422, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null}}}"#).unwrap_err(),
            ErasedCallError::Domain { error_tag, payload: SlotValue::Null } if error_tag == "known"
        ));
        let error = classify(422, br#"{"error":{"value":{"tag":"future","payload":{"sentinel":"secret"}},"kind":"domain"}}"#).unwrap_err();
        assert!(
            matches!(error, ErasedCallError::Domain { error_tag, .. } if error_tag == "future")
        );
    }

    #[test]
    fn canonical_call_table_maps_every_typed_category() {
        #[rustfmt::skip]
        let expected = ["unavailable", "unavailable", "contract", "invalid", "contract", "invalid", "deadline", "unavailable", "invalid", "internal"];
        for (wire, category_name) in WireCallError::ALL.into_iter().zip(expected) {
            let (status, code, message) = wire.spec();
            assert_eq!(
                category(classify(status, &call(code, message)).unwrap_err()),
                category_name,
                "{code}"
            );
        }
    }

    #[test]
    fn envelopes_are_strict_and_status_bound() {
        for wire in WireCallError::ALL {
            let (status, code, message) = wire.spec();
            assert_invalid(if status == 400 { 401 } else { 400 }, &call(code, message));
        }
        for (status, body) in [
            (500, br#"{"error":{"kind":"call","code":"mystery","message":"sentinel"}}"#.as_slice()),
            (500, br#"{"error":{"kind":"call","code":"internal","message":"wrong"}}"#),
            (500, br#"{"error":{"kind":"domain","code":"internal","message":"internal error"}}"#),
            (500, br#"{"error":{"kind":"call","code":"internal","message":"internal error","extra":0}}"#),
            (500, br#"{"error":{"kind":"call","code":"internal","code":"internal","message":"internal error"}}"#),
            (422, br#"{"result":{"value":{"known":"x"}}}"#),
            (200, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null}}}"#),
            (200, br#"{"result":{"value":{"known":"x"},"extra":0}}"#),
            (200, br#"{"result":{"value":{"known":"x"},"value":{"known":"x"}}}"#),
            (422, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null},"extra":0}}"#),
        ] {
            assert_invalid(status, body);
        }
        let error = classify(
            500,
            br#"{"error":{"kind":"call","code":"mystery","message":"sentinel"}}"#,
        )
        .unwrap_err();
        assert!(!format!("{error:?}").contains("sentinel"));
    }

    #[test]
    fn rejects_bad_headers_bodies_and_unrecognized_statuses_without_echoing() {
        let body = br#"{"sentinel":"DO_NOT_ECHO"}"#;
        for headers in [
            &[][..],
            &[b"text/json".as_slice()][..],
            &[
                b"application/json".as_slice(),
                b"application/json".as_slice(),
            ][..],
            &[b"application/json; charset=utf-8".as_slice()][..],
        ] {
            let error = classify_response(200, headers, body, &output(), &domain(), limits(body))
                .unwrap_err();
            assert!(!format!("{error:?}").contains("DO_NOT_ECHO"));
        }
        assert!(
            classify_response(
                200,
                &[b"APPLICATION/JSON"],
                br#"{"result":{"value":{"known":"x"}}}"#,
                &output(),
                &domain(),
                ResponseLimits::default()
            )
            .is_ok()
        );
        for (status, body) in [
            (100, b"{}".as_slice()),
            (204, b""),
            (302, b"{"),
            (201, b"[]"),
            (599, b"null"),
        ] {
            assert_invalid(status, body);
        }
    }

    #[test]
    fn byte_and_depth_limits_are_inclusive() {
        let body = br#"{"result":{"value":{"known":"x"}}}"#;
        assert!(
            classify_response(
                200,
                &[b"application/json"],
                body,
                &output(),
                &domain(),
                limits(body)
            )
            .is_ok()
        );
        let too_small = ResponseLimits {
            max_bytes: body.len() - 1,
            max_depth: 128,
        };
        assert_eq!(
            category(
                classify_response(
                    200,
                    &[b"application/json"],
                    body,
                    &output(),
                    &domain(),
                    too_small
                )
                .unwrap_err()
            ),
            "invalid"
        );
        let exact = ResponseLimits {
            max_bytes: body.len(),
            max_depth: 3,
        };
        assert!(
            classify_response(
                200,
                &[b"application/json"],
                body,
                &output(),
                &domain(),
                exact
            )
            .is_ok()
        );
        let shallow = ResponseLimits {
            max_bytes: body.len(),
            max_depth: 2,
        };
        assert_eq!(
            category(
                classify_response(
                    200,
                    &[b"application/json"],
                    body,
                    &output(),
                    &domain(),
                    shallow
                )
                .unwrap_err()
            ),
            "invalid"
        );
    }

    #[tokio::test]
    async fn executor_sends_prepared_http1_request_once_and_accepts_fragmented_exact_cap() {
        let body = br#"{"result":{"value":{"known":"x"}}}"#;
        let (base, server) = raw_server(
            vec![
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"result\":".to_vec(),
                b"{\"value\":{\"known\":\"x\"}}}".to_vec(),
            ],
            true,
        )
        .await;
        let now = Instant::now();
        let context = context(
            Some(Deadline::at(now + Duration::from_millis(1234))),
            Some("trace-parent"),
            Some("trace-state"),
            Some("idempotency-key"),
        );
        let request = prepare_request(
            &ClientOrigin::parse(&base).unwrap(),
            &capability(),
            &context,
            &SlotValue::Value(ContractValue::string("hello")),
            now,
        )
        .unwrap();
        let result = bounded_execute(
            &ClientExecutor::new().unwrap(),
            request,
            &context,
            limits(body),
        )
        .await
        .unwrap();
        assert!(matches!(result, SlotValue::Value(_)));
        let (request, second) = server.await.unwrap();
        let request = String::from_utf8(request).unwrap();
        let (head, payload) = request.split_once("\r\n\r\n").unwrap();
        let mut lines = head.lines();
        assert_eq!(lines.next(), Some("POST /rpc/box-1/cap_name HTTP/1.1"));
        assert_eq!(payload, "\"hello\"");
        let mut headers: Vec<_> = lines
            .map(|line| {
                let (name, value) = line.split_once(':').unwrap();
                format!("{}:{}", name.to_ascii_lowercase(), value.trim())
            })
            .collect();
        headers.sort();
        let mut expected = vec![
            "accept:*/*".to_owned(),
            "boxology-timeout-ms:1234".to_owned(),
            "content-length:7".to_owned(),
            "content-type:application/json".to_owned(),
            format!("host:{}", base.strip_prefix("http://").unwrap()),
            "idempotency-key:idempotency-key".to_owned(),
            "traceparent:trace-parent".to_owned(),
            "tracestate:trace-state".to_owned(),
        ];
        expected.sort();
        assert_eq!(headers, expected);
        assert!(!second);
    }

    #[tokio::test]
    async fn executor_never_follows_redirects_or_retries_transport_failures() {
        let (base, server) = raw_server(
            vec![http_response(
                "302 Found",
                "Location: /sentinel-redirect\r\nContent-Length: 0\r\nConnection: close\r\n",
                b"",
            )],
            true,
        )
        .await;
        assert!(matches!(
            direct(&base, "/rpc", b"", 34).await,
            Err(ErasedCallError::InvalidResponse(ref detail)) if detail.code() == "http_response"
        ));
        assert!(!server.await.unwrap().1);

        let (base, server) = raw_server(Vec::new(), true).await;
        let error = direct(&base, "/SENTINEL-URL", b"SENTINEL-BODY", 34)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ErasedCallError::Unavailable(ref detail) if detail.code() == "http_transport"
        ));
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("SENTINEL"));
        assert!(!server.await.unwrap().1);
    }

    #[tokio::test]
    async fn executor_enforces_declared_and_streamed_caps_for_every_status() {
        let success = br#"{"result":{"value":{"known":"x"}}}"#;
        for status in ["200 OK", "500 Internal Server Error"] {
            let (base, server) = raw_server(
                vec![
                    http_response(
                        status,
                        "Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n",
                        b"20\r\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n",
                    ),
                    b"3\r\nbbb\r\n0\r\n\r\n".to_vec(),
                ],
                false,
            )
            .await;
            assert!(matches!(
                direct(&base, "/rpc", b"", 34).await,
                Err(ErasedCallError::InvalidResponse(ref detail)) if detail.code() == "http_response"
            ));
            server.await.unwrap();
        }
        let (base, server) = raw_server(
            vec![http_response(
                "200 OK",
                "Content-Type: application/json\r\nContent-Length: 35\r\n",
                b"",
            )],
            false,
        )
        .await;
        assert!(matches!(
            direct(&base, "/rpc", b"", 34).await,
            Err(ErasedCallError::InvalidResponse(_))
        ));
        server.await.unwrap();

        let (base, server) = raw_server(
            vec![http_response(
                "200 OK",
                "Content-Type: application/json\r\nContent-Length: 34\r\n",
                success,
            )],
            false,
        )
        .await;
        assert!(direct(&base, "/rpc", b"", 34).await.is_ok());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn executor_maps_truncation_wire_errors_and_raw_content_type_lines() {
        let unavailable = call("unavailable", "service unavailable");
        let cases = [
            (
                "503 Service Unavailable",
                "Content-Type: application/json\r\nContent-Length: 78\r\n",
                unavailable.as_slice(),
                "unavailable",
            ),
            (
                "200 OK",
                "Content-Type: application/json\r\nContent-Type: application/json\r\nContent-Length: 34\r\n",
                br#"{"result":{"value":{"known":"x"}}}"#,
                "invalid",
            ),
            (
                "200 OK",
                "Content-Length: 34\r\n",
                br#"{"result":{"value":{"known":"x"}}}"#,
                "invalid",
            ),
        ];
        for (status, headers, body, expected) in cases {
            let (base, server) =
                raw_server(vec![http_response(status, headers, body)], false).await;
            assert_eq!(
                category(direct(&base, "/rpc", b"", body.len()).await.unwrap_err()),
                expected
            );
            server.await.unwrap();
        }

        let (base, server) = raw_server(
            vec![http_response(
                "200 OK",
                "Content-Type: application/json\r\nContent-Length: 14\r\n",
                b"SENTINEL-BODY",
            )],
            false,
        )
        .await;
        let error = direct(&base, "/SENTINEL-URL", b"", 14).await.unwrap_err();
        assert!(matches!(
            error,
            ErasedCallError::InvalidResponse(ref detail) if detail.code() == "http_response"
        ));
        assert!(!format!("{error:?}").contains("SENTINEL"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn executor_is_reusable_and_redacts_request_conversion_failures() {
        let executor = ClientExecutor::new().unwrap();
        let relative = Request::builder()
            .uri("/SENTINEL-PATH")
            .body(b"SENTINEL-BODY".to_vec())
            .unwrap();
        let context = context(None, None, None, None);
        let error = bounded_execute(&executor, relative, &context, ResponseLimits::default())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ErasedCallError::ContractViolation(ref detail) if detail.code() == "http_request"
        ));
        assert!(!format!("{error:?}").contains("SENTINEL"));

        for _ in 0..2 {
            let body = br#"{"result":{"value":{"known":"x"}}}"#;
            let (base, server) = raw_server(
                vec![http_response(
                    "200 OK",
                    "Content-Type: application/json\r\nContent-Length: 34\r\n",
                    body,
                )],
                false,
            )
            .await;
            let request = Request::builder()
                .uri(format!("{base}/rpc"))
                .body(Vec::new())
                .unwrap();
            assert!(
                bounded_execute(&executor, request, &context, limits(body))
                    .await
                    .is_ok()
            );
            server.await.unwrap();
        }
    }
}

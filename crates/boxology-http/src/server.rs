use crate::{
    encoder::{WireCallError, encode_domain, encode_result},
    semantic::decode_tree,
    syntax::{SyntaxError, SyntaxLimits, parse},
};
use boxology_contract::{
    BoxId, CallContext, Caller, CancelToken, CapabilityDescriptor, CapabilityName, Deadline,
    DecodeRole, ErasedCallError, ExposureLevel, IdempotencyKey, SlotValue, TraceContext,
    TypeDescriptor,
};
use boxology_runtime::{TransportExposure, TransportTaskTracker};
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use http_body::Body;
use http_body_util::Full;
use mediatype::{MediaTypeList, names};
use std::{
    collections::BTreeMap,
    future::{Future, poll_fn},
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const TIMEOUT_HEADER: &str = "boxology-timeout-ms";
const IDEMPOTENCY_HEADER: &str = "idempotency-key";
const TRACEPARENT_HEADER: &str = "traceparent";
const TRACESTATE_HEADER: &str = "tracestate";
const MAX_TRACESTATE_BYTES: usize = 512;
const BODY_FRAME_QUANTUM: usize = 32;

fn prepare_call_context(
    head_received: Instant,
    timeout: Option<Duration>,
    default_timeout: Duration,
    trace: TraceContext,
    idempotency_key: Option<IdempotencyKey>,
) -> Result<CallContext, WireCallError> {
    let deadline = head_received
        .checked_add(timeout.unwrap_or(default_timeout))
        .map(Deadline::at)
        .ok_or(WireCallError::Internal)?;
    Ok(CallContext::new(
        Caller::Anonymous,
        Some(deadline),
        CancelToken::new(),
        trace,
        idempotency_key,
    ))
}

#[derive(Clone)]
pub(crate) struct DispatchTasks(Arc<DispatchTasksInner>);

struct DispatchTasksInner {
    tracker: TransportTaskTracker,
    next_id: AtomicU64,
    tasks: Mutex<BTreeMap<u64, TaskControl>>,
    empty: tokio::sync::Notify,
}

struct TaskControl {
    cancellation: CancelToken,
    abort: tokio::task::AbortHandle,
}

struct RemoveTask {
    owner: Weak<DispatchTasksInner>,
    id: u64,
}

impl Drop for RemoveTask {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.tasks.lock().unwrap().remove(&self.id);
            owner.empty.notify_waiters();
        }
    }
}

impl DispatchTasks {
    pub(crate) fn new(tracker: TransportTaskTracker) -> Self {
        Self(Arc::new(DispatchTasksInner {
            tracker,
            next_id: AtomicU64::new(0),
            tasks: Mutex::new(BTreeMap::new()),
            empty: tokio::sync::Notify::new(),
        }))
    }

    fn spawn<F, T>(&self, cancellation: CancelToken, future: F) -> tokio::task::JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let id = self.0.next_id.fetch_add(1, Ordering::Relaxed);
        let (start, started) = tokio::sync::oneshot::channel();
        let cleanup = RemoveTask {
            owner: Arc::downgrade(&self.0),
            id,
        };
        let task = self.0.tracker.spawn(async move {
            let _cleanup = cleanup;
            let _ = started.await;
            future.await
        });
        self.0.tasks.lock().unwrap().insert(
            id,
            TaskControl {
                cancellation,
                abort: task.abort_handle(),
            },
        );
        let _ = start.send(());
        task
    }

    pub(crate) fn cancel_all(&self) {
        for task in self.0.tasks.lock().unwrap().values() {
            task.cancellation.cancel();
        }
    }

    pub(crate) fn abort_all(&self) {
        for task in self.0.tasks.lock().unwrap().values() {
            task.abort.abort();
        }
    }

    pub(crate) async fn wait_empty(&self) {
        loop {
            let empty = self.0.empty.notified();
            tokio::pin!(empty);
            empty.as_mut().enable();
            if self.0.tasks.lock().unwrap().is_empty() {
                return;
            }
            empty.await;
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.tasks.lock().unwrap().len()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct EncodedResponse {
    status: u16,
    body: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq)]
enum DispatchOutcome {
    Response(EncodedResponse),
    Abandoned,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RequestAbandoned;

struct CancelOnDrop(Option<CancelToken>);

impl CancelOnDrop {
    fn new(cancellation: CancelToken) -> Self {
        Self(Some(cancellation))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancellation) = self.0.take() {
            cancellation.cancel();
        }
    }
}

async fn dispatch_request(
    tasks: &DispatchTasks,
    exposure: TransportExposure,
    context: CallContext,
    input: SlotValue,
) -> DispatchOutcome {
    let descriptor = exposure.descriptor();
    dispatch_request_with(
        tasks,
        descriptor,
        context,
        input,
        move |context, input| async move { exposure.dispatch(context, input).await },
    )
    .await
}

async fn dispatch_request_with<F, Fut>(
    tasks: &DispatchTasks,
    descriptor: &'static CapabilityDescriptor,
    context: CallContext,
    input: SlotValue,
    dispatch: F,
) -> DispatchOutcome
where
    F: FnOnce(CallContext, SlotValue) -> Fut + Send + 'static,
    Fut: Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'static,
{
    let Some(deadline) = context.deadline() else {
        return response_error(WireCallError::Internal);
    };
    let cancellation = context.cancellation().clone();
    if deadline.instant() <= tokio::time::Instant::now().into_std() {
        cancellation.cancel();
        return response_error(WireCallError::DeadlineExceeded);
    }
    let task_cancellation = cancellation.clone();
    let task = tasks.spawn(
        cancellation.clone(),
        invoke_if_live(deadline, task_cancellation, context, input, dispatch),
    );
    await_dispatch(task, cancellation, deadline, descriptor).await
}

async fn invoke_if_live<F, Fut>(
    deadline: Deadline,
    cancellation: CancelToken,
    context: CallContext,
    input: SlotValue,
    dispatch: F,
) -> Result<SlotValue, ErasedCallError>
where
    F: FnOnce(CallContext, SlotValue) -> Fut,
    Fut: Future<Output = Result<SlotValue, ErasedCallError>>,
{
    if deadline.instant() <= tokio::time::Instant::now().into_std() {
        cancellation.cancel();
        return Err(ErasedCallError::Deadline);
    }
    dispatch(context, input).await
}

async fn await_dispatch(
    mut task: tokio::task::JoinHandle<Result<SlotValue, ErasedCallError>>,
    cancellation: CancelToken,
    deadline: Deadline,
    descriptor: &CapabilityDescriptor,
) -> DispatchOutcome {
    let timeout = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline.instant()));
    tokio::pin!(timeout);
    tokio::select! {
        biased;
        joined = &mut task => match joined {
            Ok(result) => encode_dispatch_result(result, descriptor),
            Err(_) => response_error(WireCallError::Internal),
        },
        () = &mut timeout => {
            cancellation.cancel();
            response_error(WireCallError::DeadlineExceeded)
        },
        () = cancellation.cancelled() => DispatchOutcome::Abandoned,
    }
}

fn encode_dispatch_result(
    result: Result<SlotValue, ErasedCallError>,
    descriptor: &CapabilityDescriptor,
) -> DispatchOutcome {
    match result {
        Ok(value) => match encode_result(&value, descriptor.output()) {
            Ok(body) => DispatchOutcome::Response(EncodedResponse { status: 200, body }),
            Err(_) => response_error(WireCallError::InvalidUpstreamResponse),
        },
        Err(ErasedCallError::Domain { error_tag, payload }) => {
            match encode_domain(&error_tag, &payload, descriptor.error()) {
                Ok(body) => DispatchOutcome::Response(EncodedResponse { status: 422, body }),
                Err(_) => response_error(WireCallError::InvalidUpstreamResponse),
            }
        }
        Err(error) => {
            response_error(WireCallError::from_erased(&error).unwrap_or(WireCallError::Internal))
        }
    }
}

fn response_error(error: WireCallError) -> DispatchOutcome {
    let encoded = error.encode();
    DispatchOutcome::Response(EncodedResponse {
        status: encoded.status(),
        body: encoded.body().to_vec(),
    })
}

pub(crate) async fn handle_request<B>(
    request: Request<B>,
    head_received: Instant,
    exposures: &[TransportExposure],
    tasks: &DispatchTasks,
    default_timeout: Duration,
    limits: SyntaxLimits,
) -> Result<Response<Full<bytes::Bytes>>, RequestAbandoned>
where
    B: Body<Data = bytes::Bytes> + Unpin + Send + 'static,
{
    handle_request_with(
        request,
        head_received,
        exposures,
        default_timeout,
        limits,
        |exposure, context, input| dispatch_request(tasks, exposure, context, input),
    )
    .await
}

async fn handle_request_with<B, E, F, Fut>(
    request: Request<B>,
    head_received: Instant,
    exposures: &[E],
    default_timeout: Duration,
    limits: SyntaxLimits,
    dispatch: F,
) -> Result<Response<Full<bytes::Bytes>>, RequestAbandoned>
where
    B: Body<Data = bytes::Bytes> + Unpin,
    E: ExposureView + Clone,
    F: FnOnce(E, CallContext, SlotValue) -> Fut,
    Fut: Future<Output = DispatchOutcome>,
{
    let (head, body) = request.into_parts();
    let admitted = match admit_request_head(
        head.uri.path(),
        head.uri.query().is_some(),
        &head.method,
        &head.headers,
        exposures,
    ) {
        Ok(admitted) => admitted,
        Err(error) => return Ok(http_error(error)),
    };
    let exposure = admitted.exposure.clone();
    let context = match prepare_call_context(
        head_received,
        admitted.timeout,
        default_timeout,
        admitted.trace_context,
        admitted.idempotency_key,
    ) {
        Ok(context) => context,
        Err(error) => return Ok(http_error(error)),
    };
    let Some(deadline) = context.deadline() else {
        return Ok(http_error(WireCallError::Internal));
    };
    let input = match collect_and_decode_request_body(
        body,
        exposure.descriptor().input(),
        limits,
        deadline,
    )
    .await
    {
        Ok(input) => input,
        Err(error) => return Ok(http_error(error)),
    };
    let mut cancel_on_drop = CancelOnDrop::new(context.cancellation().clone());
    let outcome = dispatch(exposure, context, input).await;
    cancel_on_drop.disarm();
    match outcome {
        DispatchOutcome::Response(response) => Ok(http_response(response)),
        DispatchOutcome::Abandoned => Err(RequestAbandoned),
    }
}

fn http_error(error: WireCallError) -> Response<Full<bytes::Bytes>> {
    let DispatchOutcome::Response(response) = response_error(error) else {
        unreachable!()
    };
    http_response(response)
}

fn http_response(encoded: EncodedResponse) -> Response<Full<bytes::Bytes>> {
    let mut response = Response::new(Full::new(bytes::Bytes::from(encoded.body)));
    *response.status_mut() =
        StatusCode::from_u16(encoded.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if response.status() == StatusCode::METHOD_NOT_ALLOWED {
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("POST"));
    }
    response
}

fn decode_request_body(
    body: &[u8],
    descriptor: &TypeDescriptor,
    limits: SyntaxLimits,
) -> Result<SlotValue, WireCallError> {
    let tree = parse(body, limits).map_err(|error| match error {
        SyntaxError::PayloadTooLarge { .. } => WireCallError::PayloadTooLarge,
        _ => WireCallError::InvalidRequest,
    })?;
    decode_tree(tree, descriptor, DecodeRole::ProviderInput)
        .map_err(|_| WireCallError::InvalidRequest)
}

async fn collect_and_decode_request_body<B>(
    body: B,
    descriptor: &TypeDescriptor,
    limits: SyntaxLimits,
    deadline: Deadline,
) -> Result<SlotValue, WireCallError>
where
    B: Body<Data = bytes::Bytes> + Unpin,
{
    if body.size_hint().lower() > limits.0 as u64 {
        return Err(WireCallError::PayloadTooLarge);
    }

    let collection = async move {
        let mut body = body;
        let mut collected = Vec::new();
        loop {
            for _ in 0..BODY_FRAME_QUANTUM {
                let next = poll_fn(|context| Pin::new(&mut body).poll_frame(context)).await;
                let Some(frame) = next else {
                    return Ok(collected);
                };
                let frame = frame.map_err(|_| WireCallError::InvalidRequest)?;
                let Ok(data) = frame.into_data() else {
                    continue;
                };
                let remaining = limits
                    .0
                    .checked_sub(collected.len())
                    .ok_or(WireCallError::PayloadTooLarge)?;
                if data.len() > remaining {
                    return Err(WireCallError::PayloadTooLarge);
                }
                reserve_body_capacity(&mut collected, data.len())?;
                collected.extend_from_slice(&data);
            }
            tokio::task::yield_now().await;
        }
    };
    let mut timeout = Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(
        deadline.instant(),
    )));
    tokio::pin!(collection);
    let collected = tokio::select! {
        biased;
        result = &mut collection => result?,
        () = &mut timeout => return Err(WireCallError::DeadlineExceeded),
    };
    decode_request_body(&collected, descriptor, limits)
}

fn reserve_body_capacity(body: &mut Vec<u8>, additional: usize) -> Result<(), WireCallError> {
    body.try_reserve_exact(additional)
        .map_err(|_| WireCallError::Internal)
}

fn one_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a HeaderValue>, WireCallError> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(first)
}

fn parse_timeout(headers: &HeaderMap) -> Result<Option<Duration>, WireCallError> {
    let Some(value) = one_header(headers, TIMEOUT_HEADER)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || (bytes.len() > 1 && bytes[0] == b'0')
        || bytes.iter().any(|byte| !byte.is_ascii_digit())
    {
        return Err(WireCallError::InvalidRequest);
    }
    let millis = bytes
        .iter()
        .fold(0_u64, |value, byte| value * 10 + u64::from(byte - b'0'));
    if millis > 9_999_999_999 {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(Some(Duration::from_millis(millis)))
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<Option<IdempotencyKey>, WireCallError> {
    let Some(value) = one_header(headers, IDEMPOTENCY_HEADER)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 256
        || bytes
            .iter()
            .any(|byte| !(0x21..=0x7e).contains(byte) || *byte == b',')
    {
        return Err(WireCallError::InvalidRequest);
    }
    let key = std::str::from_utf8(bytes).map_err(|_| WireCallError::InvalidRequest)?;
    IdempotencyKey::new(key)
        .map(Some)
        .map_err(|_| WireCallError::InvalidRequest)
}

fn parse_trace_context(headers: &HeaderMap) -> TraceContext {
    let mut parents = headers.get_all(TRACEPARENT_HEADER).iter();
    let Some(parent) = parents.next() else {
        return TraceContext::empty();
    };
    if parents.next().is_some() || !valid_traceparent(parent.as_bytes()) {
        return TraceContext::empty();
    }
    let Ok(parent) = parent.to_str() else {
        return TraceContext::empty();
    };
    let state = parse_tracestate(headers);
    TraceContext::new(Some(parent.to_owned()), state)
}

fn valid_traceparent(value: &[u8]) -> bool {
    if value.len() < 55
        || value[2] != b'-'
        || value[35] != b'-'
        || value[52] != b'-'
        || !value[..55]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
    {
        return false;
    }
    let lowercase_hex = |part: &[u8]| {
        part.iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    };
    if !lowercase_hex(&value[..2])
        || value[..2] == *b"ff"
        || !lowercase_hex(&value[3..35])
        || value[3..35].iter().all(|byte| *byte == b'0')
        || !lowercase_hex(&value[36..52])
        || value[36..52].iter().all(|byte| *byte == b'0')
        || !lowercase_hex(&value[53..55])
    {
        return false;
    }
    if value[..2] == *b"00" {
        value.len() == 55
    } else {
        value.len() == 55 || value[55] == b'-'
    }
}

fn parse_tracestate(headers: &HeaderMap) -> Option<String> {
    let values: Vec<_> = headers.get_all(TRACESTATE_HEADER).iter().collect();
    if values.is_empty() {
        return None;
    }
    let combined_len = values
        .iter()
        .map(|value| value.as_bytes().len())
        .sum::<usize>()
        + values.len().saturating_sub(1);
    if combined_len > MAX_TRACESTATE_BYTES {
        return None;
    }
    let mut combined = String::with_capacity(combined_len);
    for (index, value) in values.into_iter().enumerate() {
        if index != 0 {
            combined.push(',');
        }
        combined.push_str(value.to_str().ok()?);
    }
    valid_tracestate(&combined).then_some(combined)
}

fn valid_tracestate(value: &str) -> bool {
    let mut keys = Vec::new();
    let members: Vec<_> = value.split(',').collect();
    if members.len() > 32 {
        return false;
    }
    let last = members.len() - 1;
    for (index, member) in members.into_iter().enumerate() {
        let member = member.trim_start_matches([' ', '\t']);
        let member = if index == last {
            member
        } else {
            member.trim_end_matches([' ', '\t'])
        };
        if member.is_empty() {
            continue;
        }
        let Some((key, state)) = member.split_once('=') else {
            return false;
        };
        if !valid_tracestate_key(key)
            || state.is_empty()
            || state.len() > 256
            || state
                .as_bytes()
                .iter()
                .any(|byte| !(0x20..=0x7e).contains(byte) || *byte == b',' || *byte == b'=')
            || state.ends_with(' ')
            || keys.contains(&key)
        {
            return false;
        }
        keys.push(key);
    }
    true
}

fn valid_tracestate_key(key: &str) -> bool {
    let allowed = |byte: u8| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'-' | b'*' | b'/')
    };
    if let Some((tenant, system)) = key.split_once('@') {
        !tenant.is_empty()
            && tenant.len() <= 241
            && (tenant.as_bytes()[0].is_ascii_lowercase() || tenant.as_bytes()[0].is_ascii_digit())
            && tenant.bytes().all(allowed)
            && !system.is_empty()
            && system.len() <= 14
            && system.as_bytes()[0].is_ascii_lowercase()
            && system.bytes().all(allowed)
    } else {
        !key.is_empty()
            && key.len() <= 256
            && key.as_bytes()[0].is_ascii_lowercase()
            && key.bytes().all(allowed)
    }
}

struct AdmittedHead<'a, E> {
    exposure: &'a E,
    timeout: Option<Duration>,
    idempotency_key: Option<IdempotencyKey>,
    trace_context: TraceContext,
}

fn admit_request_head<'a, E: ExposureView>(
    raw_path: &str,
    query_present: bool,
    method: &Method,
    headers: &HeaderMap,
    exposures: &'a [E],
) -> Result<AdmittedHead<'a, E>, WireCallError> {
    let exposure = resolve_route(raw_path, query_present, exposures)?;
    if method != Method::POST {
        return Err(WireCallError::MethodNotAllowed);
    }
    validate_request_media(headers)?;
    let timeout = parse_timeout(headers)?;
    let idempotency_key = parse_idempotency_key(headers)?;
    let trace_context = parse_trace_context(headers);
    Ok(AdmittedHead {
        exposure,
        timeout,
        idempotency_key,
        trace_context,
    })
}

fn validate_request_media(headers: &HeaderMap) -> Result<(), WireCallError> {
    let Some(content_type) = one_header(headers, "content-type")? else {
        return Err(WireCallError::UnsupportedMediaType);
    };
    let value = content_type
        .to_str()
        .map_err(|_| WireCallError::UnsupportedMediaType)?;
    validate_json_content_type(value)?;
    if headers.contains_key("content-encoding") {
        return Err(WireCallError::UnsupportedMediaType);
    }
    Ok(())
}

fn validate_json_content_type(value: &str) -> Result<(), WireCallError> {
    let mut list = MediaTypeList::new(value);
    let first = list.next();
    let trimmed = value.trim_end_matches([' ', '\t']);
    let has_trailing_comma = trimmed.ends_with(',');
    if list.next().is_some() || has_trailing_comma {
        return Err(WireCallError::InvalidRequest);
    }
    if trimmed.ends_with(';') {
        return Err(WireCallError::UnsupportedMediaType);
    }
    let media_type = first
        .ok_or(WireCallError::UnsupportedMediaType)?
        .map_err(|_| WireCallError::UnsupportedMediaType)?;
    if media_type.ty != names::APPLICATION
        || media_type.subty != names::JSON
        || media_type.suffix.is_some()
    {
        return Err(WireCallError::UnsupportedMediaType);
    }
    match media_type.params.as_ref() {
        [] => Ok(()),
        [(name, value)]
            if *name == names::CHARSET && value.unquoted_str().eq_ignore_ascii_case("utf-8") =>
        {
            Ok(())
        }
        _ => Err(WireCallError::UnsupportedMediaType),
    }
}

trait ExposureView {
    fn descriptor(&self) -> &CapabilityDescriptor;
    fn level(&self) -> ExposureLevel;
}

impl ExposureView for TransportExposure {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.descriptor()
    }

    fn level(&self) -> ExposureLevel {
        self.level()
    }
}

fn resolve_route<'a, E: ExposureView>(
    raw_path: &str,
    query_present: bool,
    exposures: &'a [E],
) -> Result<&'a E, WireCallError> {
    let Some(rest) = raw_path.strip_prefix("/rpc/") else {
        return Err(WireCallError::UnknownBox);
    };
    let (box_segment, capability_segment) = rest.split_once('/').unwrap_or((rest, ""));
    let box_id = BoxId::new(box_segment).map_err(|_| WireCallError::UnknownBox)?;
    let box_known = exposures
        .iter()
        .any(|exposure| exposure.descriptor().id().box_id() == &box_id);
    if !box_known {
        return Err(WireCallError::UnknownBox);
    }
    if capability_segment.contains('/') {
        return Err(WireCallError::UnknownCapability);
    }
    let capability =
        CapabilityName::new(capability_segment).map_err(|_| WireCallError::UnknownCapability)?;
    let exposure = exposures
        .iter()
        .find(|exposure| {
            let id = exposure.descriptor().id();
            id.box_id() == &box_id && id.name() == &capability
        })
        .ok_or(WireCallError::UnknownCapability)?;
    if query_present {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(exposure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::conform_capability;
    use boxology_contract::{
        CapabilityId, CapabilityShape, ContractValue, Detail, FieldDescriptor, Idempotency,
        VariantDescriptor, VariantPayload,
    };
    use http::{HeaderValue, header::ACCEPT};
    use http_body::{Frame, SizeHint};
    use std::{
        collections::VecDeque,
        error::Error,
        fmt,
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Exposure {
        descriptor: CapabilityDescriptor,
        level: ExposureLevel,
    }

    impl ExposureView for Exposure {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn level(&self) -> ExposureLevel {
            self.level
        }
    }

    fn capability(box_id: &str, name: &str, input: TypeDescriptor) -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new(box_id).unwrap(),
                CapabilityName::new(name).unwrap(),
            ),
            input,
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
    }

    fn exposure(box_id: &str, name: &str, level: ExposureLevel) -> Exposure {
        Exposure {
            descriptor: capability(box_id, name, TypeDescriptor::string()),
            level,
        }
    }

    fn with_slots(
        shape: CapabilityShape,
        input: TypeDescriptor,
        output: TypeDescriptor,
        error: TypeDescriptor,
    ) -> CapabilityDescriptor {
        let mut descriptor = capability("box", "call", input);
        descriptor = CapabilityDescriptor::new(
            descriptor.id().clone(),
            descriptor.input().clone(),
            output,
            error,
            shape,
            ExposureLevel::External,
            Idempotency::None,
            None,
        );
        descriptor
    }

    #[test]
    fn context_uses_receipt_time_override_and_exact_metadata() {
        let receipt = Instant::now();
        let trace = TraceContext::new(Some("parent".into()), Some("state".into()));
        let key = IdempotencyKey::new("key").unwrap();
        let first = prepare_call_context(
            receipt,
            None,
            Duration::from_secs(9),
            trace.clone(),
            Some(key.clone()),
        )
        .unwrap();
        let second = prepare_call_context(
            receipt,
            Some(Duration::ZERO),
            Duration::from_secs(9),
            trace.clone(),
            Some(key.clone()),
        )
        .unwrap();
        assert_eq!(first.caller(), Caller::Anonymous);
        assert_eq!(
            first.deadline().unwrap().instant(),
            receipt + Duration::from_secs(9)
        );
        assert_eq!(second.deadline().unwrap().instant(), receipt);
        assert_eq!(first.trace(), &trace);
        assert_eq!(first.idempotency_key(), Some(&key));
        first.cancellation().cancel();
        assert!(!second.cancellation().is_cancelled());
        assert!(matches!(
            prepare_call_context(receipt, Some(Duration::MAX), Duration::ZERO, trace, None),
            Err(WireCallError::Internal)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn owner_cancels_aborts_cleans_and_waits_without_missing_completion() {
        let tracker = TransportTaskTracker::new();
        let tasks = DispatchTasks::new(tracker.clone());
        let tokens = [CancelToken::new(), CancelToken::new()];
        let first = tasks.spawn(tokens[0].clone(), std::future::pending::<()>());
        let second = tasks.spawn(tokens[1].clone(), std::future::pending::<()>());
        assert_eq!((tasks.len(), tracker.len()), (2, 2));
        tasks.cancel_all();
        assert!(tokens.iter().all(CancelToken::is_cancelled));
        tasks.abort_all();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(second.await.unwrap_err().is_cancelled());
        tasks.wait_empty().await;
        assert_eq!((tasks.len(), tracker.len()), (0, 0));

        let registered = tasks.clone();
        tasks
            .spawn(CancelToken::new(), async move {
                assert_eq!(registered.len(), 1);
            })
            .await
            .unwrap();
        tasks.wait_empty().await;
        assert_eq!(tasks.len(), 0);

        let release = Arc::new(tokio::sync::Notify::new());
        let waiter = release.clone();
        let detached = tasks.spawn(CancelToken::new(), async move { waiter.notified().await });
        drop(detached);
        assert_eq!(tasks.len(), 1);
        let waiting = tokio::spawn({
            let tasks = tasks.clone();
            async move { tasks.wait_empty().await }
        });
        tokio::task::yield_now().await;
        release.notify_one();
        waiting.await.unwrap();
        assert_eq!(tasks.len(), 0);

        assert!(
            tasks
                .spawn(CancelToken::new(), async { panic!("owned panic") })
                .await
                .is_err()
        );
        tasks.wait_empty().await;
        assert_eq!((tasks.len(), tracker.len()), (0, 0));
    }

    fn dispatch_descriptor(
        output: TypeDescriptor,
        error: TypeDescriptor,
    ) -> &'static CapabilityDescriptor {
        Box::leak(Box::new(with_slots(
            CapabilityShape::Unary,
            TypeDescriptor::string(),
            output,
            error,
        )))
    }

    fn string_dispatch_descriptor() -> &'static CapabilityDescriptor {
        dispatch_descriptor(
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
        )
    }

    fn dispatch_context(after: Option<Duration>, cancellation: CancelToken) -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            after.map(|after| Deadline::at(tokio::time::Instant::now().into_std() + after)),
            cancellation,
            TraceContext::empty(),
            None,
        )
    }

    fn exact_error(error: WireCallError) -> DispatchOutcome {
        let (status, code, message) = error.spec();
        DispatchOutcome::Response(EncodedResponse {
            status,
            body: format!(r#"{{"error":{{"kind":"call","code":"{code}","message":"{message}"}}}}"#)
                .into_bytes(),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn dispatch_rejects_missing_and_expired_deadlines_before_spawning() {
        let descriptor = string_dispatch_descriptor();
        for (after, expected, cancelled) in [
            (None, WireCallError::Internal, false),
            (Some(Duration::ZERO), WireCallError::DeadlineExceeded, true),
        ] {
            let tracker = TransportTaskTracker::new();
            let tasks = DispatchTasks::new(tracker.clone());
            let token = CancelToken::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let invoked = calls.clone();
            let outcome = dispatch_request_with(
                &tasks,
                descriptor,
                dispatch_context(after, token.clone()),
                SlotValue::Null,
                move |_, _| {
                    invoked.fetch_add(1, Ordering::Relaxed);
                    std::future::ready(Ok(SlotValue::Null))
                },
            )
            .await;
            assert_eq!(outcome, exact_error(expected));
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            assert_eq!(tasks.len() + tracker.len(), 0);
            assert_eq!(token.is_cancelled(), cancelled);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dispatch_encodes_success_domain_and_all_call_errors_canonically() {
        let errors = TypeDescriptor::enumeration([VariantDescriptor::new(
            "denied",
            VariantPayload::Value(TypeDescriptor::string()),
            None,
        )])
        .unwrap();
        let descriptor = dispatch_descriptor(TypeDescriptor::string(), errors);
        let cases = [
            (
                Ok(SlotValue::Value(ContractValue::string("yes"))),
                200,
                br#"{"result":{"value":"yes"}}"#.as_slice(),
            ),
            (
                Err(ErasedCallError::Domain {
                    error_tag: "denied".into(),
                    payload: SlotValue::Value(ContractValue::string("no")),
                }),
                422,
                br#"{"error":{"kind":"domain","value":{"tag":"denied","payload":"no"}}}"#
                    .as_slice(),
            ),
        ];
        for (result, status, body) in cases {
            assert_eq!(
                encode_dispatch_result(result, descriptor),
                DispatchOutcome::Response(EncodedResponse {
                    status,
                    body: body.to_vec()
                })
            );
        }

        let detail = || Detail::new("DO_NOT_LEAK").with_message("DO_NOT_LEAK");
        let failures = [
            (ErasedCallError::Deadline, WireCallError::DeadlineExceeded),
            (ErasedCallError::Cancelled, WireCallError::Internal),
            (
                ErasedCallError::Unavailable(detail()),
                WireCallError::Unavailable,
            ),
            (
                ErasedCallError::ContractViolation(detail()),
                WireCallError::InvalidRequest,
            ),
            (
                ErasedCallError::InvalidResponse(detail()),
                WireCallError::InvalidUpstreamResponse,
            ),
            (ErasedCallError::Internal(detail()), WireCallError::Internal),
        ];
        for (failure, expected) in failures {
            assert_eq!(
                encode_dispatch_result(Err(failure), descriptor),
                exact_error(expected)
            );
        }
    }

    #[test]
    fn invalid_handler_values_are_canonical_502_without_payload_leaks() {
        let descriptor = string_dispatch_descriptor();
        let malformed = [
            Ok(SlotValue::Null),
            Err(ErasedCallError::Domain {
                error_tag: "unknown".into(),
                payload: SlotValue::Value(ContractValue::sensitive(ContractValue::string(
                    "DO_NOT_LEAK",
                ))),
            }),
        ];
        for result in malformed {
            let outcome = encode_dispatch_result(result, descriptor);
            assert_eq!(outcome, exact_error(WireCallError::InvalidUpstreamResponse));
            let DispatchOutcome::Response(response) = outcome else {
                unreachable!()
            };
            assert!(!String::from_utf8_lossy(&response.body).contains("DO_NOT_LEAK"));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn handler_internal_and_genuine_join_failure_are_distinct_but_canonical_500() {
        let descriptor = string_dispatch_descriptor();
        let tasks = DispatchTasks::new(TransportTaskTracker::new());
        let token = CancelToken::new();
        let deadline = dispatch_context(Some(Duration::from_secs(1)), token.clone())
            .deadline()
            .unwrap();
        let guarded = tasks.spawn(token.clone(), async {
            Err(ErasedCallError::Internal(Detail::new("DO_NOT_LEAK")))
        });
        assert_eq!(
            await_dispatch(guarded, token.clone(), deadline, descriptor).await,
            exact_error(WireCallError::Internal)
        );
        let panicked = tasks.spawn(token.clone(), async { panic!("DO_NOT_LEAK") });
        assert_eq!(
            await_dispatch(panicked, token, deadline, descriptor).await,
            exact_error(WireCallError::Internal)
        );
        tasks.wait_empty().await;
    }

    #[tokio::test(start_paused = true)]
    async fn dispatch_race_order_is_completion_then_deadline_then_cancellation() {
        let descriptor = string_dispatch_descriptor();
        let tasks = DispatchTasks::new(TransportTaskTracker::new());
        let token = CancelToken::new();
        token.cancel();
        let done = tasks.spawn(token.clone(), async {
            Ok(SlotValue::Value(ContractValue::string("done")))
        });
        tokio::task::yield_now().await;
        let now = Deadline::at(tokio::time::Instant::now().into_std());
        assert_eq!(
            await_dispatch(done, token.clone(), now, descriptor).await,
            DispatchOutcome::Response(EncodedResponse {
                status: 200,
                body: br#"{"result":{"value":"done"}}"#.to_vec()
            })
        );

        let pending = tasks.spawn(token.clone(), std::future::pending());
        assert_eq!(
            await_dispatch(pending, token, now, descriptor).await,
            exact_error(WireCallError::DeadlineExceeded)
        );
        tasks.abort_all();
        tasks.wait_empty().await;
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_and_request_cancellation_leave_dispatch_owned_until_completion() {
        for deadline_wins in [true, false] {
            let tracker = TransportTaskTracker::new();
            let tasks = DispatchTasks::new(tracker.clone());
            let token = CancelToken::new();
            let release = Arc::new(tokio::sync::Notify::new());
            let waiter = release.clone();
            let handler_token = token.clone();
            let task = tasks.spawn(token.clone(), async move {
                waiter.notified().await;
                assert!(handler_token.is_cancelled());
                Ok(SlotValue::Value(ContractValue::string("late")))
            });
            let descriptor = string_dispatch_descriptor();
            let deadline =
                Deadline::at(tokio::time::Instant::now().into_std() + Duration::from_secs(1));
            let wait = tokio::spawn({
                let token = token.clone();
                async move { await_dispatch(task, token, deadline, descriptor).await }
            });
            tokio::task::yield_now().await;
            if deadline_wins {
                tokio::time::advance(Duration::from_secs(1)).await;
            } else {
                token.cancel();
            }
            let outcome = wait.await.unwrap();
            assert_eq!(
                outcome,
                if deadline_wins {
                    exact_error(WireCallError::DeadlineExceeded)
                } else {
                    DispatchOutcome::Abandoned
                }
            );
            assert_eq!((tasks.len(), tracker.len()), (1, 1));
            release.notify_one();
            tasks.wait_empty().await;
            assert_eq!((tasks.len(), tracker.len()), (0, 0));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn expiry_between_admission_and_task_invocation_skips_handler() {
        let token = CancelToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = calls.clone();
        let descriptor = string_dispatch_descriptor();
        let context = dispatch_context(Some(Duration::from_secs(1)), token.clone());
        let deadline = context.deadline().unwrap();
        let request = invoke_if_live(
            deadline,
            token.clone(),
            context,
            SlotValue::Null,
            move |_, _| {
                invoked.fetch_add(1, Ordering::Relaxed);
                std::future::ready(Ok(SlotValue::Null))
            },
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            encode_dispatch_result(request.await, descriptor),
            exact_error(WireCallError::DeadlineExceeded)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(token.is_cancelled());
    }

    fn body_limits(max_bytes: usize) -> SyntaxLimits {
        SyntaxLimits(max_bytes, crate::syntax::DEFAULT_DEPTH_LIMIT)
    }

    #[derive(Debug)]
    struct BodyFailure;

    impl fmt::Display for BodyFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("body failure")
        }
    }

    impl Error for BodyFailure {}

    struct TestBody {
        frames: VecDeque<Result<Frame<bytes::Bytes>, BodyFailure>>,
        lower: u64,
        delay: Option<Pin<Box<tokio::time::Sleep>>>,
        delay_after_first: Option<Duration>,
        polls: Arc<AtomicUsize>,
    }

    impl TestBody {
        fn new(frames: impl IntoIterator<Item = Result<Frame<bytes::Bytes>, BodyFailure>>) -> Self {
            Self {
                frames: frames.into_iter().collect(),
                lower: 0,
                delay: None,
                delay_after_first: None,
                polls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn delayed(mut self, delay: Duration) -> Self {
            self.delay = Some(Box::pin(tokio::time::sleep(delay)));
            self
        }

        fn trickled(mut self, delay: Duration) -> Self {
            self.delay_after_first = Some(delay);
            self
        }
    }

    impl Body for TestBody {
        type Data = bytes::Bytes;
        type Error = BodyFailure;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            self.polls.fetch_add(1, Ordering::Relaxed);
            if let Some(delay) = &mut self.delay {
                if delay.as_mut().poll(context).is_pending() {
                    return Poll::Pending;
                }
                self.delay = None;
            }
            let frame = self.frames.pop_front();
            if frame.is_some()
                && let Some(delay) = self.delay_after_first.take()
            {
                self.delay = Some(Box::pin(tokio::time::sleep(delay)));
            }
            Poll::Ready(frame)
        }

        fn size_hint(&self) -> SizeHint {
            let mut hint = SizeHint::new();
            hint.set_lower(self.lower);
            hint
        }
    }

    struct EndlessBody(Arc<AtomicUsize>);

    impl Body for EndlessBody {
        type Data = bytes::Bytes;
        type Error = BodyFailure;

        fn poll_frame(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Poll::Ready(Some(Ok(Frame::data(bytes::Bytes::new()))))
        }
    }

    fn data(value: &'static [u8]) -> Result<Frame<bytes::Bytes>, BodyFailure> {
        Ok(Frame::data(bytes::Bytes::from_static(value)))
    }

    fn rpc_request(body: TestBody) -> Request<TestBody> {
        Request::builder()
            .method(Method::POST)
            .uri("/rpc/box/call")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    async fn response_parts(
        response: Response<Full<bytes::Bytes>>,
    ) -> (StatusCode, HeaderMap, bytes::Bytes) {
        use http_body_util::BodyExt;

        let (parts, body) = response.into_parts();
        (
            parts.status,
            parts.headers,
            body.collect().await.unwrap().to_bytes(),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn request_orchestration_preserves_selection_input_context_and_response() {
        let exposures = [
            exposure("other", "call", ExposureLevel::External),
            exposure("box", "call", ExposureLevel::External),
        ];
        let received = tokio::time::Instant::now().into_std();
        let request = Request::builder()
            .method(Method::POST)
            .uri("/rpc/box/call")
            .header(header::CONTENT_TYPE, "application/json")
            .header(TIMEOUT_HEADER, "2500")
            .header(IDEMPOTENCY_HEADER, "same-request")
            .header(
                TRACEPARENT_HEADER,
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            .header(TRACESTATE_HEADER, "vendor=value")
            .body(TestBody::new([data(b"\"hello\"")]))
            .unwrap();
        let cancellation = Arc::new(Mutex::new(None));
        let observed = cancellation.clone();
        let response = handle_request_with(
            request,
            received,
            &exposures,
            Duration::from_secs(9),
            body_limits(64),
            move |selected, context, input| async move {
                assert_eq!(selected.descriptor.id().to_string(), "box.call");
                assert_eq!(input, SlotValue::Value(ContractValue::string("hello")));
                assert_eq!(
                    context.deadline().unwrap().instant(),
                    received + Duration::from_millis(2500)
                );
                assert_eq!(
                    context.trace(),
                    &TraceContext::new(
                        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into()),
                        Some("vendor=value".into())
                    )
                );
                assert_eq!(context.idempotency_key().unwrap().as_str(), "same-request");
                *observed.lock().unwrap() = Some(context.cancellation().clone());
                DispatchOutcome::Response(EncodedResponse {
                    status: 200,
                    body: br#"{"result":{"value":"hello"}}"#.to_vec(),
                })
            },
        )
        .await
        .unwrap();
        let (status, headers, body) = response_parts(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert!(!headers.contains_key(header::ALLOW));
        assert_eq!(body, br#"{"result":{"value":"hello"}}"#.as_slice());
        assert!(
            !cancellation
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn head_and_body_failures_return_exact_http_without_dispatch() {
        let exposures = [exposure("box", "call", ExposureLevel::External)];
        let calls = Arc::new(AtomicUsize::new(0));
        let assert_never_called = |calls: Arc<AtomicUsize>| {
            move |_: Exposure, _: CallContext, _: SlotValue| {
                calls.fetch_add(1, Ordering::Relaxed);
                std::future::ready(DispatchOutcome::Abandoned)
            }
        };

        let body = TestBody::new([data(b"\"ignored\"")]);
        let polls = body.polls.clone();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/rpc/box/call")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap();
        let response = handle_request_with(
            request,
            tokio::time::Instant::now().into_std(),
            &exposures,
            Duration::from_secs(1),
            body_limits(64),
            assert_never_called(calls.clone()),
        )
        .await
        .unwrap();
        let (status, headers, body) = response_parts(response).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers[header::ALLOW], "POST");
        assert_eq!(body, WireCallError::MethodNotAllowed.encode().body());
        assert_eq!(polls.load(Ordering::Relaxed), 0);

        let mut request =
            rpc_request(TestBody::new([data(b"\"late\"")]).delayed(Duration::from_millis(2)));
        request
            .headers_mut()
            .insert(TIMEOUT_HEADER, HeaderValue::from_static("1"));
        let response = handle_request_with(
            request,
            tokio::time::Instant::now().into_std(),
            &exposures,
            Duration::from_secs(1),
            body_limits(64),
            assert_never_called(calls.clone()),
        )
        .await
        .unwrap();
        let (status, headers, body) = response_parts(response).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert!(!headers.contains_key(header::ALLOW));
        assert_eq!(body, WireCallError::DeadlineExceeded.encode().body());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn request_drop_and_explicit_cancel_preserve_owned_dispatch_until_cleanup() {
        for drop_request in [true, false] {
            let tracker = TransportTaskTracker::new();
            let tasks = DispatchTasks::new(tracker.clone());
            let exposures = vec![exposure("box", "call", ExposureLevel::External)];
            let descriptor = string_dispatch_descriptor();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let completed = Arc::new(AtomicUsize::new(0));
            let token = Arc::new(Mutex::new(None));
            let owned_tasks = tasks.clone();
            let seen_token = token.clone();
            let handler_entered = entered.clone();
            let handler_release = release.clone();
            let handler_completed = completed.clone();
            let request = tokio::spawn(async move {
                handle_request_with(
                    rpc_request(TestBody::new([data(b"\"work\"")])),
                    tokio::time::Instant::now().into_std(),
                    &exposures,
                    Duration::from_secs(10),
                    body_limits(64),
                    move |_, context, input| {
                        *seen_token.lock().unwrap() = Some(context.cancellation().clone());
                        async move {
                            dispatch_request_with(
                                &owned_tasks,
                                descriptor,
                                context,
                                input,
                                move |context, _| async move {
                                    handler_entered.notify_one();
                                    handler_release.notified().await;
                                    assert!(context.cancellation().is_cancelled());
                                    handler_completed.fetch_add(1, Ordering::Relaxed);
                                    Ok(SlotValue::Value(ContractValue::string("late")))
                                },
                            )
                            .await
                        }
                    },
                )
                .await
            });
            entered.notified().await;
            assert_eq!((tasks.len(), tracker.len()), (1, 1));
            let cancellation = token.lock().unwrap().clone().unwrap();
            if drop_request {
                request.abort();
                assert!(request.await.unwrap_err().is_cancelled());
                tokio::task::yield_now().await;
            } else {
                cancellation.cancel();
                assert!(matches!(request.await.unwrap(), Err(RequestAbandoned)));
            }
            assert!(cancellation.is_cancelled());
            assert_eq!((tasks.len(), tracker.len()), (1, 1));
            release.notify_one();
            tasks.wait_empty().await;
            assert_eq!((tasks.len(), tracker.len()), (0, 0));
            assert_eq!(completed.load(Ordering::Relaxed), 1);
        }
    }

    async fn collect(
        body: TestBody,
        max_bytes: usize,
        after: Duration,
    ) -> Result<SlotValue, WireCallError> {
        collect_and_decode_request_body(
            body,
            &TypeDescriptor::string(),
            body_limits(max_bytes),
            Deadline::at(tokio::time::Instant::now().into_std() + after),
        )
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn streaming_body_accepts_exact_limit_multiple_frames_and_trailers() {
        let frames = [
            data(b"\"he"),
            Ok(Frame::trailers(headers("x-trailer", b"ignored"))),
            data(b"llo\""),
        ];
        assert_eq!(
            collect(TestBody::new(frames), 7, Duration::from_secs(1)).await,
            Ok(SlotValue::Value(ContractValue::string("hello")))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn body_size_failures_precede_decode_and_definitive_hint_is_not_polled() {
        let mut hinted = TestBody::new([data(b"\"ok\"")]);
        hinted.lower = 6;
        let polls = hinted.polls.clone();
        assert_eq!(
            collect(hinted, 5, Duration::from_secs(1)).await,
            Err(WireCallError::PayloadTooLarge)
        );
        assert_eq!(polls.load(Ordering::Relaxed), 0);

        for frames in [
            vec![data(b"{bad"), data(b" payload")],
            vec![data(b"\"okay\""), data(b"x")],
        ] {
            assert_eq!(
                collect(TestBody::new(frames), 6, Duration::from_secs(1)).await,
                Err(WireCallError::PayloadTooLarge)
            );
        }
        assert_eq!(
            collect(TestBody::new([data(b"{bad")]), 4, Duration::from_secs(1)).await,
            Err(WireCallError::InvalidRequest)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn body_deadline_covers_already_expired_and_trickled_input() {
        assert_eq!(
            collect(
                TestBody::new([data(b"\"late\"")]).delayed(Duration::from_secs(1)),
                64,
                Duration::ZERO,
            )
            .await,
            Err(WireCallError::DeadlineExceeded)
        );
        assert_eq!(
            collect(
                TestBody::new([data(b"\"la"), data(b"te\"")]).trickled(Duration::from_secs(2)),
                64,
                Duration::from_secs(1),
            )
            .await,
            Err(WireCallError::DeadlineExceeded)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn completion_wins_at_deadline_and_stream_errors_are_bad_requests() {
        assert_eq!(
            collect(
                TestBody::new([data(b"\"done\"")]).delayed(Duration::from_secs(1)),
                6,
                Duration::from_secs(1),
            )
            .await,
            Ok(SlotValue::Value(ContractValue::string("done")))
        );
        assert_eq!(
            collect(
                TestBody::new([Err(BodyFailure)]).delayed(Duration::from_secs(1)),
                64,
                Duration::from_secs(1),
            )
            .await,
            Err(WireCallError::InvalidRequest)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn endlessly_ready_empty_frames_cannot_starve_deadline() {
        let polls = Arc::new(AtomicUsize::new(0));
        let descriptor = TypeDescriptor::string();
        let mut collection = Box::pin(collect_and_decode_request_body(
            EndlessBody(polls.clone()),
            &descriptor,
            body_limits(64),
            Deadline::at(tokio::time::Instant::now().into_std() + Duration::from_secs(1)),
        ));
        let mut context = Context::from_waker(Waker::noop());
        assert!(collection.as_mut().poll(&mut context).is_pending());
        assert_eq!(polls.load(Ordering::Relaxed), BODY_FRAME_QUANTUM);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(matches!(
            collection.as_mut().poll(&mut context),
            Poll::Ready(Err(WireCallError::DeadlineExceeded))
        ));
        assert_eq!(polls.load(Ordering::Relaxed), BODY_FRAME_QUANTUM * 2);
    }

    #[test]
    fn capacity_overflow_is_an_internal_resource_failure() {
        assert_eq!(
            reserve_body_capacity(&mut Vec::new(), usize::MAX),
            Err(WireCallError::Internal)
        );
    }

    fn assert_body_error(
        body: &[u8],
        descriptor: &TypeDescriptor,
        limits: SyntaxLimits,
        expected: WireCallError,
    ) {
        let error = decode_request_body(body, descriptor, limits).unwrap_err();
        assert_eq!(error, expected);
        let encoded = error.encode();
        let (status, body) = match expected {
            WireCallError::InvalidRequest => (400, br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#.as_slice()),
            WireCallError::PayloadTooLarge => (413, br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#.as_slice()),
            _ => unreachable!(),
        };
        assert_eq!(encoded.status(), status);
        assert_eq!(encoded.body(), body);
        assert!(
            !encoded
                .body()
                .windows(11)
                .any(|bytes| bytes == b"DO_NOT_LEAK")
        );
    }

    #[test]
    fn request_body_decodes_plain_structured_and_sensitive_values() {
        let string = TypeDescriptor::string();
        assert_eq!(
            decode_request_body(br#""plain""#, &string, body_limits(7)),
            Ok(SlotValue::Value(ContractValue::string("plain")))
        );
        let structure = TypeDescriptor::structure([
            FieldDescriptor::new("name", string.clone(), None),
            FieldDescriptor::new("active", TypeDescriptor::bool(), None),
        ])
        .unwrap();
        let expected = ContractValue::object([
            ("name".into(), ContractValue::string("Ada")),
            ("active".into(), ContractValue::bool(true)),
        ])
        .unwrap();
        let structured = br#"{"name":"Ada","active":true}"#;
        assert_eq!(
            decode_request_body(structured, &structure, body_limits(structured.len())),
            Ok(SlotValue::Value(expected))
        );
        let secret = TypeDescriptor::secret(string).unwrap();
        assert_eq!(
            decode_request_body(br#""DO_NOT_LEAK""#, &secret, body_limits(13)),
            Ok(SlotValue::Value(ContractValue::sensitive(
                ContractValue::string("DO_NOT_LEAK")
            )))
        );
    }

    #[test]
    fn request_body_enforces_byte_limit_before_payload_inspection() {
        let body = br#""okay""#;
        assert!(
            decode_request_body(body, &TypeDescriptor::string(), body_limits(body.len())).is_ok()
        );
        assert_body_error(
            body,
            &TypeDescriptor::string(),
            body_limits(body.len() - 1),
            WireCallError::PayloadTooLarge,
        );
        assert_body_error(
            b"{DO_NOT_LEAK",
            &TypeDescriptor::string(),
            body_limits(1),
            WireCallError::PayloadTooLarge,
        );
    }

    #[test]
    fn request_body_maps_syntax_failures_to_canonical_bad_request() {
        let string = TypeDescriptor::string();
        for body in [
            b"".as_slice(),
            &[0xff],
            b"DO_NOT_LEAK",
            br#""value" trailing"#,
        ] {
            assert_body_error(
                body,
                &string,
                body_limits(body.len()),
                WireCallError::InvalidRequest,
            );
        }
        assert_body_error(
            b"[[]]",
            &TypeDescriptor::list(TypeDescriptor::list(string).unwrap()).unwrap(),
            SyntaxLimits(4, 1),
            WireCallError::InvalidRequest,
        );
    }

    #[test]
    fn request_body_maps_provider_semantic_failures_to_canonical_bad_request() {
        let map = TypeDescriptor::map(TypeDescriptor::string()).unwrap();
        let structure = TypeDescriptor::structure([FieldDescriptor::new(
            "known",
            TypeDescriptor::string(),
            None,
        )])
        .unwrap();
        let enumeration = TypeDescriptor::enumeration([VariantDescriptor::new(
            "known",
            VariantPayload::Unit,
            None,
        )])
        .unwrap();
        for (body, descriptor) in [
            (br#"{"DO_NOT_LEAK":"a","DO_NOT_LEAK":"b"}"#.as_slice(), &map),
            (br#"{"DO_NOT_LEAK":"value"}"#, &structure),
            (br#"{"tag":"DO_NOT_LEAK","payload":null}"#, &enumeration),
            (br#""01""#, &TypeDescriptor::i64()),
        ] {
            assert_body_error(
                body,
                descriptor,
                body_limits(body.len()),
                WireCallError::InvalidRequest,
            );
        }
    }

    #[test]
    fn exact_route_returns_the_selected_exposure_and_runtime_seam_exists() {
        fn actual_runtime_exposure_uses_view<T: ExposureView>() {}
        actual_runtime_exposure_uses_view::<TransportExposure>();
        let exposures = [
            exposure("alpha", "read", ExposureLevel::Internal),
            exposure("alpha", "write", ExposureLevel::External),
            exposure("beta", "read", ExposureLevel::CodeOnly),
        ];
        let selected = resolve_route("/rpc/alpha/write", false, &exposures).unwrap();
        assert_eq!(selected.descriptor().id().to_string(), "alpha.write");
        assert_eq!(selected.level(), ExposureLevel::External);
        assert_eq!(
            resolve_route("/rpc/beta/read", false, &exposures)
                .unwrap()
                .level(),
            ExposureLevel::CodeOnly
        );
    }

    #[test]
    fn malformed_and_unknown_routes_have_canonical_distinct_errors() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        for path in [
            "",
            "/",
            "/RPC/known/call",
            "/rpc",
            "/rpc/",
            "/rpc//call",
            "/rpc/Known/call",
            "/rpc/known%2fother/call",
            "/rpc/ghost/call",
            "/rpc_ignored/known/call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownBox),
                "{path}"
            );
        }
        for path in [
            "/rpc/known",
            "/rpc/known/",
            "/rpc/known/Call",
            "/rpc/known/call%20",
            "/rpc/known/ghost",
            "/rpc/known/call/",
            "/rpc/known/call/extra",
            "/rpc/known//call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownCapability),
                "{path}"
            );
        }
        let box_error = WireCallError::UnknownBox.encode();
        let capability_error = WireCallError::UnknownCapability.encode();
        assert_eq!(box_error.status(), 404);
        assert_eq!(capability_error.status(), 404);
        assert_ne!(box_error.body(), capability_error.body());
        assert_eq!(
            box_error.body(),
            br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#
        );
        assert_eq!(capability_error.body(), br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#);
    }

    #[test]
    fn route_precedes_query_validation() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        assert_eq!(
            resolve_route("/rpc/known/call", true, &exposures),
            Err(WireCallError::InvalidRequest)
        );
        assert_eq!(
            resolve_route("/rpc/known/ghost", true, &exposures),
            Err(WireCallError::UnknownCapability)
        );
        assert_eq!(
            resolve_route("/rpc/ghost/call", true, &exposures),
            Err(WireCallError::UnknownBox)
        );
    }

    fn json_headers() -> HeaderMap {
        headers("content-type", b"application/json")
    }

    fn assert_admission_error(
        path: &str,
        query_present: bool,
        method: Method,
        headers: &HeaderMap,
        exposures: &[Exposure],
        expected: WireCallError,
    ) {
        let error = match admit_request_head(path, query_present, &method, headers, exposures) {
            Ok(_) => panic!("request head unexpectedly admitted"),
            Err(error) => error,
        };
        assert_eq!(error, expected);
        let encoded = error.encode();
        let (status, code, message) = expected.spec();
        assert_eq!(encoded.status(), status);
        assert_eq!(
            encoded.body(),
            format!(r#"{{"error":{{"kind":"call","code":"{code}","message":"{message}"}}}}"#)
                .as_bytes()
        );
    }

    #[test]
    fn admission_preserves_selected_exposure_and_exact_context_values() {
        let exposures = [
            exposure("alpha", "read", ExposureLevel::Internal),
            exposure("alpha", "write", ExposureLevel::External),
        ];
        let mut headers = headers("content-type", b"Application/JSON ; Charset=\"ut\\f-8\"");
        headers.insert(TIMEOUT_HEADER, HeaderValue::from_static("42"));
        headers.insert(IDEMPOTENCY_HEADER, HeaderValue::from_static("request-7"));
        headers.insert(TRACEPARENT_HEADER, HeaderValue::from_static(PARENT));
        headers.append(TRACESTATE_HEADER, HeaderValue::from_static("one=1"));
        headers.append(TRACESTATE_HEADER, HeaderValue::from_static("two=2"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/plain"));
        headers.insert("x-forwarded-for", HeaderValue::from_static("ignored"));

        let admitted = admit_request_head(
            "/rpc/alpha/write",
            false,
            &Method::POST,
            &headers,
            &exposures,
        )
        .unwrap();
        assert_eq!(
            admitted.exposure.descriptor().id().to_string(),
            "alpha.write"
        );
        assert_eq!(admitted.exposure.level(), ExposureLevel::External);
        assert_eq!(admitted.timeout, Some(Duration::from_millis(42)));
        assert_eq!(
            admitted
                .idempotency_key
                .as_ref()
                .map(IdempotencyKey::as_str),
            Some("request-7")
        );
        assert_eq!(admitted.trace_context.traceparent(), Some(PARENT));
        assert_eq!(admitted.trace_context.tracestate(), Some("one=1,two=2"));
    }

    #[test]
    fn admission_enforces_route_then_query_then_method_precedence() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        let bad_headers = headers(TIMEOUT_HEADER, b"bad");
        for (path, query, expected) in [
            ("/rpc/ghost/call", false, WireCallError::UnknownBox),
            ("/rpc/known/ghost", false, WireCallError::UnknownCapability),
            ("/rpc/known/call", true, WireCallError::InvalidRequest),
        ] {
            assert_admission_error(path, query, Method::GET, &bad_headers, &exposures, expected);
        }
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert_admission_error(
                "/rpc/known/call",
                false,
                method,
                &bad_headers,
                &exposures,
                WireCallError::MethodNotAllowed,
            );
        }
    }

    #[test]
    fn admission_enforces_media_before_contractual_headers() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        let mut wrong = headers("content-type", b"text/plain");
        wrong.insert(TIMEOUT_HEADER, HeaderValue::from_static("bad"));
        let mut encoded = json_headers();
        encoded.insert("content-encoding", HeaderValue::from_static("identity"));
        encoded.insert(TIMEOUT_HEADER, HeaderValue::from_static("bad"));
        for media in [HeaderMap::new(), wrong, encoded] {
            assert_admission_error(
                "/rpc/known/call",
                false,
                Method::POST,
                &media,
                &exposures,
                WireCallError::UnsupportedMediaType,
            );
        }

        let mut duplicate = json_headers();
        duplicate.append("content-type", HeaderValue::from_static("application/json"));
        let comma_joined = headers("content-type", b"application/json, application/json");
        let trailing_comma = headers("content-type", b"application/json, \t");
        for invalid in [duplicate, comma_joined, trailing_comma] {
            assert_admission_error(
                "/rpc/known/call",
                false,
                Method::POST,
                &invalid,
                &exposures,
                WireCallError::InvalidRequest,
            );
        }
    }

    #[test]
    fn admission_accepts_only_the_declared_json_media_forms() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        for raw in [
            b"application/json".as_slice(),
            b"APPLICATION/JSON",
            b" application/json ",
            b"application/json;charset=utf-8",
            b"application/json ; CHARSET=\"UTF-8\" ",
            b"application/json; charset=\"ut\\f-8\"",
        ] {
            let headers = headers("content-type", raw);
            assert!(
                admit_request_head(
                    "/rpc/known/call",
                    false,
                    &Method::POST,
                    &headers,
                    &exposures
                )
                .is_ok(),
                "rejected {raw:?}"
            );
        }
        for raw in [
            b"application/json; charset=ascii".as_slice(),
            b"application/json; charset =utf-8",
            b"application/json; charset= utf-8",
            b"application/json; charset=utf-8; version=1",
            b"application/json; charset=utf-8; charset=utf-8",
            b"application/json; boundary=x",
            b"application/json; note=\"a,b\"",
            b"application/json;",
            b"application/problem+json",
        ] {
            assert_admission_error(
                "/rpc/known/call",
                false,
                Method::POST,
                &headers("content-type", raw),
                &exposures,
                WireCallError::UnsupportedMediaType,
            );
        }
    }

    #[test]
    fn admission_validates_fallible_headers_before_non_failing_trace_context() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        let mut bad_timeout = json_headers();
        bad_timeout.insert(TIMEOUT_HEADER, HeaderValue::from_static("bad"));
        bad_timeout.insert(IDEMPOTENCY_HEADER, HeaderValue::from_static("also bad"));
        assert_admission_error(
            "/rpc/known/call",
            false,
            Method::POST,
            &bad_timeout,
            &exposures,
            WireCallError::InvalidRequest,
        );

        let mut bad_idempotency = json_headers();
        bad_idempotency.insert(TIMEOUT_HEADER, HeaderValue::from_static("7"));
        bad_idempotency.insert(IDEMPOTENCY_HEADER, HeaderValue::from_static("bad key"));
        assert_admission_error(
            "/rpc/known/call",
            false,
            Method::POST,
            &bad_idempotency,
            &exposures,
            WireCallError::InvalidRequest,
        );

        let mut bad_trace = json_headers();
        bad_trace.insert(TRACEPARENT_HEADER, HeaderValue::from_static("malformed"));
        bad_trace.insert(
            TRACESTATE_HEADER,
            HeaderValue::from_static("also malformed"),
        );
        let admitted = admit_request_head(
            "/rpc/known/call",
            false,
            &Method::POST,
            &bad_trace,
            &exposures,
        )
        .unwrap();
        assert_eq!(admitted.trace_context.traceparent(), None);
        assert_eq!(admitted.trace_context.tracestate(), None);
    }

    #[test]
    fn rejects_every_non_unary_shape_before_presence() {
        let tri = TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap();
        for shape in [
            CapabilityShape::ServerStreaming,
            CapabilityShape::ClientStreaming,
            CapabilityShape::BidirectionalStreaming,
            CapabilityShape::EventSubscription,
        ] {
            let error =
                conform_capability(&with_slots(shape, tri.clone(), tri.clone(), tri.clone()))
                    .unwrap_err();
            assert_eq!(error.code(), "http_non_unary");
        }
    }

    #[test]
    fn top_level_field_is_rejected_in_each_slot_with_stable_precedence() {
        let plain = || TypeDescriptor::string();
        let field = || TypeDescriptor::tri_state(plain()).unwrap();
        for (descriptor, slot) in [
            (
                with_slots(CapabilityShape::Unary, field(), plain(), plain()),
                "input",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), field(), plain()),
                "output",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), plain(), field()),
                "error",
            ),
        ] {
            let error = conform_capability(&descriptor).unwrap_err();
            assert_eq!(error.code(), "http_top_level_field");
            assert_eq!(
                error.message(),
                Some(format!("HTTP cannot represent top-level Field in {slot}").as_str())
            );
        }
        let all = with_slots(CapabilityShape::Unary, field(), field(), field());
        assert_eq!(
            conform_capability(&all).unwrap_err().message(),
            Some("HTTP cannot represent top-level Field in input")
        );
    }

    #[test]
    fn secret_rejects_deep_presence_across_all_aggregate_kinds_without_leaking_names() {
        let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let enumeration = TypeDescriptor::enumeration([VariantDescriptor::new(
            "variant-sentinel",
            VariantPayload::Value(optional),
            None,
        )])
        .unwrap();
        let nested = TypeDescriptor::structure([FieldDescriptor::new(
            "payload-sentinel",
            TypeDescriptor::list(TypeDescriptor::map(enumeration).unwrap()).unwrap(),
            None,
        )])
        .unwrap();
        let error = conform_capability(&with_slots(
            CapabilityShape::Unary,
            TypeDescriptor::string(),
            TypeDescriptor::secret(nested).unwrap(),
            TypeDescriptor::string(),
        ))
        .unwrap_err();
        assert_eq!(error.code(), "http_secret_presence");
        assert_eq!(
            error.message(),
            Some("HTTP cannot represent presence inside Secret")
        );
        assert!(!error.to_string().contains("sentinel"));

        let tri_in_struct = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        let error_slot = TypeDescriptor::secret(tri_in_struct).unwrap();
        assert_eq!(
            conform_capability(&with_slots(
                CapabilityShape::Unary,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
                error_slot
            ))
            .unwrap_err()
            .code(),
            "http_secret_presence"
        );
    }

    #[test]
    fn accepts_supported_presence_and_secret_shapes() {
        let object_field = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        for input in [
            TypeDescriptor::string(),
            TypeDescriptor::optional(TypeDescriptor::string()).unwrap(),
            object_field,
            TypeDescriptor::secret(TypeDescriptor::string()).unwrap(),
            TypeDescriptor::optional(TypeDescriptor::secret(TypeDescriptor::string()).unwrap())
                .unwrap(),
        ] {
            conform_capability(&with_slots(
                CapabilityShape::Unary,
                input,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
            ))
            .unwrap();
        }
    }

    fn headers(name: &'static str, value: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_bytes(value).unwrap());
        headers
    }

    #[test]
    fn timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers() {
        assert_eq!(parse_timeout(&HeaderMap::new()), Ok(None));
        for (raw, millis) in [
            (b"0".as_slice(), 0),
            (b"1", 1),
            (b"9999999999", 9_999_999_999),
        ] {
            assert_eq!(
                parse_timeout(&headers(TIMEOUT_HEADER, raw)),
                Ok(Some(Duration::from_millis(millis)))
            );
        }
        let mut mixed = headers("BoXoLoGy-TiMeOuT-Ms", b"7");
        mixed.insert(ACCEPT, HeaderValue::from_static("text/plain"));
        mixed.insert("x-proxy-note", HeaderValue::from_static("ignored"));
        assert_eq!(parse_timeout(&mixed), Ok(Some(Duration::from_millis(7))));
    }

    #[test]
    fn timeout_header_rejects_every_malformed_or_duplicate_form() {
        for raw in [
            b"".as_slice(),
            b"00",
            b"01",
            b"+1",
            b"-1",
            b" 1",
            b"1 ",
            b"10000000000",
            b"one",
            b"1,2",
            &[0x80],
        ] {
            assert_eq!(
                parse_timeout(&headers(TIMEOUT_HEADER, raw)),
                Err(WireCallError::InvalidRequest)
            );
        }
        for duplicate in [b"1".as_slice(), b"2"] {
            let mut values = headers(TIMEOUT_HEADER, b"1");
            values.append(TIMEOUT_HEADER, HeaderValue::from_bytes(duplicate).unwrap());
            assert_eq!(parse_timeout(&values), Err(WireCallError::InvalidRequest));
        }
    }

    #[test]
    fn idempotency_header_accepts_boundaries_and_preserves_only_the_key() {
        assert_eq!(parse_idempotency_key(&HeaderMap::new()), Ok(None));
        let boundary = vec![b'x'; 256];
        for raw in [b"!".as_slice(), b"~", boundary.as_slice()] {
            let parsed = parse_idempotency_key(&headers("IdEmPoTeNcY-KeY", raw))
                .unwrap()
                .unwrap();
            assert_eq!(parsed.as_str().as_bytes(), raw);
        }
    }

    #[test]
    fn idempotency_header_rejects_every_malformed_or_duplicate_form() {
        let too_long = vec![b'x'; 257];
        for raw in [
            b"".as_slice(),
            too_long.as_slice(),
            b"a b",
            b"a\tb",
            &[0x80],
            b"a,b",
        ] {
            assert_eq!(
                parse_idempotency_key(&headers(IDEMPOTENCY_HEADER, raw)),
                Err(WireCallError::InvalidRequest)
            );
        }
        // `http` refuses these before a `HeaderMap` can carry them. The parser's
        // visible-ASCII check independently excludes the same byte ranges.
        assert!(HeaderValue::from_bytes(&[0x1f]).is_err());
        assert!(HeaderValue::from_bytes(&[0x7f]).is_err());
        let mut duplicate = headers(IDEMPOTENCY_HEADER, b"same");
        duplicate.append(IDEMPOTENCY_HEADER, HeaderValue::from_static("same"));
        assert_eq!(
            parse_idempotency_key(&duplicate),
            Err(WireCallError::InvalidRequest)
        );
    }

    const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    fn trace_headers(parent: Option<&str>, states: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(parent) = parent {
            headers.insert(TRACEPARENT_HEADER, HeaderValue::from_str(parent).unwrap());
        }
        for state in states {
            headers.append(TRACESTATE_HEADER, HeaderValue::from_str(state).unwrap());
        }
        headers
    }

    #[test]
    fn traceparent_accepts_level_one_and_future_prefixes_opaquely() {
        for parent in [
            PARENT.to_owned(),
            PARENT.replacen("00-", "01-", 1),
            format!("{}-future-opaque", PARENT.replacen("00-", "fe-", 1)),
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(&parent), &[]));
            assert_eq!(parsed.traceparent(), Some(parent.as_str()));
            assert_eq!(parsed.tracestate(), None);
        }
    }

    #[test]
    fn malformed_or_duplicate_traceparent_drops_the_entire_context() {
        for parent in [
            "",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4BF92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00F067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0A",
            "00-4bf92f3577b34da6a3ce929d0e0e4736_00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01x",
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(parent), &["vendor=state"]));
            assert_eq!(parsed.traceparent(), None, "accepted {parent:?}");
            assert_eq!(parsed.tracestate(), None);
        }

        let mut duplicate = trace_headers(Some(PARENT), &["vendor=state"]);
        duplicate.append(TRACEPARENT_HEADER, HeaderValue::from_static(PARENT));
        let parsed = parse_trace_context(&duplicate);
        assert_eq!(parsed.traceparent(), None);
        assert_eq!(parsed.tracestate(), None);
    }

    #[test]
    fn tracestate_accepts_level_one_grammar_and_preserves_combined_bytes() {
        for state in [
            "vendor=value",
            "tenant-1@system=value",
            " ,\t, vendor=value , ",
            "",
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(PARENT), &[state]));
            assert_eq!(parsed.tracestate(), Some(state));
        }

        let parsed = parse_trace_context(&trace_headers(Some(PARENT), &["rojo=one", "congo=two"]));
        assert_eq!(parsed.tracestate(), Some("rojo=one,congo=two"));

        let members = (0..32)
            .map(|index| format!("k{index}=v"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&members])).tracestate(),
            Some(members.as_str())
        );
        let max_value = format!("vendor={}", "v".repeat(256));
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&max_value])).tracestate(),
            Some(max_value.as_str())
        );
        let max_combined = format!("a={},b={}", "v".repeat(256), "w".repeat(251));
        assert_eq!(max_combined.len(), 512);
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&max_combined])).tracestate(),
            Some(max_combined.as_str())
        );
    }

    #[test]
    fn invalid_tracestate_drops_only_state() {
        let too_many = (0..33)
            .map(|index| format!("k{index}=v"))
            .collect::<Vec<_>>()
            .join(",");
        let too_large = format!("a={},b={}", "v".repeat(256), "w".repeat(252));
        assert_eq!(too_large.len(), 513);
        for state in [
            "Vendor=value",
            "1vendor=value",
            "tenant@System=value",
            "vendor",
            "vendor=",
            "vendor=one,vendor=two",
            "vendor=bad=value",
            "vendor=bad\tvalue",
            "vendor=trailing ",
            too_many.as_str(),
            too_large.as_str(),
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(PARENT), &[state]));
            assert_eq!(
                parsed.traceparent(),
                Some(PARENT),
                "lost parent for {state:?}"
            );
            assert_eq!(parsed.tracestate(), None, "accepted {state:?}");
        }
        let orphan = parse_trace_context(&trace_headers(None, &["vendor=value"]));
        assert_eq!(orphan.traceparent(), None);
        assert_eq!(orphan.tracestate(), None);
    }
}

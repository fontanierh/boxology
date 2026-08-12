use serde::{Deserialize, Serialize};
#[rustfmt::skip]
use std::{collections::BTreeSet, fs::{self, File, OpenOptions}, io::{ErrorKind, Read, Seek, SeekFrom, Write}, os::unix::fs::OpenOptionsExt, path::{Path, PathBuf}, sync::Mutex, time::Duration};

const MAX_EVENTS: usize = 4096;
const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_RECORD: usize = 256 * 1024;

#[rustfmt::skip]
boxology::contract! {
    pub struct LoadRequest { pub session_id: String }
    pub enum SessionEventKind { User, Assistant, ToolCall, ToolResult }
    pub struct NewSessionEvent { pub event_id: String, pub kind: SessionEventKind, pub payload_json: String }
    pub struct AppendRequest { pub session_id: String, pub expected_sequence: u64, pub event: NewSessionEvent }
    pub struct SessionEvent { pub sequence: u64, pub event_id: String, pub kind: SessionEventKind, pub payload_json: String }
    pub struct LoadResult { pub events: Vec<SessionEvent>, pub next_sequence: u64 }
    pub struct AppendResult { pub sequence: u64, pub appended: bool }
    pub enum SessionFailureClass { Input, Boundary, Conflict, Resource, Corrupt, Local, Cancelled, Deadline }
    pub struct SessionFailure { pub class: SessionFailureClass, pub code: String, pub message: String, pub retryable: bool, pub side_effect_possible: bool }
    pub struct LoadOutcome { pub result: Option<LoadResult>, pub failure: Option<SessionFailure> }
    pub struct AppendOutcome { pub result: Option<AppendResult>, pub failure: Option<SessionFailure> }
    #[error] pub enum SessionError { Internal }
    #[capability] pub async fn load(request: LoadRequest) -> Result<LoadOutcome, SessionError>;
    #[capability] pub async fn append(request: AppendRequest) -> Result<AppendOutcome, SessionError>;
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
struct Record { schema: u64, version: u64, sequence: u64, event_id: String, kind: String, payload_json: String }
#[rustfmt::skip]
struct Log { events: Vec<SessionEvent>, committed: u64, torn: bool, exists: bool }

#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
#[rustfmt::skip]
enum Fault { TornWrite, PreSync, FileSync, ParentSync, PreWriteCancel }

/// One-process, mutex-serialized, root-confined linear JSONL session store.
#[rustfmt::skip]
pub struct SessionStoreService { root: PathBuf, serial: Mutex<()>, #[cfg(test)] fault: Option<Fault> }

#[rustfmt::skip]
impl SessionStoreService {
    /// The root must be an existing canonical directory with non-symlink spelling.
    pub fn new(root: PathBuf) -> Result<Self, SessionFailure> {
        directory(&root, false)?;
        if fs::canonicalize(&root).map_err(|error| io(error, false))? != root { return Err(fail(SessionFailureClass::Boundary, "outside_root", false)); }
        Ok(Self { root, serial: Mutex::new(()), #[cfg(test)] fault: None })
    }

    fn path(&self, id: &str) -> Result<PathBuf, SessionFailure> {
        valid_id(id, 64).then(|| self.root.join(format!("{id}.jsonl"))).ok_or_else(|| input("session_id_invalid"))
    }

    fn read(&self, path: &Path) -> Result<Log, SessionFailure> {
        directory(&self.root, false)?;
        let metadata = match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Log { events: vec![], committed: 0, torn: false, exists: false }),
            Err(error) => return Err(io(error, false)), Ok(metadata) => metadata,
        };
        regular(&metadata, false)?;
        if metadata.len() > MAX_FILE as u64 { return Err(resource("session_too_large")); }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path).map_err(|error| io(error, false))?.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes).map_err(|error| io(error, false))?;
        if bytes.len() > MAX_FILE { return Err(resource("session_too_large")); }
        let torn = bytes.last().is_some_and(|byte| *byte != b'\n');
        let committed = if torn { bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |at| at + 1) } else { bytes.len() };
        if torn && bytes.len() - committed > MAX_RECORD { return Err(corrupt("record_too_large")); }
        let mut events = Vec::new(); let mut ids = BTreeSet::new();
        for (sequence, line) in bytes[..committed].strip_suffix(b"\n").unwrap_or(&[]).split(|byte| *byte == b'\n').enumerate() {
            if committed == 0 { break; }
            if line.is_empty() { return Err(corrupt("record_invalid")); }
            if line.len() + 1 > MAX_RECORD { return Err(corrupt("record_too_large")); }
            let record: Record = serde_json::from_slice(line).map_err(|_| corrupt("record_invalid"))?;
            let payload: serde_json::Value = serde_json::from_str(&record.payload_json).map_err(|_| corrupt("record_invalid"))?;
            let kind = decode_kind(&record.kind).ok_or_else(|| corrupt("record_invalid"))?;
            if record.schema != 1 || record.version != 1 || record.sequence != sequence as u64 || !valid_id(&record.event_id, 128) || !payload.is_object() || !ids.insert(record.event_id.clone()) { return Err(corrupt("record_invalid")); }
            if events.len() == MAX_EVENTS { return Err(corrupt("event_limit")); }
            events.push(SessionEvent { sequence: record.sequence, event_id: record.event_id, kind, payload_json: record.payload_json });
        }
        Ok(Log { events, committed: committed as u64, torn, exists: true })
    }

    fn append_inner(&self, context: &boxology::CallContext, request: AppendRequest) -> Result<AppendResult, SessionFailure> {
        check(context, false)?; let path = self.path(&request.session_id)?;
        if !valid_id(&request.event.event_id, 128) { return Err(input("event_id_invalid")); }
        let kind = encode_kind(&request.event.kind).ok_or_else(|| input("event_kind_invalid"))?;
        if request.event.payload_json.len() >= MAX_RECORD { return Err(resource("record_too_large")); }
        let payload: serde_json::Value = serde_json::from_str(&request.event.payload_json).map_err(|_| input("payload_invalid"))?;
        if !payload.is_object() { return Err(input("payload_invalid")); }
        let log = self.read(&path)?; let replay = log.events.iter().find(|event| event.event_id == request.event.event_id);
        if let Some(event) = replay { if event.sequence != request.expected_sequence || encode_kind(&event.kind) != Some(kind) || event.payload_json != request.event.payload_json { return Err(conflict("event_conflict")); } }
        else if request.expected_sequence != log.events.len() as u64 { return Err(conflict("sequence_conflict")); }
        if replay.is_none() && log.events.len() == MAX_EVENTS { return Err(resource("event_limit")); }
        let mut encoded = replay.is_none().then(|| serde_json::to_vec(&Record { schema: 1, version: 1, sequence: request.expected_sequence, event_id: request.event.event_id.clone(), kind: kind.into(), payload_json: request.event.payload_json.clone() }).expect("record serialization"));
        if let Some(bytes) = &mut encoded { bytes.push(b'\n'); if bytes.len() > MAX_RECORD { return Err(resource("record_too_large")); }
            if log.committed as usize + bytes.len() > MAX_FILE { return Err(resource("session_too_large")); } }
        check(context, false)?; directory(&self.root, false)?; let created = !log.exists;
        let mut file = if created { OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path) } else { OpenOptions::new().read(true).write(true).open(&path) }.map_err(|error| io(error, created))?;
        regular(&file.metadata().map_err(|error| io(error, created))?, created)?;
        #[cfg(test)] if self.fault == Some(Fault::PreWriteCancel) { context.cancellation().cancel(); }
        check(context, created)?;
        if log.torn { file.set_len(log.committed).map_err(|error| io(error, true))?; }
        file.seek(SeekFrom::End(0)).map_err(|error| io(error, log.torn || created))?;
        if let Some(bytes) = encoded {
            #[cfg(test)] if self.fault == Some(Fault::TornWrite) { file.write_all(&bytes[..bytes.len()/2]).map_err(|error| io(error, true))?; return Err(local(true)); }
            file.write_all(&bytes).map_err(|error| io(error, true))?;
            #[cfg(test)] if self.fault == Some(Fault::PreSync) { return Err(local(true)); }
        }
        #[cfg(test)] if self.fault == Some(Fault::FileSync) { return Err(local(log.torn || created || replay.is_none())); }
        file.sync_all().map_err(|error| io(error, log.torn || created || replay.is_none()))?;
        if created {
            #[cfg(test)] if self.fault == Some(Fault::ParentSync) { return Err(local(true)); }
            File::open(&self.root).and_then(|directory| directory.sync_all()).map_err(|error| io(error, true))?;
        }
        Ok(AppendResult { sequence: request.expected_sequence, appended: replay.is_none() })
    }
}

#[boxology::implementation]
#[rustfmt::skip]
impl SessionStoreService {
    pub async fn load(&self, context: boxology::CallContext, request: LoadRequest) -> Result<LoadOutcome, SessionError> {
        let result = (|| { check(&context, false)?; let _guard = self.serial.lock().map_err(|_| local(false))?; check(&context, false)?; let log = self.read(&self.path(&request.session_id)?)?; Ok(LoadResult { next_sequence: log.events.len() as u64, events: log.events }) })();
        Ok(match result { Ok(result) => LoadOutcome { result: Some(result), failure: None }, Err(failure) => LoadOutcome { result: None, failure: Some(failure) } })
    }
    pub async fn append(&self, context: boxology::CallContext, request: AppendRequest) -> Result<AppendOutcome, SessionError> {
        let result = self.serial.lock().map_err(|_| local(false)).and_then(|_guard| self.append_inner(&context, request));
        Ok(match result { Ok(result) => AppendOutcome { result: Some(result), failure: None }, Err(failure) => AppendOutcome { result: None, failure: Some(failure) } })
    }
}

#[rustfmt::skip] fn valid_id(value: &str, max: usize) -> bool { !value.is_empty() && value.len() <= max && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')) }
#[rustfmt::skip] fn encode_kind(kind: &SessionEventKind) -> Option<&'static str> { match kind { SessionEventKind::User => Some("user"), SessionEventKind::Assistant => Some("assistant"), SessionEventKind::ToolCall => Some("tool_call"), SessionEventKind::ToolResult => Some("tool_result"), SessionEventKind::Unknown { .. } => None } }
#[rustfmt::skip] fn decode_kind(kind: &str) -> Option<SessionEventKind> { match kind { "user" => Some(SessionEventKind::User), "assistant" => Some(SessionEventKind::Assistant), "tool_call" => Some(SessionEventKind::ToolCall), "tool_result" => Some(SessionEventKind::ToolResult), _ => None } }
#[rustfmt::skip] fn directory(path: &Path, side: bool) -> Result<(), SessionFailure> { let metadata = fs::symlink_metadata(path).map_err(|error| io(error, side))?; if metadata.file_type().is_symlink() { Err(fail(SessionFailureClass::Boundary, "symlink", side)) } else if !metadata.is_dir() { Err(fail(SessionFailureClass::Boundary, "not_directory", side)) } else { Ok(()) } }
#[rustfmt::skip] fn regular(metadata: &fs::Metadata, side: bool) -> Result<(), SessionFailure> { if metadata.file_type().is_symlink() { Err(fail(SessionFailureClass::Boundary, "symlink", side)) } else if !metadata.is_file() { Err(fail(SessionFailureClass::Boundary, "not_file", side)) } else { Ok(()) } }
#[rustfmt::skip] fn check(context: &boxology::CallContext, side: bool) -> Result<(), SessionFailure> { if context.cancellation().is_cancelled() { Err(fail(SessionFailureClass::Cancelled, "cancelled", side)) } else if context.deadline().is_some_and(|deadline| deadline.remaining() == Duration::ZERO) { Err(fail(SessionFailureClass::Deadline, "deadline_exceeded", side)) } else { Ok(()) } }
#[rustfmt::skip] fn io(_: std::io::Error, side: bool) -> SessionFailure { local(side) }
#[rustfmt::skip] fn fail(class: SessionFailureClass, code: &str, side_effect_possible: bool) -> SessionFailure { SessionFailure { class, code: code.into(), message: code.replace('_', " "), retryable: false, side_effect_possible } }
#[rustfmt::skip] fn input(code: &str) -> SessionFailure { fail(SessionFailureClass::Input, code, false) }
#[rustfmt::skip] fn conflict(code: &str) -> SessionFailure { fail(SessionFailureClass::Conflict, code, false) }
#[rustfmt::skip] fn resource(code: &str) -> SessionFailure { fail(SessionFailureClass::Resource, code, false) }
#[rustfmt::skip] fn corrupt(code: &str) -> SessionFailure { fail(SessionFailureClass::Corrupt, code, false) }
#[rustfmt::skip] fn local(side: bool) -> SessionFailure { fail(SessionFailureClass::Local, "local_io", side) }

pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}
#[cfg(test)]
mod tests;

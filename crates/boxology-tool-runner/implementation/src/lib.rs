use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const LIMIT: usize = 256 * 1024;
static STAGE: AtomicU64 = AtomicU64::new(0);

#[rustfmt::skip]
boxology::contract! {
    pub struct ReadRequest { pub path: String }
    pub struct WriteRequest { pub path: String, pub content: String }
    pub struct ExecuteRequest { pub read: Option<ReadRequest>, pub write: Option<WriteRequest> }
    pub enum FileOperation { Read, Write }
    pub struct FileResult { pub operation: FileOperation, pub path: String, pub content: Option<String>, pub bytes: u64, pub changed: bool }
    pub struct ExecuteResult { pub file: Option<FileResult> }
    pub enum ToolFailureClass { Input, Boundary, Missing, Conflict, Resource, Local, Cancelled, Deadline }
    pub struct ToolFailure { pub class: ToolFailureClass, pub code: String, pub message: String, pub retryable: bool, pub side_effect_possible: bool }
    pub struct ExecuteOutcome { pub result: Option<ExecuteResult>, pub failure: Option<ToolFailure> }
    #[error]
    pub enum ExecuteError { Internal }
    #[capability]
    pub async fn execute(request: ExecuteRequest) -> Result<ExecuteOutcome, ExecuteError>;
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
#[rustfmt::skip]
enum Fault { StagedCancel, StagedCleanup, CleanupSync, PreRenameCancel, Rename, ParentSync }

/// Root-confined UTF-8 operations: functional confinement, not protection from external races, hard links, or multiple service instances.
pub struct ToolRunnerService {
    root: PathBuf,
    mutation: Mutex<()>,
    #[cfg(test)]
    fault: Option<Fault>,
}

#[rustfmt::skip]
impl ToolRunnerService {
    /// Uses an existing directory only when supplied in canonical, non-symlink spelling.
    pub fn new(root: PathBuf) -> Result<Self, ToolFailure> {
        let metadata = fs::symlink_metadata(&root).map_err(|error| io(error, false))?;
        if metadata.file_type().is_symlink() {
            return Err(fail(ToolFailureClass::Boundary, "symlink", false));
        }
        if !metadata.is_dir() {
            return Err(fail(ToolFailureClass::Boundary, "not_directory", false));
        }
        let canonical = fs::canonicalize(&root).map_err(|error| io(error, false))?;
        if canonical != root {
            return Err(fail(ToolFailureClass::Boundary, "outside_root", false));
        }
        Ok(Self { root, mutation: Mutex::new(()), #[cfg(test)] fault: None })
    }

    fn resolve(&self, raw: &str) -> Result<(PathBuf, String), ToolFailure> {
        if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') || Path::new(raw).is_absolute()
        {
            return Err(fail(ToolFailureClass::Input, "path_invalid", false));
        }
        let parts = raw.split('/').collect::<Vec<_>>();
        if parts.iter().any(|part| part.is_empty() || *part == "." || *part == "..")
            || !Path::new(raw).components().all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(fail(ToolFailureClass::Boundary, "outside_root", false));
        }
        Ok((self.root.join(raw), parts.join("/")))
    }

    fn read_file(&self, context: &boxology::CallContext, path: &Path,
        side_effect: bool) -> Result<Vec<u8>, ToolFailure> {
        check(context, side_effect)?;
        self.file_metadata(path, side_effect)?;
        let mut file = File::open(path).map_err(|error| io(error, side_effect))?;
        let before = file.metadata().map_err(|error| io(error, side_effect))?.len();
        if before > LIMIT as u64 {
            return Err(fail(ToolFailureClass::Resource, "file_too_large", side_effect));
        }
        let mut bytes = Vec::with_capacity(before as usize);
        let mut chunk = [0_u8; 8192];
        loop {
            check(context, side_effect)?;
            let count = file.read(&mut chunk).map_err(|error| io(error, side_effect))?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > LIMIT {
                return Err(fail(ToolFailureClass::Resource, "file_too_large", side_effect));
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let after = file.metadata().map_err(|error| io(error, side_effect))?.len();
        if after > LIMIT as u64 {
            return Err(fail(ToolFailureClass::Resource, "file_too_large", side_effect));
        }
        if before != after || after != bytes.len() as u64 {
            return Err(fail(ToolFailureClass::Local, "local_io", side_effect));
        }
        Ok(bytes)
    }

    fn file_metadata(&self, target: &Path, side_effect: bool) -> Result<fs::Metadata, ToolFailure> {
        regular_dir(&self.root, side_effect)?;
        let relative = target.strip_prefix(&self.root).expect("resolved path");
        let count = relative.components().count();
        let mut current = self.root.clone();
        for (index, component) in relative.components().enumerate() {
            current.push(component);
            let metadata = fs::symlink_metadata(&current).map_err(|error| io(error, side_effect))?;
            if metadata.file_type().is_symlink() {
                return Err(fail(ToolFailureClass::Boundary, "symlink", side_effect));
            }
            if index + 1 == count {
                regular_meta(&metadata, side_effect)?;
                return Ok(metadata);
            }
            if !metadata.is_dir() {
                return Err(fail(ToolFailureClass::Boundary, "not_directory", side_effect));
            }
        }
        Err(fail(ToolFailureClass::Input, "path_invalid", side_effect))
    }

    fn prepare_parent(&self, context: &boxology::CallContext, target: &Path,
        side_effect: &mut bool) -> Result<(), ToolFailure> {
        regular_dir(&self.root, *side_effect)?;
        let relative = target.strip_prefix(&self.root).expect("resolved path");
        let mut current = self.root.clone();
        let count = relative.components().count();
        for component in relative.components().take(count - 1) {
            check(context, *side_effect)?;
            let next = current.join(component);
            match fs::symlink_metadata(&next) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(fail(ToolFailureClass::Boundary, "symlink", *side_effect));
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(fail(ToolFailureClass::Boundary, "not_directory", *side_effect)),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    regular_dir(&current, *side_effect)?;
                    fs::create_dir(&next).map_err(|error| io(error, *side_effect))?;
                    *side_effect = true;
                    regular_dir(&next, true)?;
                    #[cfg(test)]
                    if self.fault == Some(Fault::ParentSync) {
                        return Err(fail(ToolFailureClass::Local, "local_io", true));
                    }
                    File::open(&current).and_then(|directory| directory.sync_all()).map_err(|error| io(error, true))?;
                }
                Err(error) => return Err(io(error, *side_effect)),
            }
            current = next;
        }
        Ok(())
    }

    fn replace(&self, context: &boxology::CallContext, target: &Path, bytes: &[u8],
        expected: Option<&[u8]>, permissions: Option<Permissions>,
        side_effect: bool) -> Result<(), ToolFailure> {
        check(context, side_effect)?;
        let parent = target.parent().expect("resolved file has parent");
        let mut stage = None;
        let mut file = None;
        for _ in 0..32 {
            let candidate = parent.join(format!(".boxology-{}-{}.tmp", std::process::id(),
                STAGE.fetch_add(1, Ordering::Relaxed)));
            match OpenOptions::new().write(true).create_new(true).open(&candidate) {
                Ok(opened) => {
                    stage = Some(candidate);
                    file = Some(opened);
                    break;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io(error, side_effect)),
            }
        }
        let stage = stage.ok_or_else(|| fail(ToolFailureClass::Local, "local_io", side_effect))?;
        let result = (|| {
            let mut file = file.expect("stage opened");
            #[cfg(test)]
            if matches!(self.fault, Some(Fault::StagedCancel | Fault::StagedCleanup | Fault::CleanupSync)) { context.cancellation().cancel(); }
            check(context, side_effect)?;
            for chunk in bytes.chunks(8192) {
                check(context, side_effect)?;
                file.write_all(chunk).map_err(|error| io(error, side_effect))?;
            }
            if let Some(permissions) = permissions {
                file.set_permissions(permissions).map_err(|error| io(error, side_effect))?;
            }
            file.sync_all().map_err(|error| io(error, side_effect))?;
            check(context, side_effect)?;
            regular_dir(parent, side_effect)?;
            match expected {
                Some(expected) if self.read_file(context, target, side_effect)? != expected => {
                    return Err(fail(ToolFailureClass::Conflict, "local_io", side_effect));
                }
                None => match fs::symlink_metadata(target) {
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(fail(ToolFailureClass::Boundary, "symlink", side_effect));
                    }
                    Ok(_) | Err(_) => return Err(fail(ToolFailureClass::Conflict, "local_io", side_effect)),
                },
                _ => {}
            }
            #[cfg(test)]
            if self.fault == Some(Fault::PreRenameCancel) { context.cancellation().cancel(); }
            check(context, side_effect)?;
            #[cfg(test)]
            if self.fault == Some(Fault::Rename) { return Err(fail(ToolFailureClass::Local, "local_io", true)); }
            fs::rename(&stage, target).map_err(|error| io(error, true))?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io(error, true))
        })();
        if let Err(mut failure) = result {
            if !self.cleanup(&stage) { failure.side_effect_possible = true; }
            return Err(failure);
        }
        Ok(())
    }

    fn cleanup(&self, stage: &Path) -> bool {
        #[cfg(test)] if self.fault == Some(Fault::StagedCleanup) { return false; }
        if fs::remove_file(stage).is_err() { return false; }
        #[cfg(test)] if self.fault == Some(Fault::CleanupSync) { return false; }
        File::open(stage.parent().expect("stage has parent")).and_then(|directory| directory.sync_all()).is_ok()
    }

    fn write(&self, context: &boxology::CallContext,
        request: WriteRequest) -> Result<FileResult, ToolFailure> {
        if request.content.len() > LIMIT {
            return Err(fail(ToolFailureClass::Resource, "file_too_large", false));
        }
        let (target, path) = self.resolve(&request.path)?;
        let _mutation = self.mutation.lock()
            .map_err(|_| fail(ToolFailureClass::Local, "local_io", false))?;
        let mut side_effect = false;
        self.prepare_parent(context, &target, &mut side_effect)?;
        let existing = match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                regular_meta(&metadata, side_effect)?;
                let bytes = self.read_file(context, &target, side_effect)?;
                if bytes == request.content.as_bytes() {
                    return Ok(file(FileOperation::Write, path, None, bytes.len(), false));
                }
                Some((bytes, metadata.permissions()))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(io(error, side_effect)),
        };
        let (expected, permissions) = existing.as_ref().map_or((None, None), |(bytes, permissions)|
            (Some(bytes.as_slice()), Some(permissions.clone())));
        self.replace(context, &target, request.content.as_bytes(), expected, permissions, side_effect)?;
        Ok(file(FileOperation::Write, path, None, request.content.len(), true))
    }

}

#[boxology::implementation]
#[rustfmt::skip]
impl ToolRunnerService {
    pub async fn execute(
        &self,
        context: boxology::CallContext,
        request: ExecuteRequest,
    ) -> Result<ExecuteOutcome, ExecuteError> {
        let operation = match (request.read, request.write) {
            (Some(request), None) => self.resolve(&request.path).and_then(|(target, path)| {
                let bytes = self.read_file(&context, &target, false)?;
                let content = String::from_utf8(bytes)
                    .map_err(|_| fail(ToolFailureClass::Input, "not_utf8", false))?;
                let length = content.len();
                Ok(file(FileOperation::Read, path, Some(content), length, false))
            }),
            (None, Some(request)) => self.write(&context, request),
            _ => Err(fail(ToolFailureClass::Input, "request_invalid", false)),
        };
        Ok(match operation {
            Ok(file) => ExecuteOutcome { result: Some(ExecuteResult { file: Some(file) }), failure: None },
            Err(failure) => ExecuteOutcome { result: None, failure: Some(failure) },
        })
    }
}

#[rustfmt::skip]
fn regular_meta(metadata: &fs::Metadata, side_effect: bool) -> Result<(), ToolFailure> {
    if metadata.file_type().is_symlink() {
        Err(fail(ToolFailureClass::Boundary, "symlink", side_effect))
    } else if !metadata.is_file() {
        Err(fail(ToolFailureClass::Boundary, "not_file", side_effect))
    } else {
        Ok(())
    }
}
#[rustfmt::skip]
fn regular_dir(path: &Path, side_effect: bool) -> Result<(), ToolFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io(error, side_effect))?;
    if metadata.file_type().is_symlink() {
        Err(fail(ToolFailureClass::Boundary, "symlink", side_effect))
    } else if !metadata.is_dir() {
        Err(fail(ToolFailureClass::Boundary, "not_directory", side_effect))
    } else {
        Ok(())
    }
}
#[rustfmt::skip]
fn check(context: &boxology::CallContext, side_effect: bool) -> Result<(), ToolFailure> {
    if context.cancellation().is_cancelled() {
        Err(fail(ToolFailureClass::Cancelled, "cancelled", side_effect))
    } else if context.deadline().is_some_and(|deadline| deadline.remaining() == Duration::ZERO) {
        Err(fail(ToolFailureClass::Deadline, "deadline_exceeded", side_effect))
    } else {
        Ok(())
    }
}
#[rustfmt::skip]
fn io(error: std::io::Error, side_effect: bool) -> ToolFailure {
    let (class, code) = match error.kind() {
        ErrorKind::NotFound => (ToolFailureClass::Missing, "not_found"),
        _ => (ToolFailureClass::Local, "local_io"),
    };
    fail(class, code, side_effect)
}
#[rustfmt::skip]
fn fail(class: ToolFailureClass, code: &str, side_effect_possible: bool) -> ToolFailure {
    ToolFailure {
        class,
        code: code.into(),
        message: code.replace('_', " "),
        retryable: false,
        side_effect_possible,
    }
}
#[rustfmt::skip]
fn file(operation: FileOperation, path: String, content: Option<String>, bytes: usize, changed: bool) -> FileResult {
    FileResult { operation, path, content, bytes: bytes as u64, changed }
}

pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}

#[cfg(test)]
mod tests;

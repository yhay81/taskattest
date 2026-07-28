use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::PROGRESS_SCHEMA_VERSION;
use crate::RECEIPT_SCHEMA_VERSION;
use crate::VERSION;
use crate::discover::selected_checks;
use crate::error::TaskError;
use crate::git::GitContext;
use crate::hex::encode_lower;
use crate::model::{
    CheckDefinition, CheckExecution, CheckOutcome, DiscoveryReport, EnvironmentPolicy, Invocation,
    LogReference, ProgressEvent, ProgressState, Receipt, ReceiptOutcome, ReceiptPayload,
    RedactionPolicy, ToolIdentity,
};
use crate::source::identify_source;
use crate::source::sha256_bytes;
use crate::store::StateStore;

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const TERMINATION_GRACE_PERIOD: Duration = Duration::from_millis(500);
const SUMMARY_BYTES: usize = 4 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const SAFE_ENVIRONMENT: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "WINDIR",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TERM",
    "NO_COLOR",
    "CI",
];

#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

pub fn install_cancellation_handler(token: CancellationToken) -> Result<(), TaskError> {
    ctrlc::set_handler(move || token.cancel())
        .map_err(|error| TaskError::execution(format!("install Ctrl-C handler: {error}")))
}

pub fn run_checks(
    git: &GitContext,
    store: &StateStore,
    discovery: DiscoveryReport,
    invocation: Invocation,
    cancellation: &CancellationToken,
    mut on_progress: impl FnMut(&ProgressEvent),
) -> Result<Receipt, TaskError> {
    let started_unix_ms = unix_time_ms();
    let started = Instant::now();
    let checks = selected_checks(&discovery);
    let environment_names = forwarded_environment_names(&checks);
    let toolchains = collect_toolchains(&checks, &environment_names);
    let mut executions = Vec::new();
    let mut sequence = 0_u64;
    let mut stop = false;

    for check in checks {
        sequence += 1;
        on_progress(&ProgressEvent {
            schema_version: PROGRESS_SCHEMA_VERSION.to_owned(),
            sequence,
            unix_ms: unix_time_ms(),
            state: ProgressState::CheckStarted,
            check_id: Some(check.id.clone()),
            outcome: None,
            receipt_id: None,
        });

        let execution = if stop {
            skipped_execution(check)
        } else {
            execute_check(
                git,
                store,
                check,
                invocation.limits.max_runtime_ms_per_check,
                invocation.limits.max_log_bytes_per_check,
                cancellation,
            )?
        };
        let outcome = execution.outcome.clone();
        stop = invocation.fail_fast && !matches!(outcome, CheckOutcome::Passed);
        executions.push(execution);

        sequence += 1;
        on_progress(&ProgressEvent {
            schema_version: PROGRESS_SCHEMA_VERSION.to_owned(),
            sequence,
            unix_ms: unix_time_ms(),
            state: ProgressState::CheckFinished,
            check_id: executions
                .last()
                .map(|execution| execution.check.id.clone()),
            outcome: Some(outcome),
            receipt_id: None,
        });
    }

    let coverage_gaps = discovery.coverage_gaps.clone();
    let source_after = identify_source(git)?;
    let source_unchanged = source_after == discovery.source;
    let outcome = receipt_outcome(
        &executions,
        &coverage_gaps,
        cancellation.is_cancelled(),
        source_unchanged,
    );
    let completed_unix_ms = unix_time_ms();
    let non_hermetic_inputs = executions
        .iter()
        .flat_map(|execution| execution.check.non_hermetic_inputs.iter().cloned())
        .chain([
            "network access is not sandboxed".to_owned(),
            "filesystem access outside the workspace is not sandboxed".to_owned(),
            "tool executables are resolved from PATH".to_owned(),
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let payload = ReceiptPayload {
        schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
        taskattest_version: VERSION.to_owned(),
        source: discovery.source.clone(),
        source_after,
        source_unchanged,
        invocation,
        discovery,
        started_unix_ms,
        completed_unix_ms,
        duration_ms: elapsed_ms(started.elapsed()),
        outcome,
        checks: executions,
        coverage_gaps,
        toolchains,
        environment: EnvironmentPolicy {
            mode: "allowlist".to_owned(),
            forwarded_names: environment_names,
            values_recorded: false,
        },
        redaction: RedactionPolicy {
            name: "environment-minimized-summary-redaction-v1".to_owned(),
            full_logs_redacted: false,
            diagnostic_summaries_redacted: true,
        },
        non_hermetic_inputs,
        artifacts: Vec::new(),
        annotations: BTreeMap::new(),
    };
    let receipt = build_receipt(payload)?;
    store.write_receipt(&receipt)?;

    sequence += 1;
    on_progress(&ProgressEvent {
        schema_version: PROGRESS_SCHEMA_VERSION.to_owned(),
        sequence,
        unix_ms: unix_time_ms(),
        state: ProgressState::ReceiptStored,
        check_id: None,
        outcome: None,
        receipt_id: Some(receipt.receipt_id.clone()),
    });
    Ok(receipt)
}

pub fn build_receipt(payload: ReceiptPayload) -> Result<Receipt, TaskError> {
    let canonical = serde_json::to_vec(&payload)?;
    let canonical_digest = sha256_bytes(&canonical);
    let receipt_id = format!("rcpt_{}", &canonical_digest[..32]);
    Ok(Receipt {
        receipt_id,
        canonical_digest,
        payload,
    })
}

fn execute_check(
    git: &GitContext,
    store: &StateStore,
    check: CheckDefinition,
    max_runtime_ms: u64,
    max_log_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<CheckExecution, TaskError> {
    let started_unix_ms = unix_time_ms();
    let started = Instant::now();
    let working_directory = resolve_working_directory(&git.root, &check.command.working_directory)?;
    let (stdout_path, stdout_file) = store.create_capture_file("stdout")?;
    let (stderr_path, stderr_file) = match store.create_capture_file("stderr") {
        Ok(capture) => capture,
        Err(error) => {
            let _ = std::fs::remove_file(&stdout_path);
            return Err(error);
        }
    };
    let mut command = Command::new(&check.command.program);
    command
        .args(&check.command.args)
        .current_dir(&working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    forward_environment(&mut command, &check.pass_environment);
    configure_process_group(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Ok(CheckExecution {
                check,
                started_unix_ms,
                completed_unix_ms: unix_time_ms(),
                duration_ms: elapsed_ms(started.elapsed()),
                outcome: CheckOutcome::SpawnFailed,
                exit_code: None,
                stdout: None,
                stderr: None,
                stdout_summary: String::new(),
                stderr_summary: redact_summary(&error.to_string()),
                diagnostic_summaries_truncated: false,
            });
        }
    };
    let process_id = child.id();
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = terminate_process_tree(&mut child);
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Err(TaskError::execution("child stdout was not captured"));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = terminate_process_tree(&mut child);
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Err(TaskError::execution("child stderr was not captured"));
        }
    };
    let total_bytes = Arc::new(AtomicU64::new(0));
    let log_limit_exceeded = Arc::new(AtomicBool::new(false));
    let stdout_thread = capture_stream(
        stdout,
        stdout_path,
        stdout_file,
        Arc::clone(&total_bytes),
        max_log_bytes,
        Arc::clone(&log_limit_exceeded),
    );
    let stderr_thread = capture_stream(
        stderr,
        stderr_path,
        stderr_file,
        Arc::clone(&total_bytes),
        max_log_bytes,
        Arc::clone(&log_limit_exceeded),
    );

    let deadline = started + Duration::from_millis(max_runtime_ms);
    let mut forced_outcome = None;
    let status = loop {
        if cancellation.is_cancelled() {
            forced_outcome = Some(CheckOutcome::Cancelled);
            let status = terminate_process_tree(&mut child).map_err(|error| {
                TaskError::execution(format!("terminate cancelled check {}: {error}", check.id))
            })?;
            break Some(status);
        }
        if log_limit_exceeded.load(Ordering::SeqCst) {
            forced_outcome = Some(CheckOutcome::LogLimitExceeded);
            let status = terminate_process_tree(&mut child).map_err(|error| {
                TaskError::execution(format!("terminate log-limited check {}: {error}", check.id))
            })?;
            break Some(status);
        }
        if Instant::now() >= deadline {
            forced_outcome = Some(CheckOutcome::TimedOut);
            let status = terminate_process_tree(&mut child).map_err(|error| {
                TaskError::execution(format!("terminate timed-out check {}: {error}", check.id))
            })?;
            break Some(status);
        }
        if let Some(status) = try_wait_without_interruption(&mut child)
            .map_err(|error| TaskError::execution(format!("wait for {}: {error}", check.id)))?
        {
            terminate_orphaned_process_group(process_id);
            break Some(status);
        }
        thread::sleep(POLL_INTERVAL);
    };

    let stdout_capture = join_capture(stdout_thread, "stdout")?;
    let stderr_capture = join_capture(stderr_thread, "stderr")?;
    let stdout_reference = persist_capture(store, &stdout_capture)?;
    let stderr_reference = persist_capture(store, &stderr_capture)?;
    let outcome = forced_outcome.unwrap_or_else(|| {
        if total_bytes.load(Ordering::SeqCst) > max_log_bytes {
            CheckOutcome::LogLimitExceeded
        } else {
            match status {
                Some(status) if status.success() => CheckOutcome::Passed,
                Some(_) | None => CheckOutcome::Failed,
            }
        }
    });

    Ok(CheckExecution {
        check,
        started_unix_ms,
        completed_unix_ms: unix_time_ms(),
        duration_ms: elapsed_ms(started.elapsed()),
        outcome,
        exit_code: status.and_then(|status| status.code()),
        stdout: Some(stdout_reference),
        stderr: Some(stderr_reference),
        stdout_summary: redact_summary(&String::from_utf8_lossy(&stdout_capture.tail)),
        stderr_summary: redact_summary(&String::from_utf8_lossy(&stderr_capture.tail)),
        diagnostic_summaries_truncated: stdout_capture.truncated || stderr_capture.truncated,
    })
}

fn resolve_working_directory(root: &Path, relative: &str) -> Result<PathBuf, TaskError> {
    let path = root.join(relative).canonicalize().map_err(|error| {
        TaskError::execution(format!("resolve working directory {relative}: {error}"))
    })?;
    if !path.starts_with(root) || !path.is_dir() {
        return Err(TaskError::execution(format!(
            "working directory escapes the repository: {relative}"
        )));
    }
    Ok(path)
}

struct Capture {
    path: PathBuf,
    digest: String,
    bytes: u64,
    tail: Vec<u8>,
    truncated: bool,
}

fn capture_stream(
    mut reader: impl Read + Send + 'static,
    path: PathBuf,
    mut file: File,
    total_bytes: Arc<AtomicU64>,
    max_log_bytes: u64,
    limit_exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<std::io::Result<Capture>> {
    thread::spawn(move || {
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut tail = Vec::new();
        let mut truncated = false;
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
            bytes = bytes.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
            let combined =
                total_bytes.fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::SeqCst);
            if combined.saturating_add(u64::try_from(count).unwrap_or(u64::MAX)) > max_log_bytes {
                limit_exceeded.store(true, Ordering::SeqCst);
            }
            tail.extend_from_slice(&buffer[..count]);
            if tail.len() > SUMMARY_BYTES {
                truncated = true;
                let excess = tail.len() - SUMMARY_BYTES;
                tail.drain(..excess);
            }
        }
        file.sync_all()?;
        Ok(Capture {
            path,
            digest: encode_lower(hasher.finalize()),
            bytes,
            tail,
            truncated,
        })
    })
}

fn join_capture(
    handle: thread::JoinHandle<std::io::Result<Capture>>,
    stream: &str,
) -> Result<Capture, TaskError> {
    handle
        .join()
        .map_err(|_| TaskError::execution(format!("{stream} capture thread panicked")))?
        .map_err(|error| TaskError::execution(format!("capture {stream}: {error}")))
}

fn persist_capture(store: &StateStore, capture: &Capture) -> Result<LogReference, TaskError> {
    store.store_blob(&capture.path, &capture.digest, capture.bytes)?;
    Ok(LogReference {
        algorithm: "sha256".to_owned(),
        digest: capture.digest.clone(),
        bytes: capture.bytes,
        handle: format!("sha256:{}", capture.digest),
        content_encoding: "raw".to_owned(),
        sensitivity: "potentially_sensitive".to_owned(),
    })
}

fn skipped_execution(check: CheckDefinition) -> CheckExecution {
    let now = unix_time_ms();
    CheckExecution {
        check,
        started_unix_ms: now,
        completed_unix_ms: now,
        duration_ms: 0,
        outcome: CheckOutcome::Skipped,
        exit_code: None,
        stdout: None,
        stderr: None,
        stdout_summary: String::new(),
        stderr_summary: "skipped after an earlier failure because fail-fast was enabled".to_owned(),
        diagnostic_summaries_truncated: false,
    }
}

fn receipt_outcome(
    checks: &[CheckExecution],
    coverage_gaps: &[String],
    cancelled: bool,
    source_unchanged: bool,
) -> ReceiptOutcome {
    if cancelled
        || checks
            .iter()
            .any(|check| matches!(check.outcome, CheckOutcome::Cancelled))
    {
        return ReceiptOutcome::Cancelled;
    }
    if !source_unchanged
        || checks
            .iter()
            .any(|check| !matches!(check.outcome, CheckOutcome::Passed))
    {
        return ReceiptOutcome::Failed;
    }
    if checks.is_empty() || !coverage_gaps.is_empty() {
        return ReceiptOutcome::Incomplete;
    }
    ReceiptOutcome::Passed
}

fn forwarded_environment_names(checks: &[CheckDefinition]) -> Vec<String> {
    SAFE_ENVIRONMENT
        .iter()
        .map(|name| (*name).to_owned())
        .chain(
            checks
                .iter()
                .flat_map(|check| check.pass_environment.iter().cloned()),
        )
        .filter(|name| std::env::var_os(name).is_some())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn forward_environment(command: &mut Command, explicit: &[String]) {
    for name in SAFE_ENVIRONMENT
        .iter()
        .map(|value| (*value).to_owned())
        .chain(explicit.iter().cloned())
        .collect::<BTreeSet<_>>()
    {
        if let Some(value) = std::env::var_os(&name) {
            command.env(name, value);
        }
    }
}

fn collect_toolchains(
    checks: &[CheckDefinition],
    environment_names: &[String],
) -> Vec<ToolIdentity> {
    let programs = checks
        .iter()
        .map(|check| check.command.program.as_str())
        .collect::<BTreeSet<_>>();
    let mut commands = BTreeMap::from([("git", vec!["git", "--version"])]);
    if programs.contains("cargo") {
        commands.insert("cargo", vec!["cargo", "--version", "--verbose"]);
        commands.insert("rustc", vec!["rustc", "--version", "--verbose"]);
    }
    for manager in ["npm", "pnpm", "yarn", "bun"] {
        if programs.contains(manager) {
            commands.insert(manager, vec![manager, "--version"]);
        }
    }
    if programs.contains("python") {
        commands.insert("python", vec!["python", "--version"]);
    }
    if programs.contains("python3") {
        commands.insert("python3", vec!["python3", "--version"]);
    }
    if programs.contains("go") {
        commands.insert("go", vec!["go", "version"]);
    }
    commands
        .into_iter()
        .map(|(name, command)| tool_identity(name, &command, environment_names))
        .collect()
}

fn tool_identity(name: &str, command: &[&str], environment_names: &[String]) -> ToolIdentity {
    let mut process = Command::new(command[0]);
    process.args(&command[1..]).stdin(Stdio::null()).env_clear();
    for environment_name in environment_names {
        if let Some(value) = std::env::var_os(environment_name) {
            process.env(environment_name, value);
        }
    }
    match process.output() {
        Ok(output)
            if output.stdout.len().saturating_add(output.stderr.len()) <= MAX_TOOL_OUTPUT_BYTES =>
        {
            let mut evidence = output.stdout;
            evidence.extend_from_slice(&output.stderr);
            let summary = String::from_utf8_lossy(&evidence)
                .chars()
                .take(2_048)
                .collect::<String>();
            ToolIdentity {
                name: name.to_owned(),
                command: command.iter().map(|value| (*value).to_owned()).collect(),
                output_sha256: sha256_bytes(&evidence),
                summary: redact_summary(&summary),
                available: output.status.success(),
            }
        }
        Ok(_) => ToolIdentity {
            name: name.to_owned(),
            command: command.iter().map(|value| (*value).to_owned()).collect(),
            output_sha256: sha256_bytes(b"tool output exceeded bound"),
            summary: "tool output exceeded the 64 KiB safety bound".to_owned(),
            available: false,
        },
        Err(error) => ToolIdentity {
            name: name.to_owned(),
            command: command.iter().map(|value| (*value).to_owned()).collect(),
            output_sha256: sha256_bytes(error.to_string().as_bytes()),
            summary: redact_summary(&error.to_string()),
            available: false,
        },
    }
}

fn redact_summary(summary: &str) -> String {
    summary
        .lines()
        .map(|line| {
            let lowercase = line.to_ascii_lowercase();
            let sensitive = [
                "token",
                "secret",
                "password",
                "authorization",
                "api_key",
                "apikey",
            ]
            .iter()
            .any(|marker| lowercase.contains(marker));
            if !sensitive {
                return line.to_owned();
            }
            if let Some(position) = line.find('=').or_else(|| line.find(':')) {
                format!("{}=[REDACTED]", line[..position].trim_end())
            } else {
                "[REDACTED SENSITIVE LINE]".to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_tree(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    let process_group = -(i32::try_from(child.id()).unwrap_or(i32::MAX));
    signal_process_group(process_group, libc::SIGTERM)?;
    let deadline = Instant::now() + TERMINATION_GRACE_PERIOD;
    while Instant::now() < deadline {
        if let Some(status) = try_wait_without_interruption(child)? {
            signal_process_group(process_group, libc::SIGKILL)?;
            return Ok(status);
        }
        thread::sleep(POLL_INTERVAL);
    }
    signal_process_group(process_group, libc::SIGKILL)?;
    wait_without_interruption(child)
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: i32) -> std::io::Result<()> {
    let result = unsafe { libc::kill(process_group, signal) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn terminate_orphaned_process_group(process_id: u32) {
    let process_group = -(i32::try_from(process_id).unwrap_or(i32::MAX));
    if process_group_exists(process_group) {
        let _ = signal_process_group(process_group, libc::SIGTERM);
        thread::sleep(Duration::from_millis(100));
        let _ = signal_process_group(process_group, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn process_group_exists(process_group: i32) -> bool {
    let result = unsafe { libc::kill(process_group, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn terminate_process_tree(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    let result = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status()?;
    if !result.success() && try_wait_without_interruption(child)?.is_none() {
        child.kill()?;
    }
    wait_without_interruption(child)
}

#[cfg(windows)]
fn terminate_orphaned_process_group(_process_id: u32) {}

#[cfg(not(any(unix, windows)))]
fn terminate_process_tree(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    child.kill()?;
    wait_without_interruption(child)
}

#[cfg(not(any(unix, windows)))]
fn terminate_orphaned_process_group(_process_id: u32) {}

fn try_wait_without_interruption(
    child: &mut std::process::Child,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    loop {
        match child.try_wait() {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn wait_without_interruption(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    loop {
        match child.wait() {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn elapsed_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_redact_common_secret_assignments() {
        let summary = "normal\nAPI_TOKEN=abc123\npassword: value";
        assert_eq!(
            redact_summary(summary),
            "normal\nAPI_TOKEN=[REDACTED]\npassword=[REDACTED]"
        );
    }

    #[test]
    fn receipt_id_is_derived_from_canonical_payload() {
        let payload = ReceiptPayload {
            schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
            taskattest_version: VERSION.to_owned(),
            source: crate::model::SourceIdentity {
                git_commit: None,
                git_ref: None,
                dirty: false,
                status_sha256: "0".repeat(64),
                workspace_sha256: "1".repeat(64),
                workspace_file_count: 0,
                changed_paths: Vec::new(),
            },
            source_after: crate::model::SourceIdentity {
                git_commit: None,
                git_ref: None,
                dirty: false,
                status_sha256: "0".repeat(64),
                workspace_sha256: "1".repeat(64),
                workspace_file_count: 0,
                changed_paths: Vec::new(),
            },
            source_unchanged: true,
            invocation: Invocation {
                changed_only: false,
                requested_checks: Vec::new(),
                fail_fast: false,
                limits: crate::model::ExecutionLimits {
                    max_runtime_ms_per_check: 1,
                    max_log_bytes_per_check: 1,
                },
            },
            discovery: DiscoveryReport {
                schema_version: crate::DISCOVERY_SCHEMA_VERSION.to_owned(),
                source: crate::model::SourceIdentity {
                    git_commit: None,
                    git_ref: None,
                    dirty: false,
                    status_sha256: "0".repeat(64),
                    workspace_sha256: "1".repeat(64),
                    workspace_file_count: 0,
                    changed_paths: Vec::new(),
                },
                checks: Vec::new(),
                selection: Vec::new(),
                coverage_gaps: Vec::new(),
                configuration_files: Vec::new(),
                workflow_observations: Vec::new(),
            },
            started_unix_ms: 1,
            completed_unix_ms: 2,
            duration_ms: 1,
            outcome: ReceiptOutcome::Incomplete,
            checks: Vec::new(),
            coverage_gaps: Vec::new(),
            toolchains: Vec::new(),
            environment: EnvironmentPolicy {
                mode: "allowlist".to_owned(),
                forwarded_names: Vec::new(),
                values_recorded: false,
            },
            redaction: RedactionPolicy {
                name: "test".to_owned(),
                full_logs_redacted: false,
                diagnostic_summaries_redacted: true,
            },
            non_hermetic_inputs: Vec::new(),
            artifacts: Vec::new(),
            annotations: BTreeMap::new(),
        };
        let first = build_receipt(payload.clone()).expect("build first receipt");
        let second = build_receipt(payload).expect("build second receipt");
        assert_eq!(first.receipt_id, second.receipt_id);
        assert_eq!(first.canonical_digest, second.canonical_digest);
    }
}

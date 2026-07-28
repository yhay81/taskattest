use std::collections::BTreeMap;

use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
    Ndjson,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Format,
    Lint,
    Test,
    TypeCheck,
    Build,
    Custom,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Explicit,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct DiscoverySource {
    pub path: String,
    pub sha256: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CheckDefinition {
    pub id: String,
    pub label: String,
    pub kind: CheckKind,
    pub command: CommandSpec,
    pub reason: String,
    pub sources: Vec<DiscoverySource>,
    pub confidence: Confidence,
    pub coverage_paths: Vec<String>,
    pub pass_environment: Vec<String>,
    pub non_hermetic_inputs: Vec<String>,
    pub replaces_workflow_steps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CheckSelection {
    pub check_id: String,
    pub selected: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SourceIdentity {
    pub git_commit: Option<String>,
    pub git_ref: Option<String>,
    pub dirty: bool,
    pub status_sha256: String,
    pub workspace_sha256: String,
    pub workspace_file_count: u64,
    pub changed_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct DiscoveryReport {
    pub schema_version: String,
    pub source: SourceIdentity,
    pub checks: Vec<CheckDefinition>,
    pub selection: Vec<CheckSelection>,
    pub coverage_gaps: Vec<String>,
    pub configuration_files: Vec<DiscoverySource>,
    pub workflow_observations: Vec<WorkflowObservation>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowClassification {
    MatchedCheck,
    DiscoveredCheck,
    UnmodeledVerification,
    ReplacedByExplicitCheck,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct WorkflowObservation {
    pub id: String,
    pub source: DiscoverySource,
    pub job: String,
    pub step: String,
    pub run_sha256: String,
    pub run_summary: String,
    pub classification: WorkflowClassification,
    pub check_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ExecutionLimits {
    pub max_runtime_ms_per_check: u64,
    pub max_log_bytes_per_check: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Invocation {
    pub changed_only: bool,
    pub requested_checks: Vec<String>,
    pub fail_fast: bool,
    pub limits: ExecutionLimits,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct LogReference {
    pub algorithm: String,
    pub digest: String,
    pub bytes: u64,
    pub handle: String,
    pub content_encoding: String,
    pub sensitivity: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    LogLimitExceeded,
    SpawnFailed,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CheckExecution {
    pub check: CheckDefinition,
    pub started_unix_ms: u64,
    pub completed_unix_ms: u64,
    pub duration_ms: u64,
    pub outcome: CheckOutcome,
    pub exit_code: Option<i32>,
    pub stdout: Option<LogReference>,
    pub stderr: Option<LogReference>,
    pub stdout_summary: String,
    pub stderr_summary: String,
    pub diagnostic_summaries_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ToolIdentity {
    pub name: String,
    pub command: Vec<String>,
    pub output_sha256: String,
    pub summary: String,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentPolicy {
    pub mode: String,
    pub forwarded_names: Vec<String>,
    pub values_recorded: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct RedactionPolicy {
    pub name: String,
    pub full_logs_redacted: bool,
    pub diagnostic_summaries_redacted: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    Passed,
    Failed,
    Incomplete,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ReceiptPayload {
    pub schema_version: String,
    pub taskattest_version: String,
    pub source: SourceIdentity,
    pub source_after: SourceIdentity,
    pub source_unchanged: bool,
    pub invocation: Invocation,
    pub discovery: DiscoveryReport,
    pub started_unix_ms: u64,
    pub completed_unix_ms: u64,
    pub duration_ms: u64,
    pub outcome: ReceiptOutcome,
    pub checks: Vec<CheckExecution>,
    pub coverage_gaps: Vec<String>,
    pub toolchains: Vec<ToolIdentity>,
    pub environment: EnvironmentPolicy,
    pub redaction: RedactionPolicy,
    pub non_hermetic_inputs: Vec<String>,
    pub artifacts: Vec<ArtifactReference>,
    pub annotations: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Receipt {
    pub receipt_id: String,
    pub canonical_digest: String,
    #[serde(flatten)]
    pub payload: ReceiptPayload,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ArtifactReference {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressState {
    CheckStarted,
    CheckFinished,
    ReceiptStored,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ProgressEvent {
    pub schema_version: String,
    pub sequence: u64,
    pub unix_ms: u64,
    pub state: ProgressState,
    pub check_id: Option<String>,
    pub outcome: Option<CheckOutcome>,
    pub receipt_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct BlobVerification {
    pub handle: String,
    pub found: bool,
    pub digest_matches: bool,
    pub expected_bytes: u64,
    pub actual_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct VerificationReport {
    pub schema_version: String,
    pub receipt_id: String,
    pub valid: bool,
    pub schema_supported: bool,
    pub canonical_digest_matches: bool,
    pub receipt_id_matches: bool,
    pub blobs: Vec<BlobVerification>,
    pub problems: Vec<String>,
}

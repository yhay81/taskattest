use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

fn taskattest(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskattest"))
        .arg("--workspace")
        .arg(workspace)
        .args(args)
        .output()
        .expect("run taskattest")
}

fn taskattest_from_workspace(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskattest"))
        .current_dir(workspace)
        .args(["--workspace", "."])
        .args(args)
        .output()
        .expect("run taskattest from workspace")
}

fn git(workspace: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn workspace_with_check(command: &[&str]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary workspace");
    git(directory.path(), &["init", "--quiet"]);
    git(
        directory.path(),
        &["config", "user.email", "taskattest@example.invalid"],
    );
    git(
        directory.path(),
        &["config", "user.name", "TaskAttest Test"],
    );
    let command = command
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    let config = format!(
        r#"version = 1

[[checks]]
id = "portable-check"
label = "Portable fixture check"
kind = "test"
command = [{command}]
reason = "the fixture explicitly declares this check"
coverage_paths = ["**"]
"#
    );
    fs::write(directory.path().join(".taskattest.toml"), config)
        .expect("write TaskAttest configuration");
    fs::write(directory.path().join("tracked.txt"), "original\n").expect("write tracked file");
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "--quiet", "-m", "fixture"]);
    directory
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("quote TOML-compatible basic string")
}

#[test]
fn run_store_show_and_verify_round_trip() {
    let directory = workspace_with_check(&["git", "status", "--short"]);
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "json"],
    );
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    assert_eq!(receipt["outcome"], "passed");
    assert_eq!(receipt["source_unchanged"], true);
    assert_eq!(receipt["checks"][0]["outcome"], "passed");
    let receipt_id = receipt["receipt_id"].as_str().expect("receipt id");

    let show = taskattest(
        directory.path(),
        &["receipt", "show", receipt_id, "--format", "json"],
    );
    assert!(show.status.success());
    let shown: Value = serde_json::from_slice(&show.stdout).expect("parse shown receipt");
    assert_eq!(shown["canonical_digest"], receipt["canonical_digest"]);

    let verify = taskattest(
        directory.path(),
        &["verify", receipt_id, "--format", "json"],
    );
    assert!(
        verify.status.success(),
        "verify failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let report: Value = serde_json::from_slice(&verify.stdout).expect("parse verification");
    assert_eq!(report["valid"], true);
    assert_eq!(report["blobs"].as_array().map(Vec::len), Some(2));
}

#[test]
fn ndjson_progress_uses_stderr_and_final_receipt_uses_stdout() {
    let directory = workspace_with_check(&["git", "status", "--short"]);
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "ndjson"],
    );
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&run.stdout).expect("stdout must contain only the final receipt");
    assert_eq!(receipt["outcome"], "passed");
    let progress = String::from_utf8(run.stderr).expect("progress must be UTF-8");
    let events = progress
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse progress event"))
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| event["state"] == "check_started"));
    assert!(
        events
            .iter()
            .any(|event| event["state"] == "receipt_stored")
    );
}

#[test]
fn occupied_receipt_output_is_rejected_before_check_execution() {
    let directory = workspace_with_check(&["git", "tag", "taskattest-check-ran"]);
    let requested = directory.path().join(".git").join("occupied-receipt.json");
    fs::write(&requested, b"occupied-before-run").expect("occupy receipt path");

    let run = taskattest_from_workspace(
        directory.path(),
        &[
            "run",
            "--check",
            "portable-check",
            "--receipt-out",
            ".git/occupied-receipt.json",
            "--format",
            "json",
        ],
    );

    assert_eq!(run.status.code(), Some(4));
    assert!(run.stdout.is_empty());
    let error: Value = serde_json::from_slice(&run.stderr).expect("parse preflight error");
    assert_eq!(error["schema_version"], "taskattest.error.v1");
    assert_eq!(error["error_code"], "output_already_exists");
    assert_eq!(
        fs::read(&requested).expect("read occupied receipt"),
        b"occupied-before-run"
    );
    assert!(
        !directory
            .path()
            .join(".git")
            .join("refs")
            .join("tags")
            .join("taskattest-check-ran")
            .exists(),
        "preflight failure must prevent the configured check"
    );
    assert!(
        !directory.path().join(".git").join("taskattest").exists(),
        "preflight failure must not create a receipt store"
    );
}

#[test]
fn post_check_receipt_race_returns_durable_receipt_and_forbids_retry() {
    let directory = workspace_with_check(&[
        "git",
        "archive",
        "--format=tar",
        "--output=.git/standalone-receipt.json",
        "HEAD",
    ]);
    let requested = directory
        .path()
        .join(".git")
        .join("standalone-receipt.json");
    assert!(!requested.exists());

    let args = [
        "run",
        "--check",
        "portable-check",
        "--receipt-out",
        ".git/standalone-receipt.json",
        "--format",
        "json",
    ];
    let run = taskattest_from_workspace(directory.path(), &args);

    assert_eq!(run.status.code(), Some(7));
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse durable receipt");
    assert_eq!(receipt["outcome"], "passed");
    let receipt_id = receipt["receipt_id"].as_str().expect("receipt id");
    let recovery: Value =
        serde_json::from_slice(&run.stderr).expect("parse receipt recovery document");
    assert_eq!(recovery["schema_version"], "taskattest.receipt-recovery.v1");
    assert_eq!(
        recovery["error_code"],
        "receipt_publication_failed_after_execution"
    );
    assert_eq!(recovery["exit_code"], 7);
    assert_eq!(recovery["action"], "do_not_retry_run");
    assert_eq!(recovery["command_state"], "checks_completed");
    assert_eq!(recovery["receipt_id"], receipt_id);
    assert_eq!(recovery["receipt_persisted"], true);
    assert_eq!(recovery["publication_error_code"], "output_already_exists");
    assert_eq!(
        recovery["requested_receipt"],
        ".git/standalone-receipt.json"
    );
    let stored_path = Path::new(
        recovery["stored_receipt"]
            .as_str()
            .expect("stored receipt path"),
    );
    let stored: Value =
        serde_json::from_slice(&fs::read(stored_path).expect("read stored receipt"))
            .expect("parse stored receipt");
    assert_eq!(stored, receipt);
    let requested_before_retry = fs::read(&requested).expect("read raced output");
    assert!(
        serde_json::from_slice::<Value>(&requested_before_retry).is_err(),
        "the command-created path must not be replaced by the receipt"
    );
    assert_eq!(
        fs::read_dir(
            directory
                .path()
                .join(".git")
                .join("taskattest")
                .join("receipts")
        )
        .expect("read receipt store")
        .count(),
        1
    );

    let retry = taskattest_from_workspace(directory.path(), &args);
    assert_eq!(retry.status.code(), Some(4));
    assert!(retry.stdout.is_empty());
    let retry_error: Value =
        serde_json::from_slice(&retry.stderr).expect("parse retry preflight error");
    assert_eq!(retry_error["error_code"], "output_already_exists");
    assert_eq!(
        fs::read(&requested).expect("read requested output after retry"),
        requested_before_retry
    );
    assert_eq!(
        fs::read_dir(
            directory
                .path()
                .join(".git")
                .join("taskattest")
                .join("receipts")
        )
        .expect("read receipt store after retry")
        .count(),
        1,
        "the refused retry must not execute the check or create another receipt"
    );
}

#[test]
fn brief_schema_and_every_completion_target_are_available() {
    let directory = workspace_with_check(&["git", "status", "--short"]);
    let schema = taskattest(
        directory.path(),
        &["schema", "--document", "brief", "--format", "json"],
    );
    assert!(schema.status.success());
    let document: Value = serde_json::from_slice(&schema.stdout).expect("parse brief schema");
    assert_eq!(document["schema_version"], "taskattest.brief.v1");
    assert_eq!(
        document["exit_codes"]["7"],
        "checks completed and the durable receipt exists, but standalone receipt publication failed; do not retry run"
    );
    assert_eq!(
        document["document_versions"]["receipt_recovery"],
        "taskattest.receipt-recovery.v1"
    );
    assert_eq!(
        document["safety"]["standalone_receipt_publication"],
        "preflighted_no_clobber_with_no_retry_recovery"
    );

    let recovery_schema = taskattest(
        directory.path(),
        &[
            "schema",
            "--document",
            "receipt-recovery",
            "--format",
            "json",
        ],
    );
    assert!(recovery_schema.status.success());
    let recovery_document: Value =
        serde_json::from_slice(&recovery_schema.stdout).expect("parse recovery schema");
    assert_eq!(
        recovery_document
            .pointer("/$defs/ReceiptRecoveryAction/enum/0")
            .and_then(Value::as_str),
        Some("do_not_retry_run")
    );

    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let completion = taskattest(directory.path(), &["completions", shell]);
        assert!(
            completion.status.success(),
            "{shell} completion failed: {}",
            String::from_utf8_lossy(&completion.stderr)
        );
        assert!(!completion.stdout.is_empty());
    }
}

#[test]
fn tampered_receipt_is_rejected_without_rerunning_checks() {
    let directory = workspace_with_check(&["git", "status", "--short"]);
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "json"],
    );
    assert!(run.status.success());
    let mut receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    receipt["duration_ms"] = Value::from(
        receipt["duration_ms"]
            .as_u64()
            .expect("duration")
            .saturating_add(1),
    );
    let tampered = directory.path().join("tampered.json");
    fs::write(
        &tampered,
        serde_json::to_vec_pretty(&receipt).expect("serialize tampered receipt"),
    )
    .expect("write tampered receipt");
    let state_dir = directory.path().join(".git").join("taskattest");
    let verify = Command::new(env!("CARGO_BIN_EXE_taskattest"))
        .arg("--workspace")
        .arg(directory.path())
        .arg("--state-dir")
        .arg(state_dir)
        .args([
            "verify",
            tampered.to_str().expect("UTF-8 path"),
            "--format",
            "json",
        ])
        .output()
        .expect("verify tampered receipt");
    assert_eq!(verify.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&verify.stdout).expect("parse verification");
    assert_eq!(report["valid"], false);
    assert_eq!(report["canonical_digest_matches"], false);
}

#[test]
fn failed_check_still_produces_an_internally_valid_receipt() {
    let directory = workspace_with_check(&["git", "show", "--definitely-invalid-option"]);
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "json"],
    );
    assert_eq!(
        run.status.code(),
        Some(1),
        "unexpected taskattest stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    assert_eq!(receipt["outcome"], "failed");
    assert_eq!(receipt["checks"][0]["outcome"], "failed");
    assert!(
        receipt["checks"][0]["stderr"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    );
    let receipt_id = receipt["receipt_id"].as_str().expect("receipt id");
    let verify = taskattest(
        directory.path(),
        &["verify", receipt_id, "--format", "json"],
    );
    assert!(verify.status.success());
    let report: Value = serde_json::from_slice(&verify.stdout).expect("parse verification");
    assert_eq!(report["valid"], true);
}

#[cfg(unix)]
#[test]
fn timeout_terminates_the_process_group_and_records_evidence() {
    let directory = workspace_with_check(&["sh", "-c", "sleep 30 & wait"]);
    let started = Instant::now();
    let run = taskattest(
        directory.path(),
        &[
            "run",
            "--check",
            "portable-check",
            "--max-runtime-ms-per-check",
            "100",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        run.status.code(),
        Some(1),
        "unexpected taskattest stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    assert_eq!(receipt["checks"][0]["outcome"], "timed_out");
    assert_eq!(receipt["source_unchanged"], true);
}

#[cfg(unix)]
#[test]
fn successful_parent_cannot_leave_an_orphan_holding_log_pipes_open() {
    let directory = workspace_with_check(&["sh", "-c", "(trap '' TERM; sleep 30) & exit 0"]);
    let started = Instant::now();
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "json"],
    );
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    assert_eq!(receipt["checks"][0]["outcome"], "passed");
}

#[cfg(unix)]
#[test]
fn a_check_that_mutates_source_cannot_produce_a_passing_receipt() {
    let directory = workspace_with_check(&["sh", "-c", "printf changed > tracked.txt"]);
    let run = taskattest(
        directory.path(),
        &["run", "--check", "portable-check", "--format", "json"],
    );
    assert_eq!(run.status.code(), Some(1));
    let receipt: Value = serde_json::from_slice(&run.stdout).expect("parse receipt");
    assert_eq!(receipt["checks"][0]["outcome"], "passed");
    assert_eq!(receipt["source_unchanged"], false);
    assert_eq!(receipt["outcome"], "failed");
}

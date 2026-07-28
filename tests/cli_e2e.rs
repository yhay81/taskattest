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
fn brief_schema_and_every_completion_target_are_available() {
    let directory = workspace_with_check(&["git", "status", "--short"]);
    let schema = taskattest(
        directory.path(),
        &["schema", "--document", "brief", "--format", "json"],
    );
    assert!(schema.status.success());
    let document: Value = serde_json::from_slice(&schema.stdout).expect("parse brief schema");
    assert_eq!(document["schema_version"], "taskattest.brief.v1");

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

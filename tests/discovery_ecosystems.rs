use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

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

fn discover(files: &[(&str, &str)]) -> Value {
    let directory = workspace(files);
    let output = taskattest(directory.path(), &["discover", "--format", "json"]);
    assert!(
        output.status.success(),
        "discover failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse discovery report")
}

fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
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
    for (path, contents) in files {
        let path = directory.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture");
    }
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "--quiet", "-m", "fixture"]);
    directory
}

fn taskattest(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskattest"))
        .arg("--workspace")
        .arg(workspace)
        .args(args)
        .output()
        .expect("run taskattest")
}

fn check_ids(report: &Value) -> Vec<&str> {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .map(|check| check["id"].as_str().expect("check id"))
        .collect()
}

#[test]
fn discovers_safe_javascript_package_scripts_and_lockfile_manager() {
    let report = discover(&[
        (
            "package.json",
            r#"{
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest",
    "lint": "eslint . --fix",
    "lint:ci": "eslint .",
    "typecheck": "tsc --noEmit",
    "build": "tsc -p tsconfig.build.json"
  }
}"#,
        ),
        ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n"),
    ]);
    assert_eq!(
        check_ids(&report),
        ["js-build", "js-lint-ci", "js-test", "js-typecheck"]
    );
    assert!(
        report["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .all(|check| check["command"]["program"] == "pnpm")
    );
}

#[test]
fn discovers_declared_python_tools_without_guessing_unittest() {
    let report = discover(&[(
        "pyproject.toml",
        r#"[project]
name = "fixture"
version = "0.1.0"
dependencies = ["pytest>=8", "ruff>=0.5", "mypy>=1.10"]

[tool.pytest.ini_options]
testpaths = ["tests"]

[tool.ruff]
line-length = 100

[tool.mypy]
strict = true
"#,
    )]);
    assert_eq!(
        check_ids(&report),
        ["python-mypy", "python-ruff", "python-test"]
    );
}

#[test]
fn python_comments_do_not_create_checks_and_invalid_toml_is_rejected() {
    let report = discover(&[(
        "pyproject.toml",
        r#"[project]
name = "fixture"
version = "0.1.0"
# pytest, ruff, and mypy are intentionally not configured.
"#,
    )]);
    assert!(check_ids(&report).is_empty());
    assert_eq!(report["coverage_gaps"].as_array().map(Vec::len), Some(1));

    let directory = workspace(&[("pyproject.toml", "[project\nname = \"broken\"\n")]);
    let output = taskattest(directory.path(), &["discover", "--format", "json"]);
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn discovers_go_test_vet_and_build_checks() {
    let report = discover(&[("go.mod", "module example.invalid/fixture\n\ngo 1.23\n")]);
    assert_eq!(check_ids(&report), ["go-build", "go-test", "go-vet"]);
}

#[test]
fn explicit_argv_check_can_replace_an_unmodeled_workflow_step() {
    let directory = workspace(&[
        (
            ".github/workflows/ci.yml",
            r#"name: CI
on: push
jobs:
  quality:
    runs-on: ubuntu-latest
    steps:
      - name: Validate fixtures
        run: |
          python3 -m json.tool one.json >/dev/null
          python3 -m json.tool two.json >/dev/null
"#,
        ),
        (
            ".taskattest.toml",
            r#"version = 1

[[checks]]
id = "fixture-json"
label = "JSON fixtures"
kind = "test"
command = ["git", "status", "--short"]
reason = "the checked-in configuration supplies an argv-only equivalent"
coverage_paths = ["**"]
replaces_workflow_steps = [".github/workflows/ci.yml#quality#Validate fixtures"]
"#,
        ),
        ("one.json", "{}\n"),
        ("two.json", "{}\n"),
    ]);
    let output = taskattest(directory.path(), &["run", "--format", "json"]);
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("parse receipt");
    assert_eq!(receipt["outcome"], "passed");
    assert_eq!(
        receipt["discovery"]["workflow_observations"][0]["classification"],
        "replaced_by_explicit_check"
    );
    assert_eq!(
        receipt["discovery"]["workflow_observations"][0]["check_id"],
        "fixture-json"
    );
    assert_eq!(receipt["coverage_gaps"].as_array().map(Vec::len), Some(0));
}

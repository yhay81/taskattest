use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::CONFIG_VERSION;
use crate::DISCOVERY_SCHEMA_VERSION;
use crate::error::TaskError;
use crate::git::GitContext;
use crate::model::{
    CheckDefinition, CheckKind, CheckSelection, CommandSpec, Confidence, DiscoveryReport,
    DiscoverySource, SourceIdentity, WorkflowClassification, WorkflowObservation,
};
use crate::source::{sha256_bytes, sha256_path};

const MAX_WORKFLOW_BYTES: u64 = 1024 * 1024;
const MAX_PROJECT_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskAttestConfig {
    version: u32,
    #[serde(default)]
    disable_checks: Vec<String>,
    #[serde(default)]
    checks: Vec<ConfiguredCheck>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredCheck {
    id: String,
    label: String,
    kind: CheckKind,
    command: Vec<String>,
    #[serde(default = "default_working_directory")]
    working_directory: String,
    reason: String,
    #[serde(default)]
    coverage_paths: Vec<String>,
    #[serde(default)]
    pass_environment: Vec<String>,
    #[serde(default)]
    non_hermetic_inputs: Vec<String>,
    #[serde(default)]
    replaces_workflow_steps: Vec<String>,
}

fn default_working_directory() -> String {
    ".".to_owned()
}

pub fn discover_checks(
    git: &GitContext,
    source: SourceIdentity,
    changed_only: bool,
    requested_checks: &[String],
) -> Result<DiscoveryReport, TaskError> {
    let config_path = git.root.join(".taskattest.toml");
    let (config, config_source) = if config_path.is_file() {
        let text = read_bounded_project_file(&config_path, ".taskattest.toml")?;
        let config: TaskAttestConfig = toml::from_str(&text).map_err(|error| {
            TaskError::configuration(format!("parse .taskattest.toml: {error}"))
        })?;
        if config.version != CONFIG_VERSION {
            return Err(TaskError::configuration(format!(
                "unsupported .taskattest.toml version {}; expected {}",
                config.version, CONFIG_VERSION
            )));
        }
        let source = discovery_source(
            &git.root,
            ".taskattest.toml",
            "explicit TaskAttest configuration",
        )?;
        (config, Some(source))
    } else {
        (TaskAttestConfig::default(), None)
    };

    let mut checks = Vec::new();
    if git.root.join("package.json").is_file() {
        checks.extend(discover_javascript(git)?);
    }
    if [
        "pyproject.toml",
        "requirements.txt",
        "requirements-dev.txt",
        "tox.ini",
    ]
    .iter()
    .any(|path| git.root.join(path).is_file())
    {
        checks.extend(discover_python(git)?);
    }
    if git.root.join("Cargo.toml").is_file() {
        checks.extend(discover_rust(git)?);
    }
    if git.root.join("go.mod").is_file() {
        checks.extend(discover_go(git)?);
    }
    let mut workflow_discovery = discover_workflows(git, &mut checks)?;

    let disabled: BTreeSet<_> = config.disable_checks.iter().cloned().collect();
    checks.retain(|check| !disabled.contains(&check.id));
    for configured in config.checks {
        checks.push(configured_check(git, configured, config_source.clone())?);
    }
    checks.sort_by(|left, right| left.id.cmp(&right.id));
    validate_checks(&checks)?;
    apply_workflow_replacements(&checks, &mut workflow_discovery)?;

    let requested: BTreeSet<_> = requested_checks.iter().cloned().collect();
    for check_id in &requested {
        if !checks.iter().any(|check| &check.id == check_id) {
            return Err(TaskError::discovery(format!(
                "requested check was not discovered: {check_id}"
            )));
        }
    }

    let matchers = checks
        .iter()
        .map(|check| compile_coverage(&check.coverage_paths))
        .collect::<Result<Vec<_>, _>>()?;
    let selection: Vec<_> = checks
        .iter()
        .zip(matchers.iter())
        .map(|(check, matcher)| {
            select_check(
                check,
                matcher,
                &source.changed_paths,
                changed_only,
                &requested,
            )
        })
        .collect();

    let mut coverage_gaps = coverage_gaps(&source.changed_paths, &checks, &matchers, &selection)?;
    coverage_gaps.extend(workflow_discovery.coverage_gaps);
    coverage_gaps.sort();
    coverage_gaps.dedup();
    let configuration_files = config_source
        .into_iter()
        .chain(workflow_discovery.sources)
        .collect();

    Ok(DiscoveryReport {
        schema_version: DISCOVERY_SCHEMA_VERSION.to_owned(),
        source,
        checks,
        selection,
        coverage_gaps,
        configuration_files,
        workflow_observations: workflow_discovery.observations,
    })
}

#[derive(Default)]
struct WorkflowDiscovery {
    observations: Vec<WorkflowObservation>,
    sources: Vec<DiscoverySource>,
    coverage_gaps: Vec<String>,
}

fn discover_workflows(
    git: &GitContext,
    checks: &mut Vec<CheckDefinition>,
) -> Result<WorkflowDiscovery, TaskError> {
    let workflows_directory = git.root.join(".github").join("workflows");
    if !workflows_directory.is_dir() {
        return Ok(WorkflowDiscovery::default());
    }
    let mut workflow_paths = std::fs::read_dir(&workflows_directory)
        .map_err(|error| {
            TaskError::io(
                "read GitHub Actions workflow directory",
                &workflows_directory,
                error,
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "yml" | "yaml"))
        })
        .collect::<Vec<_>>();
    workflow_paths.sort();
    let mut result = WorkflowDiscovery::default();

    for path in workflow_paths {
        let metadata = std::fs::metadata(&path)
            .map_err(|error| TaskError::io("inspect workflow", &path, error))?;
        if metadata.len() > MAX_WORKFLOW_BYTES {
            return Err(TaskError::discovery(format!(
                "workflow exceeds the 1 MiB safety bound: {}",
                path.display()
            )));
        }
        let relative = path
            .strip_prefix(&git.root)
            .ok()
            .and_then(Path::to_str)
            .ok_or_else(|| TaskError::discovery("workflow path is not portable UTF-8"))?
            .replace('\\', "/");
        let source = discovery_source(&git.root, &relative, "GitHub Actions workflow")?;
        result.sources.push(source.clone());
        let text = std::fs::read_to_string(&path)
            .map_err(|error| TaskError::io("read workflow", &path, error))?;
        let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).map_err(|error| {
            TaskError::discovery(format!("parse GitHub Actions workflow {relative}: {error}"))
        })?;
        inspect_workflow_document(git, &relative, &source, &document, checks, &mut result)?;
    }
    Ok(result)
}

fn inspect_workflow_document(
    git: &GitContext,
    relative: &str,
    source: &DiscoverySource,
    document: &serde_yaml_ng::Value,
    checks: &mut Vec<CheckDefinition>,
    result: &mut WorkflowDiscovery,
) -> Result<(), TaskError> {
    let Some(jobs) = yaml_field(document, "jobs").and_then(serde_yaml_ng::Value::as_mapping) else {
        return Ok(());
    };
    for (job_key, job_value) in jobs {
        let job = yaml_scalar(job_key).unwrap_or_else(|| "unnamed-job".to_owned());
        let Some(steps) =
            yaml_field(job_value, "steps").and_then(serde_yaml_ng::Value::as_sequence)
        else {
            continue;
        };
        for (index, step_value) in steps.iter().enumerate() {
            let Some(run) = yaml_field(step_value, "run").and_then(serde_yaml_ng::Value::as_str)
            else {
                continue;
            };
            let step = yaml_field(step_value, "name")
                .and_then(serde_yaml_ng::Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("step-{}", index + 1));
            let observation_id = format!("{relative}#{job}#{step}");
            let run_sha256 = sha256_bytes(run.as_bytes());
            let run_summary = bounded_run_summary(run);
            let working_directory = yaml_field(step_value, "working-directory")
                .and_then(serde_yaml_ng::Value::as_str)
                .unwrap_or(".");
            match safe_workflow_command(run, working_directory) {
                Ok(Some((kind, command))) => {
                    validate_working_directory(&git.root, &command.working_directory)?;
                    if let Some(existing) = checks.iter_mut().find(|check| {
                        check.command.program == command.program
                            && check.command.args == command.args
                            && check.command.working_directory == command.working_directory
                    }) {
                        if !existing.sources.iter().any(|item| item.path == source.path) {
                            existing.sources.push(source.clone());
                        }
                        existing.reason =
                            format!("{}; also declared by {observation_id}", existing.reason);
                        result.observations.push(WorkflowObservation {
                            id: observation_id,
                            source: source.clone(),
                            job: job.clone(),
                            step,
                            run_sha256,
                            run_summary,
                            classification: WorkflowClassification::MatchedCheck,
                            check_id: Some(existing.id.clone()),
                            reason: "safe workflow command matches an existing discovered check"
                                .to_owned(),
                        });
                    } else {
                        let id = format!("ci-{}-{}", check_kind_id(&kind), &run_sha256[..8]);
                        checks.push(CheckDefinition {
                            id: id.clone(),
                            label: format!("CI: {step}"),
                            kind,
                            command,
                            reason: format!("safe single-command workflow step {observation_id}"),
                            sources: vec![source.clone()],
                            confidence: Confidence::High,
                            coverage_paths: language_coverage(run),
                            pass_environment: Vec::new(),
                            non_hermetic_inputs: vec![
                                "GitHub Actions environment is not reproduced locally".to_owned(),
                                "workflow job services and setup steps are not reproduced"
                                    .to_owned(),
                            ],
                            replaces_workflow_steps: Vec::new(),
                        });
                        result.observations.push(WorkflowObservation {
                            id: observation_id,
                            source: source.clone(),
                            job: job.clone(),
                            step,
                            run_sha256,
                            run_summary,
                            classification: WorkflowClassification::DiscoveredCheck,
                            check_id: Some(id),
                            reason: "safe workflow command was converted to an argument vector"
                                .to_owned(),
                        });
                    }
                }
                Ok(None) | Err(_)
                    if looks_like_unmodeled_verification(relative, &job, &step, run) =>
                {
                    result.coverage_gaps.push(format!(
                        "verification workflow step is not safely modeled: {observation_id}"
                    ));
                    result.observations.push(WorkflowObservation {
                        id: observation_id,
                        source: source.clone(),
                        job: job.clone(),
                        step,
                        run_sha256,
                        run_summary,
                        classification: WorkflowClassification::UnmodeledVerification,
                        check_id: None,
                        reason: "step requires shell syntax, an unsupported verifier, or dynamic workflow context"
                            .to_owned(),
                    });
                }
                Ok(None) | Err(_) => {}
            }
        }
    }
    Ok(())
}

fn yaml_field<'a>(value: &'a serde_yaml_ng::Value, key: &str) -> Option<&'a serde_yaml_ng::Value> {
    value
        .as_mapping()?
        .get(serde_yaml_ng::Value::String(key.to_owned()))
}

fn yaml_scalar(value: &serde_yaml_ng::Value) -> Option<String> {
    match value {
        serde_yaml_ng::Value::String(value) => Some(value.clone()),
        serde_yaml_ng::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn looks_like_unmodeled_verification(relative: &str, job: &str, step: &str, run: &str) -> bool {
    if looks_like_operational_step(relative, job, step) {
        return false;
    }
    if run.to_ascii_lowercase().contains("taskattest") {
        // A TaskAttest invocation records the underlying checks; recursively
        // discovering it would make a repository's evidence set self-referential.
        return false;
    }
    let lowercase = format!("{step} {run}").to_ascii_lowercase();
    [
        " test",
        "test ",
        "clippy",
        " fmt",
        "fmt ",
        " lint",
        "lint ",
        " check",
        "check ",
        "require",
        "fixture",
        "schema",
        "capability",
        "pytest",
        "ruff",
        "mypy",
        "go vet",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
}

fn looks_like_operational_step(relative: &str, job: &str, step: &str) -> bool {
    let relative = relative.to_ascii_lowercase();
    let job = job.to_ascii_lowercase();
    let step = step.to_ascii_lowercase();
    let setup_prefix = [
        "install ",
        "set up ",
        "setup ",
        "configure ",
        "checkout ",
        "cache ",
        "upload ",
        "download ",
    ];
    if setup_prefix.iter().any(|prefix| step.starts_with(prefix)) {
        return true;
    }
    let delivery_context =
        relative.contains("release") || job.contains("publish") || job.contains("release");
    delivery_context
        && [
            "stage ",
            "archive",
            "checksum",
            "sbom",
            "create release",
            "publish",
            "upload",
            "sign ",
            "provenance",
        ]
        .iter()
        .any(|marker| step.contains(marker))
}

fn safe_workflow_command(
    run: &str,
    working_directory: &str,
) -> Result<Option<(CheckKind, CommandSpec)>, TaskError> {
    if run.contains(['\n', '\r', '|', '&', ';', '>', '<', '$', '`'])
        || working_directory.contains("${{")
    {
        return Ok(None);
    }
    let Some(tokens) = shlex::split(run) else {
        return Ok(None);
    };
    if tokens.is_empty() || tokens[0].contains('=') {
        return Ok(None);
    }
    let (kind, command_index) = if tokens[0] == "cargo" {
        let index = usize::from(tokens.get(1).is_some_and(|value| value.starts_with('+'))) + 1;
        let Some(subcommand) = tokens.get(index) else {
            return Ok(None);
        };
        let kind = match subcommand.as_str() {
            "fmt" => CheckKind::Format,
            "clippy" => CheckKind::Lint,
            "test" => CheckKind::Test,
            "check" => CheckKind::TypeCheck,
            "build" | "package" => CheckKind::Build,
            _ => return Ok(None),
        };
        (kind, index)
    } else if matches!(tokens[0].as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        let script = match tokens.as_slice() {
            [manager, command, script, ..] if manager == "npm" && command == "run" => script,
            [manager, command, script, ..]
                if matches!(manager.as_str(), "pnpm" | "yarn" | "bun") && command == "run" =>
            {
                script
            }
            [manager, script, ..]
                if matches!(manager.as_str(), "npm" | "pnpm" | "yarn" | "bun") =>
            {
                script
            }
            _ => return Ok(None),
        };
        let Some(kind) = javascript_script_kind(script, "") else {
            return Ok(None);
        };
        (kind, 1)
    } else if matches!(tokens[0].as_str(), "python" | "python3") {
        let Some(module) = tokens
            .windows(2)
            .find_map(|pair| (pair[0] == "-m").then_some(pair[1].as_str()))
        else {
            return Ok(None);
        };
        let kind = match module {
            "pytest" | "unittest" | "tox" => CheckKind::Test,
            "ruff" => CheckKind::Lint,
            "mypy" | "pyright" => CheckKind::TypeCheck,
            "build" => CheckKind::Build,
            _ => return Ok(None),
        };
        (kind, 1)
    } else if tokens[0] == "go" {
        let kind = match tokens.get(1).map(String::as_str) {
            Some("test") => CheckKind::Test,
            Some("vet") => CheckKind::Lint,
            Some("build") => CheckKind::Build,
            _ => return Ok(None),
        };
        (kind, 1)
    } else {
        return Ok(None);
    };
    if command_index >= tokens.len() {
        return Ok(None);
    }
    Ok(Some((
        kind,
        CommandSpec {
            program: tokens[0].clone(),
            args: tokens[1..].to_vec(),
            working_directory: working_directory.to_owned(),
        },
    )))
}

fn check_kind_id(kind: &CheckKind) -> &'static str {
    match kind {
        CheckKind::Format => "format",
        CheckKind::Lint => "lint",
        CheckKind::Test => "test",
        CheckKind::TypeCheck => "typecheck",
        CheckKind::Build => "build",
        CheckKind::Custom => "custom",
    }
}

fn language_coverage(run: &str) -> Vec<String> {
    if run.trim_start().starts_with("cargo ") {
        vec![
            "**/*.rs".to_owned(),
            "Cargo.toml".to_owned(),
            "Cargo.lock".to_owned(),
            ".cargo/**".to_owned(),
            ".github/workflows/**".to_owned(),
            ".taskattest.toml".to_owned(),
        ]
    } else if ["npm ", "pnpm ", "yarn ", "bun "]
        .iter()
        .any(|prefix| run.trim_start().starts_with(prefix))
    {
        javascript_coverage()
    } else if ["python ", "python3 "]
        .iter()
        .any(|prefix| run.trim_start().starts_with(prefix))
    {
        python_coverage()
    } else if run.trim_start().starts_with("go ") {
        go_coverage()
    } else {
        Vec::new()
    }
}

fn bounded_run_summary(run: &str) -> String {
    let flattened = run.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result = flattened.chars().take(240).collect::<String>();
    if flattened.chars().count() > 240 {
        result.push('…');
    }
    result
}

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

fn discover_javascript(git: &GitContext) -> Result<Vec<CheckDefinition>, TaskError> {
    let text = read_bounded_project_file(&git.root.join("package.json"), "package.json")?;
    let package: PackageJson = serde_json::from_str(&text)
        .map_err(|error| TaskError::discovery(format!("parse package.json: {error}")))?;
    let (manager, lockfile) = [
        ("pnpm", "pnpm-lock.yaml"),
        ("yarn", "yarn.lock"),
        ("bun", "bun.lock"),
        ("bun", "bun.lockb"),
        ("npm", "npm-shrinkwrap.json"),
        ("npm", "package-lock.json"),
    ]
    .into_iter()
    .find(|(_, path)| git.root.join(path).is_file())
    .unwrap_or(("npm", ""));
    let mut sources = vec![discovery_source(
        &git.root,
        "package.json",
        "JavaScript package scripts",
    )?];
    if !lockfile.is_empty() {
        sources.push(discovery_source(
            &git.root,
            lockfile,
            "JavaScript locked dependency graph and package-manager selection",
        )?);
    }
    let mut checks = Vec::new();
    let mut ids = BTreeSet::new();
    for (script, command_text) in package.scripts {
        let Some(kind) = javascript_script_kind(&script, &command_text) else {
            continue;
        };
        let mut id = format!("js-{}", portable_id_fragment(&script));
        if !ids.insert(id.clone()) {
            id = format!("{}-{}", id, &sha256_bytes(script.as_bytes())[..8]);
            ids.insert(id.clone());
        }
        checks.push(CheckDefinition {
            id,
            label: format!("JavaScript package script: {script}"),
            kind,
            command: CommandSpec {
                program: manager.to_owned(),
                args: vec!["run".to_owned(), script.clone()],
                working_directory: ".".to_owned(),
            },
            reason: format!("package.json declares the verification script {script}"),
            sources: sources.clone(),
            confidence: Confidence::High,
            coverage_paths: javascript_coverage(),
            pass_environment: Vec::new(),
            non_hermetic_inputs: vec![
                format!("{manager} installation and dependency store"),
                "package-manager script runners may invoke a platform shell".to_owned(),
                "host operating system and services used by checks".to_owned(),
            ],
            replaces_workflow_steps: Vec::new(),
        });
    }
    Ok(checks)
}

fn javascript_script_kind(script: &str, command_text: &str) -> Option<CheckKind> {
    let name = script.to_ascii_lowercase();
    let command = command_text.to_ascii_lowercase();
    if ["watch", "dev", "serve", "start", "update", "snapshot"]
        .iter()
        .any(|marker| name.split([':', '-', '_']).any(|part| part == *marker))
        || ["--watch", "--fix", "--write"]
            .iter()
            .any(|marker| command.contains(marker))
    {
        return None;
    }
    let base = name.split(':').next().unwrap_or(&name);
    if matches!(base, "typecheck" | "type-check" | "check-types" | "tsc") {
        Some(CheckKind::TypeCheck)
    } else if base == "test" {
        Some(CheckKind::Test)
    } else if base == "lint" {
        Some(CheckKind::Lint)
    } else if base == "build" {
        Some(CheckKind::Build)
    } else if (base == "format" || base == "fmt")
        && (name.contains("check") || command.contains("--check"))
    {
        Some(CheckKind::Format)
    } else {
        None
    }
}

fn javascript_coverage() -> Vec<String> {
    [
        "**/*.js",
        "**/*.jsx",
        "**/*.mjs",
        "**/*.cjs",
        "**/*.ts",
        "**/*.tsx",
        "package.json",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "tsconfig*.json",
        ".github/workflows/**",
        ".taskattest.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn discover_python(git: &GitContext) -> Result<Vec<CheckDefinition>, TaskError> {
    let mut evidence = PythonToolEvidence::default();
    let mut sources = Vec::new();
    for path in [
        "pyproject.toml",
        "requirements.txt",
        "requirements-dev.txt",
        "tox.ini",
    ] {
        if !git.root.join(path).is_file() {
            continue;
        }
        let text = read_bounded_project_file(&git.root.join(path), path)?;
        if path == "pyproject.toml" {
            let document: toml::Value = toml::from_str(&text)
                .map_err(|error| TaskError::discovery(format!("parse pyproject.toml: {error}")))?;
            collect_pyproject_evidence(&document, &mut evidence);
        } else if path.starts_with("requirements") {
            for line in text.lines() {
                let requirement = line.split('#').next().unwrap_or("").trim();
                if let Some(distribution) = python_distribution_name(requirement) {
                    evidence.record_distribution(&distribution);
                }
            }
        } else if path == "tox.ini" {
            evidence.tox = true;
        }
        sources.push(discovery_source(
            &git.root,
            path,
            "Python project and verification configuration",
        )?);
    }
    let coverage = python_coverage();
    let mut checks = Vec::new();
    if evidence.pytest {
        checks.push(language_check(
            "python-test",
            "Python tests",
            CheckKind::Test,
            "python3",
            &["-m", "pytest"],
            "Python project configuration declares pytest",
            &sources,
            &coverage,
            &[
                "Python interpreter and installed environment",
                "host operating system and services used by tests",
            ],
        ));
    } else if evidence.tox {
        checks.push(language_check(
            "python-test",
            "Python tox environments",
            CheckKind::Test,
            "python3",
            &["-m", "tox"],
            "tox.ini declares Python test environments",
            &sources,
            &coverage,
            &["Python interpreter, tox, and installed environments"],
        ));
    }
    if evidence.ruff {
        checks.push(language_check(
            "python-ruff",
            "Python Ruff lints",
            CheckKind::Lint,
            "python3",
            &["-m", "ruff", "check", "."],
            "Python project configuration declares Ruff",
            &sources,
            &coverage,
            &["Python interpreter and installed environment"],
        ));
    }
    if evidence.mypy {
        checks.push(language_check(
            "python-mypy",
            "Python mypy type checking",
            CheckKind::TypeCheck,
            "python3",
            &["-m", "mypy", "."],
            "Python project configuration declares mypy",
            &sources,
            &coverage,
            &["Python interpreter, type stubs, and installed environment"],
        ));
    }
    Ok(checks)
}

#[derive(Default)]
struct PythonToolEvidence {
    pytest: bool,
    tox: bool,
    ruff: bool,
    mypy: bool,
}

impl PythonToolEvidence {
    fn record_distribution(&mut self, distribution: &str) {
        match distribution {
            "pytest" => self.pytest = true,
            "tox" => self.tox = true,
            "ruff" => self.ruff = true,
            "mypy" => self.mypy = true,
            _ if distribution.starts_with("pytest-") => self.pytest = true,
            _ => {}
        }
    }
}

fn collect_pyproject_evidence(document: &toml::Value, evidence: &mut PythonToolEvidence) {
    let Some(root) = document.as_table() else {
        return;
    };
    if let Some(project) = root.get("project").and_then(toml::Value::as_table) {
        collect_requirement_array(project.get("dependencies"), evidence);
        if let Some(groups) = project
            .get("optional-dependencies")
            .and_then(toml::Value::as_table)
        {
            for requirements in groups.values() {
                collect_requirement_array(Some(requirements), evidence);
            }
        }
    }
    if let Some(groups) = root
        .get("dependency-groups")
        .and_then(toml::Value::as_table)
    {
        for requirements in groups.values() {
            collect_requirement_array(Some(requirements), evidence);
        }
    }
    let Some(tool) = root.get("tool").and_then(toml::Value::as_table) else {
        return;
    };
    evidence.pytest |= tool.contains_key("pytest");
    evidence.ruff |= tool.contains_key("ruff");
    evidence.mypy |= tool.contains_key("mypy");
    evidence.tox |= tool.contains_key("tox");

    if let Some(poetry) = tool.get("poetry").and_then(toml::Value::as_table) {
        collect_dependency_table(poetry.get("dependencies"), evidence);
        collect_dependency_table(poetry.get("dev-dependencies"), evidence);
        if let Some(groups) = poetry.get("group").and_then(toml::Value::as_table) {
            for group in groups.values().filter_map(toml::Value::as_table) {
                collect_dependency_table(group.get("dependencies"), evidence);
            }
        }
    }
    if let Some(uv) = tool.get("uv").and_then(toml::Value::as_table) {
        collect_requirement_array(uv.get("dev-dependencies"), evidence);
    }
    if let Some(pdm) = tool.get("pdm").and_then(toml::Value::as_table) {
        if let Some(groups) = pdm.get("dev-dependencies").and_then(toml::Value::as_table) {
            for requirements in groups.values() {
                collect_requirement_array(Some(requirements), evidence);
            }
        }
    }
    if let Some(hatch) = tool.get("hatch").and_then(toml::Value::as_table) {
        if let Some(environments) = hatch.get("envs").and_then(toml::Value::as_table) {
            for environment in environments.values().filter_map(toml::Value::as_table) {
                collect_requirement_array(environment.get("dependencies"), evidence);
            }
        }
    }
}

fn collect_requirement_array(value: Option<&toml::Value>, evidence: &mut PythonToolEvidence) {
    let Some(requirements) = value.and_then(toml::Value::as_array) else {
        return;
    };
    for requirement in requirements.iter().filter_map(toml::Value::as_str) {
        if let Some(distribution) = python_distribution_name(requirement) {
            evidence.record_distribution(&distribution);
        }
    }
}

fn collect_dependency_table(value: Option<&toml::Value>, evidence: &mut PythonToolEvidence) {
    let Some(dependencies) = value.and_then(toml::Value::as_table) else {
        return;
    };
    for distribution in dependencies.keys() {
        evidence.record_distribution(&normalize_python_distribution(distribution));
    }
}

fn python_distribution_name(requirement: &str) -> Option<String> {
    let requirement = requirement.trim();
    if requirement.is_empty() || requirement.starts_with(['-', '.', '/', '\\']) {
        return None;
    }
    let name_source = if let Some((distribution, reference)) = requirement.split_once('@') {
        let reference = reference.trim();
        if reference.contains("://") || reference.starts_with("git+") {
            distribution.trim()
        } else {
            requirement
        }
    } else if requirement.contains("://") || requirement.starts_with("git+") {
        return None;
    } else {
        requirement
    };
    let name = name_source
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || "-_.".contains(*character))
        .collect::<String>();
    (!name.is_empty()).then(|| normalize_python_distribution(&name))
}

fn normalize_python_distribution(value: &str) -> String {
    let mut normalized = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else if matches!(character, '-' | '_' | '.') && !separator {
            normalized.push('-');
            separator = true;
        }
    }
    normalized.trim_matches('-').to_owned()
}

fn python_coverage() -> Vec<String> {
    [
        "**/*.py",
        "**/*.pyi",
        "pyproject.toml",
        "requirements*.txt",
        "tox.ini",
        "setup.cfg",
        ".github/workflows/**",
        ".taskattest.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn discover_go(git: &GitContext) -> Result<Vec<CheckDefinition>, TaskError> {
    let mut sources = vec![discovery_source(&git.root, "go.mod", "Go module manifest")?];
    if git.root.join("go.sum").is_file() {
        sources.push(discovery_source(
            &git.root,
            "go.sum",
            "Go dependency checksums",
        )?);
    }
    let coverage = go_coverage();
    Ok(vec![
        language_check(
            "go-test",
            "Go tests",
            CheckKind::Test,
            "go",
            &["test", "./..."],
            "go.mod declares a Go module",
            &sources,
            &coverage,
            &[
                "Go module and build caches",
                "host operating system and services used by tests",
            ],
        ),
        language_check(
            "go-vet",
            "Go vet analysis",
            CheckKind::Lint,
            "go",
            &["vet", "./..."],
            "go.mod declares packages analyzable by go vet",
            &sources,
            &coverage,
            &["Go module and build caches"],
        ),
        language_check(
            "go-build",
            "Go build",
            CheckKind::Build,
            "go",
            &["build", "./..."],
            "go.mod declares buildable Go packages",
            &sources,
            &coverage,
            &[
                "Go module and build caches",
                "host linker and native libraries",
            ],
        ),
    ])
}

fn go_coverage() -> Vec<String> {
    [
        "**/*.go",
        "go.mod",
        "go.sum",
        "go.work",
        ".github/workflows/**",
        ".taskattest.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[allow(clippy::too_many_arguments)]
fn language_check(
    id: &str,
    label: &str,
    kind: CheckKind,
    program: &str,
    args: &[&str],
    reason: &str,
    sources: &[DiscoverySource],
    coverage_paths: &[String],
    non_hermetic_inputs: &[&str],
) -> CheckDefinition {
    CheckDefinition {
        id: id.to_owned(),
        label: label.to_owned(),
        kind,
        command: CommandSpec {
            program: program.to_owned(),
            args: args.iter().map(|value| (*value).to_owned()).collect(),
            working_directory: ".".to_owned(),
        },
        reason: reason.to_owned(),
        sources: sources.to_vec(),
        confidence: Confidence::High,
        coverage_paths: coverage_paths.to_vec(),
        pass_environment: Vec::new(),
        non_hermetic_inputs: non_hermetic_inputs
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        replaces_workflow_steps: Vec::new(),
    }
}

fn portable_id_fragment(value: &str) -> String {
    let mut result = String::new();
    let mut previous_separator = false;
    for byte in value.bytes() {
        let byte = byte.to_ascii_lowercase();
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            result.push(char::from(byte));
            previous_separator = false;
        } else if !previous_separator {
            result.push('-');
            previous_separator = true;
        }
        if result.len() == 55 {
            break;
        }
    }
    let result = result.trim_matches('-');
    if result.is_empty() {
        sha256_bytes(value.as_bytes())[..12].to_owned()
    } else {
        result.to_owned()
    }
}

fn read_bounded_project_file(path: &Path, label: &str) -> Result<String, TaskError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| TaskError::io("inspect project file", path, error))?;
    if metadata.len() > MAX_PROJECT_FILE_BYTES {
        return Err(TaskError::discovery(format!(
            "{label} exceeds the 1 MiB safety bound"
        )));
    }
    std::fs::read_to_string(path).map_err(|error| TaskError::io("read project file", path, error))
}

fn discover_rust(git: &GitContext) -> Result<Vec<CheckDefinition>, TaskError> {
    let cargo_source = discovery_source(
        &git.root,
        "Cargo.toml",
        "Rust package or workspace manifest",
    )?;
    let lock_source = git
        .root
        .join("Cargo.lock")
        .is_file()
        .then(|| discovery_source(&git.root, "Cargo.lock", "locked Rust dependency graph"))
        .transpose()?;
    let mut sources = vec![cargo_source];
    if let Some(lock_source) = lock_source {
        sources.push(lock_source);
    }
    let locked = git.root.join("Cargo.lock").is_file();
    let common_coverage = vec![
        "**/*.rs".to_owned(),
        "Cargo.toml".to_owned(),
        "Cargo.lock".to_owned(),
        ".cargo/**".to_owned(),
        ".github/workflows/**".to_owned(),
        ".taskattest.toml".to_owned(),
    ];
    let cargo_args = |mut args: Vec<&str>| -> Vec<String> {
        if locked {
            args.push("--locked");
        }
        args.into_iter().map(str::to_owned).collect()
    };
    Ok(vec![
        CheckDefinition {
            id: "rust-format".to_owned(),
            label: "Rust formatting".to_owned(),
            kind: CheckKind::Format,
            command: CommandSpec {
                program: "cargo".to_owned(),
                args: vec!["fmt".to_owned(), "--check".to_owned()],
                working_directory: ".".to_owned(),
            },
            reason: "Cargo.toml declares a Rust workspace; rustfmt is the canonical formatter"
                .to_owned(),
            sources: sources.clone(),
            confidence: Confidence::High,
            coverage_paths: common_coverage.clone(),
            pass_environment: Vec::new(),
            non_hermetic_inputs: vec!["installed rustfmt component".to_owned()],
            replaces_workflow_steps: Vec::new(),
        },
        CheckDefinition {
            id: "rust-lint".to_owned(),
            label: "Rust clippy lints".to_owned(),
            kind: CheckKind::Lint,
            command: CommandSpec {
                program: "cargo".to_owned(),
                args: {
                    let mut args = cargo_args(vec!["clippy", "--all-targets"]);
                    args.extend(["--".to_owned(), "-D".to_owned(), "warnings".to_owned()]);
                    args
                },
                working_directory: ".".to_owned(),
            },
            reason: "Cargo.toml declares Rust targets and clippy can check every target".to_owned(),
            sources: sources.clone(),
            confidence: Confidence::High,
            coverage_paths: common_coverage.clone(),
            pass_environment: Vec::new(),
            non_hermetic_inputs: vec![
                "installed clippy component".to_owned(),
                "Cargo registry and cache state".to_owned(),
            ],
            replaces_workflow_steps: Vec::new(),
        },
        CheckDefinition {
            id: "rust-test".to_owned(),
            label: "Rust tests".to_owned(),
            kind: CheckKind::Test,
            command: CommandSpec {
                program: "cargo".to_owned(),
                args: cargo_args(vec!["test", "--all-targets"]),
                working_directory: ".".to_owned(),
            },
            reason: "Cargo.toml declares Rust targets and cargo test covers their test suites"
                .to_owned(),
            sources: sources.clone(),
            confidence: Confidence::High,
            coverage_paths: common_coverage.clone(),
            pass_environment: Vec::new(),
            non_hermetic_inputs: vec![
                "Cargo registry and cache state".to_owned(),
                "host operating system and services used by tests".to_owned(),
            ],
            replaces_workflow_steps: Vec::new(),
        },
        CheckDefinition {
            id: "rust-build".to_owned(),
            label: "Rust release build".to_owned(),
            kind: CheckKind::Build,
            command: CommandSpec {
                program: "cargo".to_owned(),
                args: cargo_args(vec!["build", "--release"]),
                working_directory: ".".to_owned(),
            },
            reason: "Cargo.toml declares buildable Rust targets".to_owned(),
            sources,
            confidence: Confidence::High,
            coverage_paths: common_coverage,
            pass_environment: Vec::new(),
            non_hermetic_inputs: vec![
                "Cargo registry and cache state".to_owned(),
                "host linker and native libraries".to_owned(),
            ],
            replaces_workflow_steps: Vec::new(),
        },
    ])
}

fn configured_check(
    git: &GitContext,
    configured: ConfiguredCheck,
    config_source: Option<DiscoverySource>,
) -> Result<CheckDefinition, TaskError> {
    if configured.command.is_empty() || configured.command[0].trim().is_empty() {
        return Err(TaskError::configuration(format!(
            "check {} has an empty command array",
            configured.id
        )));
    }
    validate_working_directory(&git.root, &configured.working_directory)?;
    for name in &configured.pass_environment {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(TaskError::configuration(format!(
                "check {} has an invalid environment variable name: {name}",
                configured.id
            )));
        }
    }
    Ok(CheckDefinition {
        id: configured.id,
        label: configured.label,
        kind: configured.kind,
        command: CommandSpec {
            program: configured.command[0].clone(),
            args: configured.command[1..].to_vec(),
            working_directory: configured.working_directory,
        },
        reason: configured.reason,
        sources: config_source.into_iter().collect(),
        confidence: Confidence::Explicit,
        coverage_paths: configured.coverage_paths,
        pass_environment: configured.pass_environment,
        non_hermetic_inputs: configured.non_hermetic_inputs,
        replaces_workflow_steps: configured.replaces_workflow_steps,
    })
}

fn apply_workflow_replacements(
    checks: &[CheckDefinition],
    workflow: &mut WorkflowDiscovery,
) -> Result<(), TaskError> {
    let mut claimed = BTreeMap::new();
    for check in checks {
        for observation_id in &check.replaces_workflow_steps {
            if let Some(previous) = claimed.insert(observation_id.clone(), check.id.clone()) {
                return Err(TaskError::configuration(format!(
                    "workflow step {observation_id} is replaced by both {previous} and {}",
                    check.id
                )));
            }
            let observation = workflow
                .observations
                .iter_mut()
                .find(|observation| observation.id == *observation_id)
                .ok_or_else(|| {
                    TaskError::configuration(format!(
                        "check {} replaces an unknown workflow step: {observation_id}",
                        check.id
                    ))
                })?;
            if !matches!(
                observation.classification,
                WorkflowClassification::UnmodeledVerification
            ) {
                return Err(TaskError::configuration(format!(
                    "check {} can only replace an unmodeled workflow step: {observation_id}",
                    check.id
                )));
            }
            observation.classification = WorkflowClassification::ReplacedByExplicitCheck;
            observation.check_id = Some(check.id.clone());
            observation.reason = format!(
                "explicit check {} declares an argv-based replacement for this workflow step",
                check.id
            );
        }
    }
    workflow.coverage_gaps.retain(|gap| {
        !claimed
            .keys()
            .any(|observation_id| gap.ends_with(observation_id))
    });
    Ok(())
}

fn validate_checks(checks: &[CheckDefinition]) -> Result<(), TaskError> {
    let mut ids = BTreeSet::new();
    for check in checks {
        if !valid_check_id(&check.id) {
            return Err(TaskError::configuration(format!(
                "invalid check id {}; use 1-64 lowercase letters, digits, hyphens, or underscores",
                check.id
            )));
        }
        if !ids.insert(&check.id) {
            return Err(TaskError::configuration(format!(
                "duplicate check id: {}",
                check.id
            )));
        }
        compile_coverage(&check.coverage_paths)?;
    }
    Ok(())
}

fn valid_check_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn validate_working_directory(root: &Path, relative: &str) -> Result<(), TaskError> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(TaskError::configuration(format!(
            "working directory must stay inside the repository: {relative}"
        )));
    }
    let resolved = root.join(candidate).canonicalize().map_err(|error| {
        TaskError::configuration(format!("resolve working directory {relative}: {error}"))
    })?;
    if !resolved.starts_with(root) || !resolved.is_dir() {
        return Err(TaskError::configuration(format!(
            "working directory must resolve to a repository directory: {relative}"
        )));
    }
    Ok(())
}

fn compile_coverage(patterns: &[String]) -> Result<GlobSet, TaskError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            TaskError::configuration(format!("invalid coverage pattern {pattern}: {error}"))
        })?);
    }
    builder
        .build()
        .map_err(|error| TaskError::configuration(format!("build coverage matcher: {error}")))
}

fn select_check(
    check: &CheckDefinition,
    matcher: &GlobSet,
    changed_paths: &[String],
    changed_only: bool,
    requested: &BTreeSet<String>,
) -> CheckSelection {
    if !requested.is_empty() {
        let selected = requested.contains(&check.id);
        return CheckSelection {
            check_id: check.id.clone(),
            selected,
            reason: if selected {
                "explicitly requested".to_owned()
            } else {
                "omitted by explicit check selection".to_owned()
            },
        };
    }
    if !changed_only {
        return CheckSelection {
            check_id: check.id.clone(),
            selected: true,
            reason: "full verification selects every discovered check".to_owned(),
        };
    }
    if changed_paths.is_empty() {
        return CheckSelection {
            check_id: check.id.clone(),
            selected: true,
            reason: "no dirty paths; baseline verification selects every check".to_owned(),
        };
    }
    let matches: Vec<_> = changed_paths
        .iter()
        .filter(|path| matcher.is_match(path))
        .take(3)
        .cloned()
        .collect();
    CheckSelection {
        check_id: check.id.clone(),
        selected: !matches.is_empty(),
        reason: if matches.is_empty() {
            "no changed path matches declared coverage".to_owned()
        } else {
            format!("changed coverage matched: {}", matches.join(", "))
        },
    }
}

fn coverage_gaps(
    changed_paths: &[String],
    checks: &[CheckDefinition],
    matchers: &[GlobSet],
    selection: &[CheckSelection],
) -> Result<Vec<String>, TaskError> {
    let mut gaps = Vec::new();
    for path in changed_paths {
        if !matchers.iter().enumerate().any(|(index, matcher)| {
            selection
                .get(index)
                .is_some_and(|selection| selection.selected)
                && matcher.is_match(path)
        }) {
            gaps.push(format!("no discovered check declares coverage for {path}"));
        }
    }
    if checks.is_empty() {
        gaps.push("no checks discovered; add a supported manifest or .taskattest.toml".to_owned());
    } else if !selection.iter().any(|selection| selection.selected) {
        gaps.push("no checks were selected".to_owned());
    }
    gaps.sort();
    gaps.dedup();
    Ok(gaps)
}

fn discovery_source(
    root: &Path,
    relative: &str,
    evidence: &str,
) -> Result<DiscoverySource, TaskError> {
    Ok(DiscoverySource {
        path: relative.to_owned(),
        sha256: sha256_path(&root.join(relative))?,
        evidence: evidence.to_owned(),
    })
}

pub fn selected_checks(report: &DiscoveryReport) -> Vec<CheckDefinition> {
    let selected: BTreeMap<_, _> = report
        .selection
        .iter()
        .map(|selection| (&selection.check_id, selection.selected))
        .collect();
    report
        .checks
        .iter()
        .filter(|check| selected.get(&check.id).copied().unwrap_or(false))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_id_rules_are_bounded() {
        assert!(valid_check_id("rust-test"));
        assert!(!valid_check_id("Rust-Test"));
        assert!(!valid_check_id("-leading"));
        assert!(!valid_check_id(""));
        assert_eq!(portable_id_fragment("TypeCheck"), "typecheck");
        assert_eq!(portable_id_fragment("検証").len(), 12);
    }

    #[test]
    fn coverage_globs_match_rust_paths() {
        let matcher = compile_coverage(&["**/*.rs".to_owned(), "Cargo.toml".to_owned()])
            .expect("compile coverage");
        assert!(matcher.is_match("src/lib.rs"));
        assert!(matcher.is_match("Cargo.toml"));
        assert!(!matcher.is_match("README.md"));
    }

    #[test]
    fn safe_workflow_command_accepts_only_single_argv_commands() {
        let (_, command) = safe_workflow_command("cargo +1.85.0 check --all-targets --locked", ".")
            .expect("parse workflow command")
            .expect("safe command");
        assert_eq!(command.program, "cargo");
        assert_eq!(
            command.args,
            ["+1.85.0", "check", "--all-targets", "--locked"]
        );
        assert!(
            safe_workflow_command("cargo test --locked > result.txt", ".")
                .expect("classify shell command")
                .is_none()
        );
        assert!(
            safe_workflow_command("rm -rf target", ".")
                .expect("classify unsupported command")
                .is_none()
        );
    }

    #[test]
    fn workflow_gap_heuristic_ignores_setup_and_release_delivery() {
        assert!(!looks_like_unmodeled_verification(
            ".github/workflows/ci.yml",
            "quality",
            "Install Rust quality components",
            "rustup component add rustfmt clippy",
        ));
        assert!(!looks_like_unmodeled_verification(
            ".github/workflows/release.yml",
            "publish",
            "Create GitHub release",
            "gh release create --verify-tag",
        ));
        assert!(looks_like_unmodeled_verification(
            ".github/workflows/ci.yml",
            "test",
            "Validate JSON fixtures",
            "python3 - <<'PY'\nimport json\nPY",
        ));
        assert!(looks_like_unmodeled_verification(
            ".github/workflows/ci.yml",
            "test",
            "Require subtitle capability on Linux",
            "python3 - <<'PY'\nraise SystemExit(1)\nPY",
        ));
    }

    #[test]
    fn python_evidence_uses_declared_tools_not_substrings() {
        let document: toml::Value = toml::from_str(
            r#"
[project]
name = "scruffy"
version = "0.1.0"
description = "The words pytest, Ruff, and mypy are documentation, not tool declarations"
dependencies = ["mypy-boto3-s3>=1", "scruffy>=0.3"]

[build-system]
requires = ["ruff-build-helper>=1"]
"#,
        )
        .expect("parse fixture");
        let mut evidence = PythonToolEvidence::default();
        collect_pyproject_evidence(&document, &mut evidence);
        assert!(!evidence.pytest);
        assert!(!evidence.ruff);
        assert!(!evidence.mypy);
        assert!(!evidence.tox);
    }

    #[test]
    fn python_evidence_covers_standard_and_common_dependency_groups() {
        let document: toml::Value = toml::from_str(
            r#"
[project]
name = "fixture"
version = "0.1.0"
optional-dependencies.test = ["pytest-cov>=5"]

[dependency-groups]
quality = ["ruff>=0.5"]

[tool.poetry.group.types.dependencies]
mypy = "^1.10"

[tool.poetry.dev-dependencies]
pytest = "^8"

[tool.tox]
legacy_tox_ini = """
[testenv]
commands = pytest
"""
"#,
        )
        .expect("parse fixture");
        let mut evidence = PythonToolEvidence::default();
        collect_pyproject_evidence(&document, &mut evidence);
        assert!(evidence.pytest);
        assert!(evidence.ruff);
        assert!(evidence.mypy);
        assert!(evidence.tox);
    }

    #[test]
    fn python_requirement_names_are_pep503_normalized() {
        assert_eq!(
            python_distribution_name("PyTest_Cov[all]>=5; python_version >= '3.10'"),
            Some("pytest-cov".to_owned())
        );
        assert_eq!(
            python_distribution_name("ruff @ https://example.invalid/ruff.whl"),
            Some("ruff".to_owned())
        );
        assert_eq!(
            python_distribution_name("mypy-boto3-s3>=1"),
            Some("mypy-boto3-s3".to_owned())
        );
        assert_eq!(python_distribution_name("-r base.txt"), None);
        assert_eq!(
            python_distribution_name("https://example.invalid/tool.whl"),
            None
        );
        assert_eq!(
            python_distribution_name("ruff://example.invalid/tool"),
            None
        );
    }
}

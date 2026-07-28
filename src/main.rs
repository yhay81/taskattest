use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use serde::Serialize;
use taskattest::discover::discover_checks;
use taskattest::error::{ErrorDocument, TaskError};
use taskattest::execute::{CancellationToken, install_cancellation_handler, run_checks};
use taskattest::git::GitContext;
use taskattest::model::{
    DiscoveryReport, ExecutionLimits, OutputFormat, Receipt, ReceiptOutcome, VerificationReport,
};
use taskattest::schema::{SchemaDocument, document_schema};
use taskattest::source::{identify_source, write_json_atomic};
use taskattest::store::StateStore;
use taskattest::verify::verify_receipt;

#[derive(Debug, Parser)]
#[command(
    name = "taskattest",
    version,
    about = "Evidence-backed verification receipts for software changes"
)]
struct Cli {
    /// Git workspace to inspect or verify.
    #[arg(long, global = true, default_value = ".")]
    workspace: PathBuf,
    /// Local receipt and log store; defaults to <git-dir>/taskattest.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Result encoding. NDJSON progress is written to stderr.
    #[arg(long, global = true, value_enum, default_value_t)]
    format: OutputFormat,
    /// Suppress progress while retaining the final result.
    #[arg(long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Emit the brief contract or a full JSON Schema document.
    Schema {
        /// Contract document to emit.
        #[arg(long, value_enum, default_value = "brief")]
        document: SchemaDocument,
    },
    /// Discover checks and explain selection without executing them.
    Discover {
        /// Select checks whose coverage matches dirty workspace paths.
        #[arg(long, conflicts_with = "check")]
        changed: bool,
        /// Select an exact discovered check ID; repeatable.
        #[arg(long = "check")]
        check: Vec<String>,
    },
    /// Execute selected checks and persist an integrity-bound receipt.
    Run {
        /// Select checks whose coverage matches dirty workspace paths.
        #[arg(long, conflicts_with = "check")]
        changed: bool,
        /// Select an exact discovered check ID; repeatable.
        #[arg(long = "check")]
        check: Vec<String>,
        /// Skip remaining selected checks after the first non-pass outcome.
        #[arg(long)]
        fail_fast: bool,
        /// Hard wall-clock limit for each selected check.
        #[arg(
            long,
            default_value_t = 900_000,
            value_parser = clap::value_parser!(u64).range(1..=86_400_000)
        )]
        max_runtime_ms_per_check: u64,
        /// Combined stdout and stderr byte limit for each selected check.
        #[arg(
            long,
            default_value_t = 67_108_864,
            value_parser = clap::value_parser!(u64).range(1..=1_073_741_824)
        )]
        max_log_bytes_per_check: u64,
        /// Also publish the final receipt at a new no-clobber path.
        #[arg(long)]
        receipt_out: Option<PathBuf>,
    },
    /// Read a receipt from the local store.
    Receipt {
        #[command(subcommand)]
        command: ReceiptCommands,
    },
    /// Verify receipt integrity and referenced local logs without rerunning.
    Verify {
        /// Receipt ID in the local store, or a JSON receipt path.
        receipt: String,
    },
    /// Generate a shell completion script.
    Completions {
        /// Target shell.
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Subcommand)]
enum ReceiptCommands {
    /// Show a stored receipt by ID or path.
    Show {
        /// Receipt ID in the local store, or a JSON receipt path.
        receipt: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            emit_error(&error, cli.format);
            ExitCode::from(error.exit_code)
        }
    }
}

fn run(cli: &Cli) -> Result<u8, TaskError> {
    match &cli.command {
        Commands::Schema { document } => {
            emit_json(&document_schema(*document), cli.format)?;
            Ok(0)
        }
        Commands::Completions { shell } => {
            let mut command = Cli::command();
            clap_complete::generate(*shell, &mut command, "taskattest", &mut io::stdout());
            Ok(0)
        }
        Commands::Discover { changed, check } => {
            let git = GitContext::discover(&cli.workspace)?;
            let source = identify_source(&git)?;
            let report = discover_checks(&git, source, *changed, check)?;
            emit_discovery(&report, cli.format)?;
            Ok(0)
        }
        Commands::Run {
            changed,
            check,
            fail_fast,
            max_runtime_ms_per_check,
            max_log_bytes_per_check,
            receipt_out,
        } => {
            let git = GitContext::discover(&cli.workspace)?;
            let source = identify_source(&git)?;
            let discovery = discover_checks(&git, source, *changed, check)?;
            let store = StateStore::create(&git, cli.state_dir.as_deref())?;
            let cancellation = CancellationToken::new();
            install_cancellation_handler(cancellation.clone())?;
            let invocation = taskattest::model::Invocation {
                changed_only: *changed,
                requested_checks: check.clone(),
                fail_fast: *fail_fast,
                limits: ExecutionLimits {
                    max_runtime_ms_per_check: *max_runtime_ms_per_check,
                    max_log_bytes_per_check: *max_log_bytes_per_check,
                },
            };
            let receipt = run_checks(
                &git,
                &store,
                discovery,
                invocation,
                &cancellation,
                |event| emit_progress(event, cli.format, cli.quiet),
            )?;
            if let Some(path) = receipt_out {
                if path.exists() {
                    return Err(TaskError::execution(format!(
                        "receipt output already exists: {}",
                        path.display()
                    )));
                }
                write_json_atomic(path, &receipt)?;
            }
            emit_receipt(&receipt, cli.format)?;
            Ok(match receipt.payload.outcome {
                ReceiptOutcome::Passed => 0,
                ReceiptOutcome::Failed | ReceiptOutcome::Incomplete | ReceiptOutcome::Cancelled => {
                    1
                }
            })
        }
        Commands::Receipt {
            command: ReceiptCommands::Show { receipt },
        } => {
            let (receipt, _) = load_receipt(cli, receipt)?;
            emit_receipt(&receipt, cli.format)?;
            Ok(0)
        }
        Commands::Verify { receipt } => {
            let git = GitContext::discover(&cli.workspace)?;
            let store_path = cli
                .state_dir
                .clone()
                .unwrap_or_else(|| git.git_dir.join("taskattest"));
            let store = StateStore::open_existing(&store_path)?;
            let receipt = if looks_like_path(receipt) {
                StateStore::read_receipt_file(Path::new(receipt))?
            } else {
                store.read_receipt(receipt)?.0
            };
            let report = verify_receipt(&receipt, &store)?;
            emit_verification(&report, cli.format)?;
            Ok(if report.valid { 0 } else { 1 })
        }
    }
}

fn load_receipt(cli: &Cli, value: &str) -> Result<(Receipt, PathBuf), TaskError> {
    if looks_like_path(value) {
        let path = PathBuf::from(value);
        return Ok((StateStore::read_receipt_file(&path)?, path));
    }
    let git = GitContext::discover(&cli.workspace)?;
    let store_path = cli
        .state_dir
        .clone()
        .unwrap_or_else(|| git.git_dir.join("taskattest"));
    let store = StateStore::open_existing(&store_path)?;
    store.read_receipt(value)
}

fn looks_like_path(value: &str) -> bool {
    let path = Path::new(value);
    path.components().count() > 1
        || path
            .extension()
            .is_some_and(|extension| extension == "json")
}

fn emit_discovery(report: &DiscoveryReport, format: OutputFormat) -> Result<(), TaskError> {
    match format {
        OutputFormat::Human => {
            println!(
                "{} checks discovered; {} selected; {} coverage gaps",
                report.checks.len(),
                report
                    .selection
                    .iter()
                    .filter(|selection| selection.selected)
                    .count(),
                report.coverage_gaps.len()
            );
            for selection in &report.selection {
                println!(
                    "{} {} — {}",
                    if selection.selected { "select" } else { "omit" },
                    selection.check_id,
                    selection.reason
                );
            }
            for gap in &report.coverage_gaps {
                println!("coverage gap: {gap}");
            }
            Ok(())
        }
        OutputFormat::Json | OutputFormat::Ndjson => emit_json(report, format),
    }
}

fn emit_receipt(receipt: &Receipt, format: OutputFormat) -> Result<(), TaskError> {
    match format {
        OutputFormat::Human => {
            println!(
                "{} {:?}: {} checks, {} coverage gaps",
                receipt.receipt_id,
                receipt.payload.outcome,
                receipt.payload.checks.len(),
                receipt.payload.coverage_gaps.len()
            );
            for check in &receipt.payload.checks {
                println!(
                    "{:?} {} ({} ms)",
                    check.outcome, check.check.id, check.duration_ms
                );
            }
            Ok(())
        }
        OutputFormat::Json | OutputFormat::Ndjson => emit_json(receipt, format),
    }
}

fn emit_verification(report: &VerificationReport, format: OutputFormat) -> Result<(), TaskError> {
    match format {
        OutputFormat::Human => {
            println!(
                "{}: {} ({} blobs)",
                report.receipt_id,
                if report.valid { "valid" } else { "invalid" },
                report.blobs.len()
            );
            for problem in &report.problems {
                println!("problem: {problem}");
            }
            Ok(())
        }
        OutputFormat::Json | OutputFormat::Ndjson => emit_json(report, format),
    }
}

fn emit_progress(event: &taskattest::model::ProgressEvent, format: OutputFormat, quiet: bool) {
    if quiet || matches!(format, OutputFormat::Json) {
        return;
    }
    match format {
        OutputFormat::Human => {
            let subject = event
                .check_id
                .as_deref()
                .or(event.receipt_id.as_deref())
                .unwrap_or("-");
            eprintln!("{:?} {subject}", event.state);
        }
        OutputFormat::Ndjson => {
            if let Ok(line) = serde_json::to_string(event) {
                println!("{line}");
            }
        }
        OutputFormat::Json => {}
    }
}

fn emit_json(value: &impl Serialize, format: OutputFormat) -> Result<(), TaskError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    match format {
        OutputFormat::Ndjson => serde_json::to_writer(&mut handle, value)?,
        OutputFormat::Human | OutputFormat::Json => {
            serde_json::to_writer_pretty(&mut handle, value)?
        }
    }
    handle
        .write_all(b"\n")
        .map_err(|error| TaskError::new("output_failed", error.to_string(), 6))
}

fn emit_error(error: &TaskError, format: OutputFormat) {
    match format {
        OutputFormat::Human => eprintln!("{}: {}", error.code, error.message),
        OutputFormat::Json => {
            let _ = serde_json::to_writer_pretty(io::stderr(), &ErrorDocument::from(error));
            eprintln!();
        }
        OutputFormat::Ndjson => {
            let _ = serde_json::to_writer(io::stderr(), &ErrorDocument::from(error));
            eprintln!();
        }
    }
}

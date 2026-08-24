//! `opy-cli` — the standalone Workshop-independent OPY CLI.
//!
//! The CLI owns command parsing and presentation. `opy-rs` owns OPY
//! parsing, semantic resolution, and structured diagnostics.
//!
//! Exit codes remain: 0 clean/success, 1 source diagnostics, and 2 usage or
//! I/O errors. Machine-readable output is written directly to stdout without
//! passing through human or GitHub Actions presentation.

mod cli;
mod present;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use clap_complete::{generate, shells};
use opy_rs::support::{self, SupportMatrixError};
use opy_rs::tooling::{CheckOutcome, Diagnostic as OpyDiagnostic, check};
use opy_rs::{LANGUAGE_NAME, LANGUAGE_VERSION};
use serde::Serialize;

use crate::cli::{CheckArgs, Cli, Command, FileArgs, OutputFormatArg, SupportArgs};
use crate::present::{
    CheckView, DiagnosticSeverity, DiagnosticView, PositionView, Presentation, SpanView,
};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                print!("{error}");
                return ExitCode::SUCCESS;
            }
            _ => {
                if error.kind() == ErrorKind::InvalidSubcommand {
                    eprintln!("opy-cli: unknown command");
                }
                eprint!("{error}");
                return ExitCode::from(2);
            }
        },
    };

    let presentation = Presentation::from_cli(cli.renderer, cli.color);
    if cli.version {
        return cmd_version();
    }
    match cli.command {
        None => {
            eprintln!("{}", Cli::command().render_help());
            ExitCode::from(2)
        }
        Some(Command::Check(args)) => cmd_check(&args, presentation),
        Some(Command::Inspect(args)) => cmd_inspect(&args, presentation),
        Some(Command::Support(args)) => cmd_support(&args),
        Some(Command::Completion(args)) => cmd_completion(args.shell),
        Some(Command::Help) => {
            print!("{}", Cli::command().render_help());
            ExitCode::SUCCESS
        }
        Some(Command::Version) => cmd_version(),
    }
}

fn read_main(args: &FileArgs) -> Result<(String, PathBuf, PathBuf), ExitCode> {
    let main = &args.main;
    match std::fs::read_to_string(main) {
        Ok(text) => {
            let root = main
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Ok((text, root.to_path_buf(), main.clone()))
        }
        Err(error) => {
            eprintln!("opy-cli: cannot read '{}': {error}", main.display());
            Err(ExitCode::from(2))
        }
    }
}

fn cmd_check(args: &CheckArgs, presentation: Presentation) -> ExitCode {
    let (text, root, main) = match read_main(&args.file) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let outcome = check(&text, &main.to_string_lossy(), &root);
    if args.format == OutputFormatArg::Json {
        return match print_json(&CheckReport {
            ok: outcome.is_clean(),
            diagnostics: &outcome.diagnostics,
        }) {
            Ok(()) => diagnostic_exit(&outcome),
            Err(code) => code,
        };
    }

    let code = diagnostic_exit(&outcome);
    presentation.render_check(&check_view(&outcome));
    code
}

fn cmd_inspect(args: &FileArgs, presentation: Presentation) -> ExitCode {
    let (text, root, main) = match read_main(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let outcome = check(&text, &main.to_string_lossy(), &root);
    if !outcome.is_clean() {
        let diagnostics = outcome
            .diagnostics
            .iter()
            .map(diagnostic_view)
            .collect::<Vec<_>>();
        presentation.render_diagnostics("inspect", &diagnostics);
        return ExitCode::from(1);
    }
    let model = outcome
        .model
        .as_ref()
        .expect("a clean check produces a model");
    match print_json(model) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn cmd_support(args: &SupportArgs) -> ExitCode {
    let _json_flag_is_accepted_for_compatibility = args.json;
    let matrix = match support::SupportMatrix::builtin() {
        Ok(matrix) => matrix,
        Err(error) => return matrix_error_exit(error),
    };
    let value = match args.filter.as_deref() {
        None => serde_json::to_value(matrix).expect("the matrix is serializable"),
        Some(filter) => {
            if let Some(feature) = matrix.feature(filter) {
                serde_json::to_value(feature).expect("a feature is serializable")
            } else if matrix
                .categories()
                .iter()
                .any(|category| category == filter)
            {
                let features = matrix.features_by_category(filter);
                serde_json::json!({
                    "category": filter,
                    "count": features.len(),
                    "features": features,
                })
            } else {
                eprintln!(
                    "opy-cli: unknown feature id or category '{filter}' \
                     (see `opy-cli support` for the declared matrix)"
                );
                return ExitCode::from(2);
            }
        }
    };
    match print_json(&value) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn cmd_completion(shell: cli::ShellArg) -> ExitCode {
    let mut command = Cli::command();
    let mut stdout = std::io::stdout();
    match shell {
        cli::ShellArg::Bash => generate(shells::Bash, &mut command, "opy-cli", &mut stdout),
        cli::ShellArg::Zsh => generate(shells::Zsh, &mut command, "opy-cli", &mut stdout),
        cli::ShellArg::Fish => generate(shells::Fish, &mut command, "opy-cli", &mut stdout),
        cli::ShellArg::PowerShell => {
            generate(shells::PowerShell, &mut command, "opy-cli", &mut stdout)
        }
    }
    ExitCode::SUCCESS
}

fn cmd_version() -> ExitCode {
    println!("opy-cli {}", env!("CARGO_PKG_VERSION"));
    println!("language: {LANGUAGE_NAME} {LANGUAGE_VERSION}");
    println!(
        "protocol: {} v{}",
        opy_rs::hir::types::PROTOCOL_NAME,
        opy_rs::hir::types::PROTOCOL_MAJOR
    );
    ExitCode::SUCCESS
}

#[derive(Serialize)]
struct CheckReport<'a> {
    ok: bool,
    diagnostics: &'a [OpyDiagnostic],
}

fn check_view(outcome: &CheckOutcome) -> CheckView {
    CheckView {
        clean: outcome.is_clean(),
        diagnostics: outcome.diagnostics.iter().map(diagnostic_view).collect(),
        file_count: outcome.files.len(),
        declaration_count: outcome
            .model
            .as_ref()
            .map_or(0, |model| model.declarations().len()),
        rule_count: outcome
            .model
            .as_ref()
            .map_or(0, |model| model.rules().len()),
        symbol_count: outcome
            .model
            .as_ref()
            .map_or(0, |model| model.symbols().len()),
    }
}

fn diagnostic_view(diagnostic: &OpyDiagnostic) -> DiagnosticView {
    DiagnosticView {
        severity: DiagnosticSeverity::Error,
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        span: diagnostic.span.as_ref().map(|span| SpanView {
            path: span.path.clone(),
            start: PositionView {
                line: span.start.line,
                col: span.start.col,
            },
            end: PositionView {
                line: span.end.line,
                col: span.end.col,
            },
        }),
    }
}

fn diagnostic_exit(outcome: &CheckOutcome) -> ExitCode {
    if outcome.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), ExitCode> {
    match serde_json::to_string_pretty(value) {
        Ok(rendered) => {
            println!("{rendered}");
            Ok(())
        }
        Err(error) => {
            eprintln!("opy-cli: cannot serialize output: {error}");
            Err(ExitCode::from(2))
        }
    }
}

fn matrix_error_exit(error: SupportMatrixError) -> ExitCode {
    eprintln!("opy-cli: the embedded support matrix is invalid: {error}");
    ExitCode::from(2)
}

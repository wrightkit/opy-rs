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

use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use clap_complete::{generate, shells};
use opy_rs::lookup::{LookupHit, LookupOutcome, LookupQuery, LookupScope, lookup};
use opy_rs::tooling::{CheckOutcome, Diagnostic as OpyDiagnostic, SourceLocation, check};
use opy_rs::{CompileDiagnostic, CompileStatus, Compiler};
use opy_rs::{FilesystemProject, LANGUAGE_NAME, LANGUAGE_VERSION};
use serde::Serialize;

use crate::cli::{
    CheckArgs, Cli, Command, CompileArgs, FileArgs, LookupArgs, LookupScopeArg, OutputFormatArg,
};
use crate::present::{CheckView, DiagnosticView, PositionView, Presentation, SpanView};

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
        Some(Command::Compile(args)) => cmd_compile(&args, presentation),
        Some(Command::Inspect(args)) => cmd_inspect(&args, presentation),
        Some(Command::Lookup(args)) => cmd_lookup(&args),
        Some(Command::Completion(args)) => cmd_completion(args.shell),
        Some(Command::Help) => {
            print!("{}", Cli::command().render_help());
            ExitCode::SUCCESS
        }
        Some(Command::Version) => cmd_version(),
    }
}

fn read_main(args: &FileArgs) -> Result<FilesystemProject, ExitCode> {
    FilesystemProject::load(&args.main).map_err(|error| {
        eprintln!("opy-cli: cannot read '{}': {error}", args.main.display());
        ExitCode::from(2)
    })
}

fn cmd_check(args: &CheckArgs, presentation: Presentation) -> ExitCode {
    let project = match read_main(&args.file) {
        Ok(project) => project,
        Err(code) => return code,
    };
    let outcome = check(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
    );
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

fn cmd_compile(args: &CompileArgs, presentation: Presentation) -> ExitCode {
    let project = match read_main(&args.file) {
        Ok(project) => project,
        Err(code) => return code,
    };
    let compiler = match Compiler::new() {
        Ok(compiler) => compiler,
        Err(error) => {
            eprintln!("opy-cli: cannot initialize compiler: {error}");
            return ExitCode::from(2);
        }
    };
    let report = compiler.compile_source_report_with_language(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
        &args.language,
    );
    if args.format == OutputFormatArg::Json {
        return match print_json(&report) {
            Ok(()) => ExitCode::from(report.compile.exit_code),
            Err(code) => code,
        };
    }

    if report.compile.status == CompileStatus::Success {
        if !report.compile.diagnostics.is_empty() {
            let diagnostics = report
                .compile
                .diagnostics
                .iter()
                .map(compile_diagnostic_view)
                .collect::<Vec<_>>();
            presentation.render_diagnostics("compile", &diagnostics);
        }
        print!("{}", report.compile.workshop_exact);
        ExitCode::SUCCESS
    } else {
        let diagnostics = report
            .compile
            .diagnostics
            .iter()
            .map(compile_diagnostic_view)
            .collect::<Vec<_>>();
        presentation.render_diagnostics("compile", &diagnostics);
        ExitCode::from(report.compile.exit_code)
    }
}

fn cmd_inspect(args: &FileArgs, presentation: Presentation) -> ExitCode {
    let project = match read_main(args) {
        Ok(project) => project,
        Err(code) => return code,
    };
    let outcome = check(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
    );
    if !outcome.is_clean() {
        let diagnostics = outcome
            .diagnostics
            .iter()
            .map(diagnostic_view)
            .collect::<Vec<_>>();
        presentation.render_diagnostics("inspect", &diagnostics);
        return ExitCode::from(1);
    }
    if !outcome.diagnostics.is_empty() {
        let diagnostics = outcome
            .diagnostics
            .iter()
            .map(diagnostic_view)
            .collect::<Vec<_>>();
        presentation.render_diagnostics("inspect", &diagnostics);
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

fn cmd_lookup(args: &LookupArgs) -> ExitCode {
    let scope = if args.scope.is_empty() {
        LookupScope::ALL
    } else {
        args.scope.iter().fold(
            LookupScope {
                functions: false,
                enums: false,
                settings: false,
            },
            |mut scope, arg| {
                match arg {
                    LookupScopeArg::Functions => scope.functions = true,
                    LookupScopeArg::Enums => scope.enums = true,
                    LookupScopeArg::Settings => scope.settings = true,
                }
                scope
            },
        )
    };
    let outcome = lookup(&LookupQuery {
        text: args.query.clone(),
        scope,
        locale: None,
        limit: args.limit,
    });
    let code = match &outcome {
        LookupOutcome::Matched { .. } => ExitCode::SUCCESS,
        LookupOutcome::Unsupported { .. } => ExitCode::from(1),
    };
    if args.format == OutputFormatArg::Json {
        return match print_json(&outcome) {
            Ok(()) => code,
            Err(code) => code,
        };
    }
    match &outcome {
        LookupOutcome::Matched { results, .. } => {
            if results.is_empty() {
                println!("no matches");
            }
            for hit in results {
                println!("{}", lookup_hit_line(hit));
            }
        }
        LookupOutcome::Unsupported { reason, .. } => {
            eprintln!("opy-cli: lookup cannot be answered: {reason}");
        }
    }
    code
}

/// One human-readable line per lookup hit; JSON output is the machine
/// contract, so the text form stays terse.
fn lookup_hit_line(hit: &LookupHit) -> String {
    match hit {
        LookupHit::Function {
            spelling,
            function_kind,
            signature,
            matched_on,
            ..
        } => format!(
            "{} {spelling}  {signature}  [{}]",
            function_kind.as_str(),
            matched_on.as_str()
        ),
        LookupHit::EnumDomain {
            domain,
            members,
            matched_on,
        } => format!(
            "enumDomain {domain}  {}  [{}]",
            members
                .as_ref()
                .map(|members| format!("{} members", members.len()))
                .unwrap_or_else(|| "contextual".to_string()),
            matched_on.as_str()
        ),
        LookupHit::EnumMember {
            spelling,
            display_name,
            matched_on,
            ..
        } => format!(
            "enumMember {spelling}{}  [{}]",
            display_name
                .as_deref()
                .map(|name| format!("  \"{name}\""))
                .unwrap_or_default(),
            matched_on.as_str()
        ),
        LookupHit::Setting {
            path,
            display_name,
            value,
            matched_on,
        } => format!(
            "setting {path}  {}{}  [{}]",
            value.kind.as_str(),
            display_name
                .as_deref()
                .map(|name| format!("  \"{name}\""))
                .unwrap_or_default(),
            matched_on.as_str()
        ),
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
        severity: diagnostic.severity.into(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        span: diagnostic.span.as_ref().map(diagnostic_span_view),
    }
}

fn compile_diagnostic_view(diagnostic: &CompileDiagnostic) -> DiagnosticView {
    DiagnosticView {
        severity: diagnostic.severity.into(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        span: diagnostic.span.as_ref().map(diagnostic_span_view),
    }
}

fn diagnostic_span_view(span: &SourceLocation) -> SpanView {
    SpanView {
        path: span.path.clone(),
        start: PositionView {
            line: span.start.line,
            col: span.start.col,
        },
        end: PositionView {
            line: span.end.line,
            col: span.end.col,
        },
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

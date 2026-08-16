//! `opy-cli` — the standalone Workshop-independent OPY frontend CLI (issue
//! #7).
//!
//! Surfaces the [`opy_frontend::tooling`] library API for frontend
//! validation and inspection without pretending Workshop emission is
//! available:
//!
//! * `opy-cli check <main.opy>` — preprocess/parse/resolve and print
//!   structured diagnostics to stderr; exit 0 clean, 1 on diagnostics.
//! * `opy-cli inspect <main.opy>` — print the resolved program model
//!   (declarations, rules, references, enums) as JSON.
//! * `opy-cli support [--json] [<category|feature-id>]` — print the embedded
//!   compatibility support matrix (or a filtered slice) as JSON.
//! * `opy-cli version` — crate version and frontend protocol identity.
//!
//! Exit codes: 0 clean/success, 1 diagnostics found, 2 usage or I/O errors.
//! Workshop emission and decompilation are deliberately out of scope;
//! lowering-dependent gaps are reported through `support` and documented in
//! `docs/opy/tooling-api.md`.

use std::path::Path;
use std::process::ExitCode;

use opy_frontend::support::{self, SupportMatrixError};
use opy_frontend::tooling::{Diagnostic, check};
use opy_frontend::{FRONTEND_NAME, FRONTEND_VERSION};

const USAGE: &str = "\
opy-cli — Workshop-independent OPY frontend tooling.

Usage:
  opy-cli check <main.opy>                       Parse, preprocess, and resolve; print diagnostics to stderr.
  opy-cli inspect <main.opy>                     Print the resolved program model (declarations, rules, references) as JSON.
  opy-cli support [--json] [<category|feature-id>]  Print the compatibility support matrix (or a filtered slice) as JSON.
  opy-cli version                                Print the crate version and frontend protocol identity.
  opy-cli help                                   Show this help.

Exit codes: 0 clean, 1 diagnostics found, 2 usage or I/O errors.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Some("check") => cmd_check(&args[1..]),
        Some("inspect") => cmd_inspect(&args[1..]),
        Some("support") => cmd_support(&args[1..]),
        Some("version" | "-V" | "--version") => cmd_version(),
        Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("opy-cli: unknown command '{other}'");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// One positional main-file argument; I/O errors are usage-class failures.
fn read_main(args: &[String]) -> Result<(String, String), ExitCode> {
    let Some(main) = args.first() else {
        eprintln!("opy-cli: missing <main.opy> argument");
        return Err(ExitCode::from(2));
    };
    if args.len() > 1 {
        eprintln!("opy-cli: unexpected extra arguments after '{main}'");
        return Err(ExitCode::from(2));
    }
    match std::fs::read_to_string(main) {
        Ok(text) => {
            let root = Path::new(main)
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Ok((text, root.to_string_lossy().into_owned()))
        }
        Err(error) => {
            eprintln!("opy-cli: cannot read '{main}': {error}");
            Err(ExitCode::from(2))
        }
    }
}

fn cmd_check(args: &[String]) -> ExitCode {
    let (text, root) = match read_main(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let main = args[0].clone();
    let outcome = check(&text, &main, Path::new(&root));
    if outcome.is_clean() {
        let model = outcome
            .model
            .as_ref()
            .expect("a clean check produces a model");
        println!(
            "check passed: {} file(s), {} declaration(s), {} rule entry(ies), {} symbol(s)",
            outcome.files.len(),
            model.declarations().len(),
            model.rules().len(),
            model.symbols().len(),
        );
        ExitCode::SUCCESS
    } else {
        for diagnostic in &outcome.diagnostics {
            eprintln!("{}", format_diagnostic(diagnostic));
        }
        ExitCode::from(1)
    }
}

fn cmd_inspect(args: &[String]) -> ExitCode {
    let (text, root) = match read_main(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let main = args[0].clone();
    let outcome = check(&text, &main, Path::new(&root));
    if !outcome.is_clean() {
        for diagnostic in &outcome.diagnostics {
            eprintln!("{}", format_diagnostic(diagnostic));
        }
        return ExitCode::from(1);
    }
    let model = outcome
        .model
        .as_ref()
        .expect("a clean check produces a model");
    print_json(model)
}

fn cmd_support(args: &[String]) -> ExitCode {
    let mut filter: Option<&str> = None;
    for arg in args {
        match arg.as_str() {
            "--json" => {} // Output is always JSON; accepted for explicitness.
            "-h" | "--help" => {
                println!(
                    "Print the embedded compatibility support matrix as JSON.\n\
                     Usage: opy-cli support [--json] [<category|feature-id>]"
                );
                return ExitCode::SUCCESS;
            }
            _ if filter.is_none() => filter = Some(arg),
            _ => {
                eprintln!("opy-cli: unexpected argument '{arg}'");
                return ExitCode::from(2);
            }
        }
    }
    let matrix = match support::SupportMatrix::builtin() {
        Ok(matrix) => matrix,
        Err(error) => return matrix_error_exit(error),
    };
    let value = match filter {
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
    print_json(&value)
}

fn cmd_version() -> ExitCode {
    println!("opy-cli {}", env!("CARGO_PKG_VERSION"));
    println!("frontend: {FRONTEND_NAME} {FRONTEND_VERSION}");
    println!(
        "protocol: {} v{}",
        opy_frontend::hir::types::PROTOCOL_NAME,
        opy_frontend::hir::types::PROTOCOL_MAJOR
    );
    ExitCode::SUCCESS
}

fn print_json<T: serde::Serialize>(value: &T) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(rendered) => {
            println!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("opy-cli: cannot serialize output: {error}");
            ExitCode::from(2)
        }
    }
}

fn matrix_error_exit(error: SupportMatrixError) -> ExitCode {
    eprintln!("opy-cli: the embedded support matrix is invalid: {error}");
    ExitCode::from(2)
}

fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    match &diagnostic.span {
        Some(span) => format!(
            "{}[{}]: {}\n  --> {}:{}:{}",
            diagnostic.severity.as_str(),
            diagnostic.code,
            diagnostic.message,
            span.path,
            span.start.line,
            span.start.col,
        ),
        None => format!(
            "{}[{}]: {}",
            diagnostic.severity.as_str(),
            diagnostic.code,
            diagnostic.message,
        ),
    }
}

//! Internal compatibility comparison producer.
//!
//! This target is built only by the compatibility gate. It is deliberately
//! separate from `opy-cli compile`: pinned oracle input must never become part
//! of the supported compiler API or report schema.

#[path = "opy-compat/probe.rs"]
mod probe;
#[path = "opy-compat/structural.rs"]
mod structural;

use std::path::PathBuf;
use std::process::ExitCode;

use opy_rs::Compiler;
use serde::Serialize;
use serde_json::Value;
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::signatures::ExpectedDomain;

const ALGORITHM: &str = "opy-rs::structural-identity";

#[derive(Debug)]
struct Args {
    source: PathBuf,
    root: PathBuf,
    oracle: PathBuf,
    input_sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityResult {
    schema_version: u32,
    #[serde(rename = "semanticWIR")]
    semantic_wir: SemanticWIRComparison,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SemanticWIRComparison {
    schema_version: u32,
    algorithm: &'static str,
    input_sha256: String,
    reference_input_sha256: String,
    equivalent: bool,
    differences: Vec<structural::Difference>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectComparison {
    schema_version: u32,
    equivalent: bool,
    differences: Vec<structural::Difference>,
}

struct CompatibilityExpectedDomain<'a> {
    catalog: &'a Catalog,
}

impl ExpectedDomain for CompatibilityExpectedDomain<'_> {
    fn expected_domain(&self, catalog_id: &str, arg_index: usize) -> Option<&str> {
        for kind in [Kind::Action, Kind::Value] {
            let Some(entry) = self.catalog.entry(kind, catalog_id) else {
                continue;
            };
            if let Some(domain) = entry.param_domain(arg_index) {
                return Some(domain);
            }
            if let Some(type_name) = entry.param_type(arg_index)
                && self.catalog.enum_domain(type_name).is_some()
            {
                return Some(type_name);
            }
        }
        None
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("probe-generate") => return finish(probe::generate().map(|()| true)),
        Some("probe-compare") => {
            let (Some(probes), Some(references)) = (arguments.next(), arguments.next()) else {
                eprintln!("opy-compat: probe-compare <probes.json> <references.json>");
                return ExitCode::from(2);
            };
            return finish(
                probe::compare(&PathBuf::from(probes), &PathBuf::from(references)).map(|()| true),
            );
        }
        Some("project-compare") => {
            return match project_compare(arguments) {
                Ok(result) => {
                    println!(
                        "{}",
                        serde_json::to_string(&result).expect("comparison serializes")
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("opy-compat: {error}");
                    ExitCode::from(2)
                }
            };
        }
        _ => {}
    }
    match run() {
        Ok(result) => {
            println!(
                "{}",
                serde_json::to_string(&result).expect("compatibility result serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("opy-compat: {error}");
            ExitCode::from(1)
        }
    }
}

fn finish(result: Result<bool, String>) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("opy-compat: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<CompatibilityResult, String> {
    let args = parse_args(std::env::args().skip(1))?;
    let source_text = std::fs::read_to_string(&args.source)
        .map_err(|error| format!("cannot read source '{}': {error}", args.source.display()))?;
    let oracle_text = std::fs::read_to_string(&args.oracle)
        .map_err(|error| format!("cannot read oracle '{}': {error}", args.oracle.display()))?;
    let oracle: Value = serde_json::from_str(&oracle_text)
        .map_err(|error| format!("cannot parse oracle '{}': {error}", args.oracle.display()))?;
    let reference_input_sha256 = oracle["input"]["sha256"]
        .as_str()
        .ok_or_else(|| "oracle input.sha256 is missing".to_string())?;
    let reference_workshop = oracle["compile"]["workshop"]
        .as_str()
        .ok_or_else(|| "oracle compile.workshop is missing".to_string())?;

    let (native_wir, reference_wir) =
        compile_and_parse(&args.source, &args.root, &source_text, reference_workshop)?;
    let differences = structural::differences(&native_wir, &reference_wir);

    Ok(CompatibilityResult {
        schema_version: 1,
        semantic_wir: SemanticWIRComparison {
            schema_version: 1,
            algorithm: ALGORITHM,
            input_sha256: args.input_sha256,
            reference_input_sha256: reference_input_sha256.to_string(),
            equivalent: differences.is_empty(),
            differences,
        },
    })
}

/// `project-compare --source <entry> --root <dir> --reference <workshop>`:
/// compile a project natively and compare it with an already compiled
/// reference output.
fn project_compare<I: Iterator<Item = String>>(mut args: I) -> Result<ProjectComparison, String> {
    let (mut source, mut root, mut reference) = (None, None, None);
    while let Some(argument) = args.next() {
        let target = match argument.as_str() {
            "--source" => &mut source,
            "--root" => &mut root,
            "--reference" => &mut reference,
            other => return Err(format!("unknown argument {other}")),
        };
        *target = Some(PathBuf::from(
            args.next()
                .ok_or_else(|| format!("missing value for {argument}"))?,
        ));
    }
    let (Some(source), Some(root), Some(reference)) = (source, root, reference) else {
        return Err("project-compare needs --source, --root and --reference".to_string());
    };
    let read = |path: &PathBuf| {
        std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read '{}': {error}", path.display()))
    };
    let (native, reference) =
        compile_and_parse(&source, &root, &read(&source)?, &read(&reference)?)?;
    let differences = structural::differences(&native, &reference);
    Ok(ProjectComparison {
        schema_version: 1,
        equivalent: differences.is_empty(),
        differences,
    })
}

/// Compile `source_text` natively and parse both it and the reference output
/// with the same parser, so representation choices in the in-memory program do
/// not count.
fn compile_and_parse(
    source: &std::path::Path,
    root: &std::path::Path,
    source_text: &str,
    reference_workshop: &str,
) -> Result<(workshop_rs::Program, workshop_rs::Program), String> {
    let compiler =
        Compiler::new().map_err(|error| format!("cannot initialize compiler: {error}"))?;
    let artifact = compiler
        .compile_source_with_locale(
            source_text,
            &source.to_string_lossy(),
            root,
            &Locale::new("en-US"),
        )
        .map_err(|error| error.to_string())?;
    let catalog = Catalog::builtin().map_err(|error| error.to_string())?;
    let context = CompatibilityExpectedDomain { catalog: &catalog };
    let parse = |text: &str| {
        workshop_rs::parser::parse_with_context(
            &strip_workshop_comments(text),
            &catalog,
            &Locale::new("en-US"),
            &context,
        )
        .map_err(|error| error.to_string())
    };
    Ok((parse(&artifact.emitted)?, parse(reference_workshop)?))
}

fn strip_workshop_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(character) = chars.next() {
        if in_string {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        if character == '"' {
            in_string = true;
            output.push(character);
        } else if character == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(comment_character) = chars.next() {
                if comment_character == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else if character == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment_character in chars.by_ref() {
                if comment_character == '\n' {
                    output.push('\n');
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }

    output
}

fn parse_args<I>(mut args: I) -> Result<Args, String>
where
    I: Iterator<Item = String>,
{
    let mut source = None;
    let mut root = None;
    let mut oracle = None;
    let mut input_sha256 = None;
    while let Some(argument) = args.next() {
        let value = |name: &str, args: &mut I| {
            args.next()
                .ok_or_else(|| format!("missing value for {name}"))
        };
        match argument.as_str() {
            "--source" => source = Some(PathBuf::from(value("--source", &mut args)?)),
            "--root" => root = Some(PathBuf::from(value("--root", &mut args)?)),
            "--oracle" => oracle = Some(PathBuf::from(value("--oracle", &mut args)?)),
            "--input-sha256" => input_sha256 = Some(value("--input-sha256", &mut args)?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        source: source.ok_or_else(|| "--source is required".to_string())?,
        root: root.ok_or_else(|| "--root is required".to_string())?,
        oracle: oracle.ok_or_else(|| "--oracle is required".to_string())?,
        input_sha256: input_sha256.ok_or_else(|| "--input-sha256 is required".to_string())?,
    })
}

/// Structural identity of two parsed programs; see [`structural`].
fn structurally_identical(native: &workshop_rs::Program, reference: &workshop_rs::Program) -> bool {
    structural::differences(native, reference).is_empty()
}

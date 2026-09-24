//! Internal compatibility comparison producer.
//!
//! This target is built only by the compatibility gate. It is deliberately
//! separate from `opy-cli compile`: pinned oracle input must never become part
//! of the supported compiler API or report schema.

use std::path::PathBuf;
use std::process::ExitCode;

use opy_rs::Compiler;
use serde::Serialize;
use serde_json::Value;
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::signatures::ExpectedDomain;

const ALGORITHM: &str = "workshop-rs::roundtrip::equivalent";

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

    let compiler =
        Compiler::new().map_err(|error| format!("cannot initialize compiler: {error}"))?;
    let artifact = compiler
        .compile_source_with_locale(
            &source_text,
            &args.source.to_string_lossy(),
            &args.root,
            &Locale::new("en-US"),
        )
        .map_err(|error| error.to_string())?;
    let catalog = Catalog::builtin().map_err(|error| error.to_string())?;
    let context = CompatibilityExpectedDomain { catalog: &catalog };
    let reference_wir = workshop_rs::parser::parse_with_context(
        &strip_workshop_comments(reference_workshop),
        &catalog,
        &Locale::new("en-US"),
        &context,
    )
    .map_err(|error| error.to_string())?;

    // Both sides are compared as Workshop text parsed by the same parser, so
    // representation choices in the in-memory program do not count.
    let native_wir = workshop_rs::parser::parse_with_context(
        &strip_workshop_comments(&artifact.emitted),
        &catalog,
        &Locale::new("en-US"),
        &context,
    )
    .map_err(|error| error.to_string())?;

    Ok(CompatibilityResult {
        schema_version: 1,
        semantic_wir: SemanticWIRComparison {
            schema_version: 1,
            algorithm: ALGORITHM,
            input_sha256: args.input_sha256,
            reference_input_sha256: reference_input_sha256.to_string(),
            equivalent: workshop_rs::roundtrip::equivalent(&native_wir, &reference_wir),
        },
    })
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

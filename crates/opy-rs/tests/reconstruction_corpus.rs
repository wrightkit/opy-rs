//! The upstream decompiler corpus gate (`wrightkit/opy-rs#604`).
//!
//! The reconstruction contract's input-coverage bar is the pinned upstream
//! `decompileAllRules` decompiler: the 16 `src/tests/decompiler/*.txt` inputs
//! of `overpy@9.7.17` at content commit
//! `5a7d0e294b8cad73b9701987bb584d0551d7fa4d`, vendored under
//! `tests/fixtures/reconstruction/` (attribution and provenance in its
//! `README.md`).
//!
//! For each input the harness runs the contract pipeline end to end:
//! `workshop_rs::parser` → [`reconstruct`] → OPY compile →
//! `workshop_rs::parser` → `workshop_rs::compare::equivalent`. The recorded
//! outcome for every input lives in `fixtures/reconstruction/classification.json`:
//!
//! - `equivalent` — reconstructs and recompiles to a program behaviorally
//!   equivalent to the input under the relaxed comparator;
//! - `refused` — reconstruction refuses; `codes` records the exact refusal
//!   code set, each drawn from the contract's refusal-code inventory;
//! - `unparseable` — the canonical `workshop_rs::parser` rejects the input
//!   before reconstruction runs. This is a `workshop-rs` boundary gap, not
//!   a reconstruction refusal: `cause` records the rejected surface so the
//!   gap stays visible until the canonical parser accepts it.
//!
//! The gate fails when the actual outcome differs from the checked-in
//! record, when a refused input reports a code outside the contract's
//! inventory, or when a fixture file or classification entry is added or
//! dropped without the other — every coverage change lands in review.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use serde_json::Value;
use sha2::{Digest, Sha256};
use workshop_rs::catalog::{Catalog, Locale};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reconstruction")
}

/// Every refusal code the implementation can emit — the contract's
/// refusal-code inventory in `docs/architecture/reconstruction.md`. A
/// `refused` classification may record only codes from this set; a new code
/// enters the inventory with the contract change that introduces it.
const REFUSAL_CODES: &[&str] = &[
    "manifest-error",
    "catalog-error",
    "unsupported-settings",
    "unsupported-duplicate-name",
    "unsupported-index",
    "unsupported-indexed-initializer",
    "unsupported-rule-order",
    "unsupported-control-flow",
    "unsupported-per-player-loop",
    "unsupported-member-assignment",
    "unsupported-dangling",
    "unsupported-action-call",
    "unsupported-modify-op",
    "unsupported-invalid-arity",
    "unsupported-missing-argument",
    "unsupported-invalid-argument",
    "unsupported-non-finite-number",
    "unsupported-localized-string",
    "unsupported-subroutine-value",
    "unsupported-enum-domain",
    "unsupported-enum-member",
    "unsupported-enum-domain-mismatch",
    "unsupported-value-call",
];

/// One corpus outcome as the checked-in classification records it.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Reconstructed and recompiled to an equivalent program.
    Equivalent,
    /// Reconstruction refused with this exact set of refusal codes.
    Refused(BTreeSet<String>),
    /// `workshop_rs::parser` rejected the input before reconstruction.
    Unparseable,
}

impl Outcome {
    fn describe(&self) -> String {
        match self {
            Outcome::Equivalent => "equivalent".to_string(),
            Outcome::Refused(codes) => format!(
                "refused [{}]",
                codes.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            Outcome::Unparseable => "unparseable".to_string(),
        }
    }
}

/// The recorded classification for one corpus input.
struct Recorded {
    id: String,
    file: String,
    sha256: String,
    outcome: Outcome,
}

/// Load and validate `classification.json`: statuses well formed, refused
/// codes drawn from the contract inventory, `codes` present exactly on
/// `refused` entries.
fn load_classification() -> Result<Vec<Recorded>, String> {
    let path = fixtures_dir().join("classification.json");
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| format!("classification.json is not valid JSON: {e}"))?;
    if record["schemaVersion"] != 1 {
        return Err("classification.json: unsupported schemaVersion".to_string());
    }
    if record["source"]["contentCommit"] != "5a7d0e294b8cad73b9701987bb584d0551d7fa4d" {
        return Err(
            "classification.json: source.contentCommit is not the pinned upstream content commit"
                .to_string(),
        );
    }
    let inputs = record["inputs"]
        .as_array()
        .ok_or("classification.json: inputs must be an array")?;
    let mut entries = Vec::new();
    for input in inputs {
        let id = input["id"]
            .as_str()
            .ok_or("classification.json: input id must be a string")?;
        let file = input["file"]
            .as_str()
            .ok_or(format!("{id}: file must be a string"))?;
        let sha256 = input["sha256"]
            .as_str()
            .ok_or(format!("{id}: sha256 must be a string"))?;
        let status = input["status"]
            .as_str()
            .ok_or(format!("{id}: status must be a string"))?;
        let codes: Vec<String> = match input.get("codes") {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or(format!("{id}: codes must be an array"))?
                .iter()
                .map(|code| {
                    code.as_str()
                        .map(str::to_string)
                        .ok_or(format!("{id}: codes entries must be strings"))
                })
                .collect::<Result<_, _>>()?,
        };
        let outcome = match status {
            "equivalent" => Outcome::Equivalent,
            "refused" => {
                if codes.is_empty() {
                    return Err(format!("{id}: refused requires a non-empty codes set"));
                }
                let unknown: Vec<_> = codes
                    .iter()
                    .filter(|code| !REFUSAL_CODES.contains(&code.as_str()))
                    .collect();
                if !unknown.is_empty() {
                    return Err(format!(
                        "{id}: codes outside the contract refusal inventory: {unknown:?}"
                    ));
                }
                Outcome::Refused(codes.into_iter().collect())
            }
            "unparseable" => Outcome::Unparseable,
            other => return Err(format!("{id}: unknown status {other:?}")),
        };
        if status != "refused" && input.get("codes").is_some_and(|c| !c.is_null()) {
            return Err(format!("{id}: codes is only valid on a refused entry"));
        }
        entries.push(Recorded {
            id: id.to_string(),
            file: file.to_string(),
            sha256: sha256.to_string(),
            outcome,
        });
    }
    Ok(entries)
}

/// Run the contract pipeline over one input and classify the outcome.
fn classify(text: &str, catalog: &Catalog, locale: &Locale, compiler: &Compiler) -> Outcome {
    let Ok(program) = workshop_rs::parser::parse(text, catalog, locale) else {
        return Outcome::Unparseable;
    };
    let opy = match reconstruct(&program) {
        Ok(opy) => opy,
        Err(error) => {
            return Outcome::Refused(error.issues.iter().map(|i| i.code.to_string()).collect());
        }
    };
    // Reconstructed OPY must check clean and recompile to an equivalent
    // program; anything else is a defect, never a recorded classification.
    let artifact = compiler
        .compile_source_artifact(&opy, "reconstructed.opy", Path::new("."))
        .unwrap_or_else(|e| panic!("reconstructed OPY does not recompile: {e:?}\n{opy}"));
    let reparsed = workshop_rs::parser::parse(&artifact.emitted, catalog, locale)
        .unwrap_or_else(|e| panic!("recompiled Workshop output does not reparse: {e:?}"));
    assert!(
        workshop_rs::compare::equivalent(&program, &reparsed),
        "reconstructed OPY recompiles to a non-equivalent program: {:?}",
        workshop_rs::compare::difference(&program, &reparsed)
    );
    Outcome::Equivalent
}

#[test]
fn upstream_decompiler_corpus_matches_the_recorded_classification() {
    let catalog = Catalog::builtin().expect("catalog loads");
    let locale = Locale::new("en-US");
    let compiler = Compiler::new().expect("compiler loads");
    let entries = load_classification().unwrap_or_else(|e| panic!("{e}"));

    // The fixture directory and the classification must describe the same
    // corpus: a stray or missing `.txt` fails the gate.
    let mut on_disk: BTreeSet<String> = std::fs::read_dir(fixtures_dir())
        .expect("fixture directory reads")
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.ends_with(".txt"))
        .collect();
    let mut failures = Vec::new();
    let mut classified = BTreeSet::new();

    for entry in &entries {
        if !classified.insert(entry.id.clone()) {
            failures.push(format!("{}: duplicate classification entry", entry.id));
        }
        let path = fixtures_dir().join(&entry.file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                failures.push(format!("{}: cannot read {}: {e}", entry.id, entry.file));
                continue;
            }
        };
        on_disk.remove(&entry.file);
        let digest = hex(Sha256::digest(&bytes));
        if digest != entry.sha256 {
            failures.push(format!(
                "{}: sha256 {} does not match the recorded {}; the fixture is not the pinned content",
                entry.id, digest, entry.sha256
            ));
            continue;
        }
        let text = String::from_utf8(bytes).expect("fixture is UTF-8");
        let actual = classify(&text, &catalog, &locale, &compiler);
        println!("{:>28} {}", entry.id, actual.describe());
        if actual != entry.outcome {
            failures.push(format!(
                "{}: expected {}, got {}",
                entry.id,
                entry.outcome.describe(),
                actual.describe()
            ));
        }
    }
    for name in on_disk {
        failures.push(format!(
            "{name}: fixture file without a classification entry"
        ));
    }
    assert!(
        failures.is_empty(),
        "upstream decompiler corpus drifted from the recorded classification:\n{}",
        failures.join("\n")
    );
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

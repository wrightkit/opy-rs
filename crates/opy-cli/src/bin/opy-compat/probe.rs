//! Builtin-call probes: one small OPY program per manifest function, default
//! or argument variant, and `#!optimizeForSize` mode. `probe-generate` writes
//! them; `probe-compare` compiles them natively and compares each with the
//! pinned OverPy output that `tools/overpy/probe_builtins.py` collected.

use std::collections::BTreeMap;
use std::path::Path;

use opy_rs::Compiler;
use opy_rs::manifest::{AliasKind, Function, Manifest};
use serde::{Deserialize, Serialize};
use workshop_rs::catalog::{Catalog, CatalogEntry, Kind, Locale};

use super::{CompatibilityExpectedDomain, strip_workshop_comments, structurally_identical};

/// Literals substituted into each argument position, to exercise the
/// replacements the reference applies to small constants.
const LITERALS: [&str; 12] = [
    "0",
    "1",
    "-1",
    "0.5",
    "null",
    "false",
    "true",
    "vect(0, 0, 0)",
    "vect(1, 1, 0)",
    "vect(0, 1, 0)",
    "\"\"",
    "[]",
];

/// Workshop setting calls the manifest does not list, with and without the
/// trailing sort order: (function, variant, call).
const SETTING_CALLS: [(&str, &str, &str); 16] = [
    (
        "createWorkshopSettingBool",
        "base",
        "createWorkshopSettingBool(\"C\", \"N\", true)",
    ),
    (
        "createWorkshopSettingBool",
        "sort-order",
        "createWorkshopSettingBool(\"C\", \"N\", true, 5)",
    ),
    (
        "createWorkshopSettingInt",
        "base",
        "createWorkshopSettingInt(\"C\", \"N\", 1, 0, 10)",
    ),
    (
        "createWorkshopSettingInt",
        "sort-order",
        "createWorkshopSettingInt(\"C\", \"N\", 1, 0, 10, 5)",
    ),
    (
        "createWorkshopSettingFloat",
        "base",
        "createWorkshopSettingFloat(\"C\", \"N\", 1, 0, 10)",
    ),
    (
        "createWorkshopSettingFloat",
        "sort-order",
        "createWorkshopSettingFloat(\"C\", \"N\", 1, 0, 10, 5)",
    ),
    (
        "createWorkshopSettingEnum",
        "base",
        "createWorkshopSettingEnum(\"C\", \"N\", 0, [\"a\", \"b\"])",
    ),
    (
        "createWorkshopSettingEnum",
        "sort-order",
        "createWorkshopSettingEnum(\"C\", \"N\", 0, [\"a\", \"b\"], 5)",
    ),
    (
        "createWorkshopSettingHero",
        "base",
        "createWorkshopSettingHero(\"C\", \"N\", Hero.ANA)",
    ),
    (
        "createWorkshopSettingHero",
        "sort-order",
        "createWorkshopSettingHero(\"C\", \"N\", Hero.ANA, 5)",
    ),
    (
        "createWorkshopSetting",
        "bool",
        "createWorkshopSetting(bool, \"C\", \"N\", true)",
    ),
    (
        "createWorkshopSetting",
        "bool-sort-order",
        "createWorkshopSetting(bool, \"C\", \"N\", true, 5)",
    ),
    (
        "createWorkshopSetting",
        "int",
        "createWorkshopSetting(int[0:10], \"C\", \"N\", 1)",
    ),
    (
        "createWorkshopSetting",
        "int-sort-order",
        "createWorkshopSetting(int[0:10], \"C\", \"N\", 1, 5)",
    ),
    (
        "createWorkshopSetting",
        "float",
        "createWorkshopSetting(float[0:10], \"C\", \"N\", 1)",
    ),
    (
        "createWorkshopSetting",
        "float-sort-order",
        "createWorkshopSetting(float[0:10], \"C\", \"N\", 1, 5)",
    ),
];

/// `*Literal` member spellings the reference accepts: one member per literal
/// domain, every `onlyInOverpy` `ColorLiteral` member, and the `Color.`
/// spelling of the same member. The `ColorLiteral.LIGHT_*` emissions carry
/// the reference's empty argument slot, which the canonical grammar cannot
/// parse on either side — the expected `unparsable` findings are recorded in
/// `tools/overpy/probe-gaps.json` (issue #466).
const LITERAL_MEMBER_CALLS: [(&str, &str); 11] = [
    ("TeamLiteral.1", "g = TeamLiteral.1"),
    ("HeroLiteral.ANA", "g = HeroLiteral.ANA"),
    ("MapLiteral.ROUTE66", "g = MapLiteral.ROUTE66"),
    ("GamemodeLiteral.ASSAULT", "g = GamemodeLiteral.ASSAULT"),
    ("ButtonLiteral.JUMP", "g = ButtonLiteral.JUMP"),
    ("ColorLiteral.WHITE", "g = ColorLiteral.WHITE"),
    ("ColorLiteral.LIGHT_RED", "g = ColorLiteral.LIGHT_RED"),
    ("ColorLiteral.LIGHT_PURPLE", "g = ColorLiteral.LIGHT_PURPLE"),
    ("ColorLiteral.LIGHT_VIOLET", "g = ColorLiteral.LIGHT_VIOLET"),
    ("ColorLiteral.LIGHT_GRAY", "g = ColorLiteral.LIGHT_GRAY"),
    ("Color.LIGHT_RED", "g = Color.LIGHT_RED"),
];

#[derive(Debug, Serialize, Deserialize)]
struct Probe {
    id: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct Reference {
    ok: bool,
    #[serde(default)]
    workshop: String,
    #[serde(default)]
    error: String,
}

pub(super) fn generate() -> Result<(), String> {
    let manifest = Manifest::builtin().map_err(|error| error.to_string())?;
    let catalog = Catalog::builtin().map_err(|error| error.to_string())?;
    let mut probes = Vec::new();
    for function in manifest.functions() {
        let calls = calls(&catalog, function);
        if calls.is_empty() {
            // Functions without a valid sample call would otherwise never
            // reach the oracle; a name probe still verifies the declared
            // spelling is one upstream resolves.
            probes.push(name_probe(&function.id, function.kind.is_member()));
        }
        let mut statements: Vec<(String, String)> = calls
            .iter()
            .map(|call| {
                let statement = if function.kind.is_action() {
                    call.text.clone()
                } else {
                    format!("g = {}", call.text)
                };
                (call.variant.clone(), statement)
            })
            .collect();
        // Every value also stands in the slots that wrap what they cannot hold.
        if let Some(base) = calls.first().filter(|_| !function.kind.is_action()) {
            statements.push((
                "in-boolean".to_string(),
                format!("waitUntil({}, 3)", base.text),
            ));
            statements.push((
                "in-replacement".to_string(),
                format!("g = \"s\".replace(\"s\", \"s\", {})", base.text),
            ));
        }
        for (variant, statement) in statements {
            for (mode, prefix) in [("default", ""), ("size", "#!optimizeForSize\n")] {
                probes.push(Probe {
                    id: format!("{mode}:{}:{variant}", function.id),
                    source: source(prefix, &statement),
                });
            }
        }
    }
    for alias in manifest.aliases() {
        // Alias sources are accepted spellings too; verify each resolves
        // upstream like the function table ids do.
        probes.push(name_probe(
            &alias.source,
            alias.kind == AliasKind::MemberAlias,
        ));
    }
    // Every enum-member spelling the vocabulary lookup reports is an
    // accepted OPY spelling by contract; probe each against the oracle so a
    // catalog id or other upstream-rejected spelling cannot be advertised.
    for (domain, spelling) in enum_spellings(manifest, &catalog) {
        probes.push(Probe {
            id: format!("default:{domain}.{spelling}:spelling"),
            source: source("", &format!("g = {domain}.{spelling}")),
        });
    }
    for (function, variant, call) in SETTING_CALLS {
        for (mode, prefix) in [("default", ""), ("size", "#!optimizeForSize\n")] {
            probes.push(Probe {
                id: format!("{mode}:{function}:{variant}"),
                source: source(prefix, &format!("g = {call}")),
            });
        }
    }
    for (member, statement) in LITERAL_MEMBER_CALLS {
        probes.push(Probe {
            id: format!("default:{member}:base"),
            source: source("", statement),
        });
    }
    println!(
        "{}",
        serde_json::to_string(&probes).map_err(|error| error.to_string())?
    );
    Ok(())
}

struct Call {
    variant: String,
    text: String,
}

fn source(prefix: &str, statement: &str) -> String {
    format!(
        "{prefix}globalvar g\nplayervar p\n\nrule \"probe\":\n    @Event eachPlayer\n    {statement}\n"
    )
}

/// A probe that exercises only name resolution: `g = name()` gets far enough
/// for the oracle to report `Unknown function` when the spelling is not
/// upstream. Any other oracle outcome means the name resolved, so argument
/// mismatches on the synthetic call are expected and ignored.
fn name_probe(name: &str, member: bool) -> Probe {
    let call = if member {
        format!("eventPlayer.{name}()")
    } else {
        format!("{name}()")
    };
    Probe {
        id: format!("default:{name}:name"),
        source: source("", &format!("g = {call}")),
    }
}

/// The `(domain, spelling)` pairs the vocabulary lookup reports for enum
/// members — the reported member spelling plus every reported alias —
/// enumerated through the public `lookup` API so the probed set is exactly
/// the advertised one. Catalog and manifest-parameter domain names seed
/// the walk; every `EnumDomain` hit (renamed source domains included) is
/// queued and its members' spellings collected.
fn enum_spellings(manifest: &Manifest, catalog: &Catalog) -> Vec<(String, String)> {
    use opy_rs::lookup::{LookupHit, LookupOutcome, LookupQuery, lookup};
    let mut queue: Vec<String> = catalog
        .enum_domains()
        .map(|domain| domain.domain.clone())
        .collect();
    queue.extend(
        manifest
            .functions()
            .iter()
            .flat_map(|function| function.params.iter())
            .filter_map(|param| param.domain.clone()),
    );
    let mut seen = std::collections::BTreeSet::new();
    let mut spellings = Vec::new();
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let mut query = LookupQuery::new(name);
        query.limit = 64;
        let LookupOutcome::Matched { results, .. } = lookup(&query) else {
            continue;
        };
        for hit in results {
            let LookupHit::EnumDomain {
                domain,
                members: Some(members),
                ..
            } = hit
            else {
                continue;
            };
            queue.push(domain.clone());
            for member in members {
                spellings.push((domain.clone(), member.spelling.clone()));
                spellings.extend(
                    member
                        .aliases
                        .iter()
                        .map(|alias| (domain.clone(), alias.clone())),
                );
            }
        }
    }
    spellings.sort();
    spellings.dedup();
    spellings
}

/// The base call, its trailing-default omissions, and one call per argument
/// position and probe literal.
fn calls(catalog: &Catalog, function: &Function) -> Vec<Call> {
    let Some(id) = &function.catalog_id else {
        return Vec::new();
    };
    let kind = if function.kind.is_action() {
        Kind::Action
    } else {
        Kind::Value
    };
    let Some(entry) = catalog.entry(kind, id) else {
        return Vec::new();
    };
    let member = function.kind.is_member();
    let skip = usize::from(member);
    let Some(samples) = (0..entry.param_count())
        .map(|index| sample(catalog, entry, index))
        .collect::<Option<Vec<_>>>()
    else {
        return Vec::new();
    };
    if samples.len() < skip {
        return Vec::new();
    }
    let arguments = &samples[skip..];
    let render = |arguments: &[String]| {
        if member {
            format!("{}.{}({})", samples[0], function.id, arguments.join(", "))
        } else {
            format!("{}({})", function.id, arguments.join(", "))
        }
    };
    let mut calls = vec![Call {
        variant: "base".to_string(),
        text: render(arguments),
    }];
    let defaulted = |position: usize| {
        function
            .params
            .get(position)
            .is_some_and(|param| param.default.is_some() || param.optional)
    };
    for kept in 0..arguments.len() {
        if (kept..arguments.len()).all(defaulted) {
            calls.push(Call {
                variant: format!("omit-from-{kept}"),
                text: render(&arguments[..kept]),
            });
        }
    }
    for omitted in (0..arguments.len()).filter(|position| defaulted(*position)) {
        let mut remaining = arguments.to_vec();
        remaining.remove(omitted);
        calls.push(Call {
            variant: format!("omit-arg{omitted}"),
            text: render(&remaining),
        });
    }
    for position in 0..arguments.len() {
        let declared = entry
            .param_type(position + skip)
            .or_else(|| entry.param_domain(position + skip))
            .unwrap_or_default();
        for literal in LITERALS
            .into_iter()
            .filter(|literal| accepts(declared, literal))
        {
            let mut replaced = arguments.to_vec();
            replaced[position] = literal.to_string();
            calls.push(Call {
                variant: format!("arg{position}={literal}"),
                text: render(&replaced),
            });
        }
    }
    calls
}

/// Whether a parameter declared as `declared` takes `literal` as a valid
/// value; `null` is valid everywhere.
fn accepts(declared: &str, literal: &str) -> bool {
    let kind = match literal {
        "null" => return true,
        "false" | "true" => "Boolean",
        "\"\"" => "String",
        "[]" => "Array",
        literal if literal.starts_with("vect") => "Vector",
        _ => "Number",
    };
    declared.split('|').any(|alternative| {
        matches!(alternative, "Any" | "Unknown" | "Object") || alternative == kind
    })
}

/// A valid literal for one catalog parameter, or `None` for a type the probe
/// cannot spell.
fn sample(catalog: &Catalog, entry: &CatalogEntry, index: usize) -> Option<String> {
    let declared = entry
        .param_type(index)
        .or_else(|| entry.param_domain(index))?;
    declared.split('|').find_map(|alternative| {
        if let Some(domain) = catalog.enum_domain(alternative) {
            return domain
                .members
                .first()
                .map(|member| format!("{alternative}.{}", member.member));
        }
        match alternative {
            "Number" | "Object" | "Any" | "Unknown" => Some("3".to_string()),
            "Boolean" => Some("true".to_string()),
            "Player" | "EntityId" => Some("eventPlayer".to_string()),
            "Vector" => Some("vect(1, 2, 3)".to_string()),
            "Array" => Some("[1, 2]".to_string()),
            "String" | "Text" => Some("\"s\"".to_string()),
            _ => None,
        }
    })
}

pub(super) fn compare(probes: &Path, references: &Path) -> Result<(), String> {
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read '{}': {error}", path.display()))
    };
    let probes: Vec<Probe> = serde_json::from_str(&read(probes)?)
        .map_err(|error| format!("cannot parse probes: {error}"))?;
    let references: BTreeMap<String, Reference> = serde_json::from_str(&read(references)?)
        .map_err(|error| format!("cannot parse references: {error}"))?;
    let compiler = Compiler::new().map_err(|error| error.to_string())?;
    let catalog = Catalog::builtin().map_err(|error| error.to_string())?;
    let context = CompatibilityExpectedDomain { catalog: &catalog };
    let locale = Locale::new("en-US");
    let parse = |text: &str| {
        workshop_rs::parser::parse_with_context(
            &strip_workshop_comments(text),
            &catalog,
            &locale,
            &context,
        )
        .map_err(|error| error.to_string())
    };

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut findings = Vec::new();
    for probe in &probes {
        let reference = references
            .get(&probe.id)
            .ok_or_else(|| format!("no reference result for {}", probe.id))?;
        // `mode:name:variant`; the probed spelling is the middle field and
        // `name` variants only exercise name resolution, not native behavior.
        let mut fields = probe.id.split(':');
        let _ = fields.next();
        let name = fields.next().unwrap_or_default();
        let variant = fields.next().unwrap_or_default();
        let unknown_spelling = !reference.ok
            && (reference
                .error
                .contains(&format!("Unknown function '{name}'"))
                || reference
                    .error
                    .contains(&format!("Unknown function '.{name}'")));
        let (status, detail) = if variant == "name" {
            (
                if unknown_spelling {
                    "unknown-spelling"
                } else {
                    "spelling-ok"
                },
                String::new(),
            )
        } else if variant == "spelling" {
            // An enum-spelling probe is a bare `Domain.MEMBER` access. The
            // question is whether the reference's member resolution accepts
            // the spelling: `Unknown map 'X'`-style rejections mean the
            // lookup advertised a spelling the reference does not have.
            // Later rejections (`not available in OW2`) accept the spelling
            // and fail downstream of resolution, so they classify
            // separately and need an explicit recorded gap.
            if reference.ok {
                ("spelling-ok", String::new())
            } else if reference.error.contains("Unknown ") {
                ("unknown-spelling", reference.error.clone())
            } else {
                ("reference-rejected", reference.error.clone())
            }
        } else {
            let native = compiler.compile_source_with_locale(
                &probe.source,
                "probe.opy",
                Path::new("."),
                &locale,
            );
            match (reference.ok, native) {
                (false, _) if unknown_spelling => ("unknown-spelling", String::new()),
                (false, Err(_)) => ("both-reject", String::new()),
                (false, Ok(_)) => ("native-accepts", String::new()),
                (true, Err(error)) => ("native-rejects", error.to_string()),
                (true, Ok(artifact)) => {
                    match (parse(&artifact.emitted), parse(&reference.workshop)) {
                        (Ok(native), Ok(reference))
                            if structurally_identical(&native, &reference) =>
                        {
                            ("match", String::new())
                        }
                        (Ok(_), Ok(_)) => ("different", String::new()),
                        (Err(error), _) | (_, Err(error)) => ("unparsable", error),
                    }
                }
            }
        };
        *counts.entry(status).or_default() += 1;
        if !matches!(status, "match" | "both-reject" | "spelling-ok") {
            findings.push(serde_json::json!({
                "id": probe.id,
                "status": status,
                "detail": detail,
                "source": probe.source,
            }));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "probes": probes.len(),
            "counts": counts,
            "findings": findings,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

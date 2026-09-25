//! Structural comparison of two parsed Workshop programs.
//!
//! Two programs are the same when their settings, variables, subroutines and
//! rules are identical; only source provenance, which the public program keeps
//! private, is ignored. A difference is reported by section, item, and the
//! canonical path of the first diverging node, never as a text diff.
//! `roundtrip::equivalent` is deliberately not used: it treats structurally
//! different but behaviorally equal programs as the same.

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct Difference {
    /// `settings`, `globalVariables`, `playerVariables`, `subroutines` or `rules`.
    pub section: &'static str,
    /// Position within the section; the reference position when they differ.
    pub index: usize,
    /// Declared name of the item, from the reference when it has one.
    pub name: Option<String>,
    /// Canonical path of the first diverging node, e.g. `Rule.conditions[1].Call.args[0]`.
    pub path: String,
    pub native: String,
    pub reference: String,
}

pub(super) fn differences(
    native: &workshop_rs::Program,
    reference: &workshop_rs::Program,
) -> Vec<Difference> {
    let mut found = Vec::new();
    compare(
        &mut found,
        "settings",
        &native.settings.iter().map(dump).collect::<Vec<_>>(),
        &reference.settings.iter().map(dump).collect::<Vec<_>>(),
    );
    compare(
        &mut found,
        "globalVariables",
        &dumps(&native.global_variables),
        &dumps(&reference.global_variables),
    );
    compare(
        &mut found,
        "playerVariables",
        &dumps(&native.player_variables),
        &dumps(&reference.player_variables),
    );
    compare(
        &mut found,
        "subroutines",
        &dumps(&native.subroutines),
        &dumps(&reference.subroutines),
    );
    compare(
        &mut found,
        "rules",
        &dumps(&native.rules),
        &dumps(&reference.rules),
    );
    found
}

/// Pretty-printed item without source positions, which are presentation,
/// not structure.
fn dump(item: &impl std::fmt::Debug) -> String {
    format!("{item:#?}")
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !(line.starts_with("line: ") || line.starts_with("col: "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dumps<T: std::fmt::Debug>(items: &[T]) -> Vec<String> {
    items.iter().map(dump).collect()
}

fn compare(
    found: &mut Vec<Difference>,
    section: &'static str,
    native: &[String],
    reference: &[String],
) {
    for index in 0..native.len().max(reference.len()) {
        let (native_item, reference_item) = (native.get(index), reference.get(index));
        if native_item == reference_item {
            continue;
        }
        let (path, native_line, reference_line) = diverge(
            native_item.map_or("", String::as_str),
            reference_item.map_or("", String::as_str),
        );
        found.push(Difference {
            section,
            index,
            name: reference_item.or(native_item).and_then(|item| name(item)),
            path,
            native: native_line,
            reference: reference_line,
        });
    }
}

/// The value of the first `name: "…"` field, which identifies a rule,
/// variable or subroutine.
fn name(dump: &str) -> Option<String> {
    dump.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("name: ")
            .map(|value| value.trim_end_matches(',').to_string())
    })
}

/// First diverging line of two dumps, with the path of the enclosing nodes.
fn diverge(native: &str, reference: &str) -> (String, String, String) {
    let native: Vec<&str> = native.lines().collect();
    let reference: Vec<&str> = reference.lines().collect();
    let at = native
        .iter()
        .zip(&reference)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| native.len().min(reference.len()));
    let shown = |lines: &[&str]| {
        lines
            .get(at)
            .map_or("<absent>", |line| line.trim())
            .to_string()
    };
    let anchor = if at < reference.len() {
        &reference
    } else {
        &native
    };
    (path(anchor, at), shown(&native), shown(&reference))
}

struct Frame {
    indent: usize,
    label: String,
    list: bool,
    children: usize,
}

/// Enclosing node labels of `lines[at]`, from the indentation of the
/// pretty-printed tree; list members carry their position.
fn path(lines: &[&str], at: usize) -> String {
    let mut stack: Vec<Frame> = Vec::new();
    let mut labels = Vec::new();
    for (position, line) in lines.iter().enumerate().take(at + 1) {
        let text = line.trim_start();
        let indent = line.len() - text.len();
        while stack.last().is_some_and(|frame| frame.indent >= indent) {
            stack.pop();
        }
        let closes = text.starts_with(['}', ']', ')']);
        let position_label = stack
            .last_mut()
            .filter(|frame| frame.list && !closes)
            .map(|frame| {
                frame.children += 1;
                format!("[{}]", frame.children - 1)
            });
        if position == at {
            labels = stack.iter().map(|frame| frame.label.clone()).collect();
            break;
        }
        if text.ends_with(['{', '[', '(']) && !closes {
            let head = text.split([':', '{', '[', '(']).next().unwrap_or("").trim();
            let label = match position_label {
                Some(position) if head.is_empty() => position,
                Some(position) => format!("{position}{head}"),
                None => head.to_string(),
            };
            stack.push(Frame {
                indent,
                label,
                list: text.ends_with('['),
                children: 0,
            });
        }
    }
    labels.retain(|label| !label.is_empty());
    labels.join(".")
}

#[cfg(test)]
mod tests {
    use super::{differences, path};
    use crate::CompatibilityExpectedDomain;
    use workshop_rs::catalog::{Catalog, Locale};

    fn parse(text: &str) -> workshop_rs::Program {
        let catalog = Catalog::builtin().unwrap();
        let context = CompatibilityExpectedDomain { catalog: &catalog };
        workshop_rs::parser::parse_with_context(text, &catalog, &Locale::new("en-US"), &context)
            .unwrap()
    }

    const TWO_CONDITIONS: &str = r#"rule("R") {
        event { Ongoing - Global; }
        conditions { Is Game In Progress == True; Global.A == 1; }
        actions { Wait(1, Ignore Condition); }
    }"#;

    #[test]
    fn merging_two_conditions_is_reported_by_rule_and_path() {
        let merged = TWO_CONDITIONS.replace(
            "Is Game In Progress == True; Global.A == 1;",
            "Is Game In Progress == True && Global.A == 1;",
        );
        let found = differences(&parse(&merged), &parse(TWO_CONDITIONS));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].section, found[0].index), ("rules", 0));
        assert_eq!(found[0].name.as_deref(), Some("\"R\""));
        assert!(found[0].path.contains("conditions"), "{}", found[0].path);
    }

    #[test]
    fn block_and_line_shapes_of_the_same_program_do_not_differ() {
        let compact = "settings { extensions { Buff Status Effects } }\n";
        let spread = "settings\n{\n\textensions\n\t{\n\t\tBuff Status Effects\n\t}\n}\n";
        assert!(differences(&parse(compact), &parse(spread)).is_empty());
        let reflowed = TWO_CONDITIONS.replace("; ", ";\n\t\t");
        assert!(differences(&parse(&reflowed), &parse(TWO_CONDITIONS)).is_empty());
    }

    #[test]
    fn a_path_names_the_enclosing_nodes_and_list_positions() {
        let dump = [
            "Rule {",
            "    name: \"r\",",
            "    conditions: [",
            "        Condition {",
            "            value: 1,",
            "        },",
            "        Condition {",
            "            value: 2,",
            "        },",
            "    ],",
            "}",
        ];
        assert_eq!(path(&dump, 7), "Rule.conditions.[1]Condition");
    }
}

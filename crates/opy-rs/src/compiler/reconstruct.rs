use std::collections::{HashMap, HashSet};
use std::fmt;

use workshop_rs::Program;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::program::{
    Action, Event, EventTarget, EventTeam, ModifyOp, Rule, Value, Variable,
};
use workshop_rs::source::Span;

use crate::lexer::{is_ident_continue, is_ident_start, is_identifier};
use crate::lower::special_forms::SpecialValueCall;
use crate::lower::{NameNamespace, is_reserved_name, policy};
use crate::manifest::{Function, FunctionKind, Manifest};

/// A structured reconstruction diagnostic naming one non-representable
/// Workshop construct, with its source span when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructIssue {
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
}

/// All reconstruction failures for one program. The emitter never returns
/// partial output: a non-empty issue list means no OPY was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructError {
    pub issues: Vec<ReconstructIssue>,
}

impl fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.issues.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            let location = match issue.span {
                Some(span) => format!(" at {}:{}", span.start.line, span.start.col),
                None => String::new(),
            };
            write!(f, "{}: {}{location}", issue.code, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ReconstructError {}

/// Reconstruct a canonical Workshop program into OPY source.
///
/// Resolves builtin identities through the built-in OPY semantic manifest
/// and Workshop catalog (`en-US`). Returns all diagnostics when a construct
/// cannot be represented in OPY.
pub fn reconstruct(program: &Program) -> Result<String, ReconstructError> {
    let manifest = match Manifest::builtin() {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "manifest-error",
                    message: format!(
                        "cannot load the OPY semantic compatibility manifest: {error}"
                    ),
                    span: None,
                }],
            });
        }
    };
    let catalog = match Catalog::builtin_ref() {
        Ok(catalog) => catalog,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "catalog-error",
                    message: format!("cannot load the Workshop catalog: {error}"),
                    span: None,
                }],
            });
        }
    };
    reconstruct_with(program, manifest, catalog, &Locale::new("en-US"))
}

/// The context-sensitive form of [`reconstruct`]: resolves identities through
/// the supplied manifest and catalog. The locale selects the catalog
/// spellings used for cross-checks (reconstruction emits OPY, which is
/// locale-independent; `en-US` is the catalog's declared surface).
pub fn reconstruct_with(
    program: &Program,
    manifest: &Manifest,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<String, ReconstructError> {
    let mut emitter = Emitter::new(program, manifest, catalog, locale);
    emitter.run();
    if emitter.issues.is_empty() {
        Ok(emitter.out)
    } else {
        Err(ReconstructError {
            issues: emitter.issues,
        })
    }
}

/// Call names a `name()` invocation resolves before the declared-subroutine
/// check in the OPY frontend (`lower_call`): the translation calls,
/// `compressed`, `vect`, `async`/`startRule`, plus the `sorted` and
/// `createWorkshopSetting*` special forms (kept in `SPECIAL_VALUE_CALLS`),
/// and `range`, which a subroutine would steal from every `for ... in`
/// iterable. A subroutine carrying one of these names is emitted mangled;
/// the remaining shadowed names are the upstream reserved sets used through
/// `is_reserved_name`.
const SHADOWED_SUBROUTINE_NAMES: &[&str] = &[
    "_",
    "__",
    "___",
    "compressed",
    "vect",
    "async",
    "startRule",
    "range",
];

/// Binary operator spellings the OPY frontend lowers to `Value::Call`s with
/// the same name (source operators, not Workshop spellings like `add`).
const BINARY_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "and", "or",
];

/// Call names the frontend lowers to dedicated Workshop actions or values.
const DEDICATED_ACTION_NAMES: &[&str] = &["append"];
const DEDICATED_VALUE_NAMES: &[&str] = &["vect", "range", "chase"];

struct Emitter<'a> {
    program: &'a Program,
    manifest: &'a Manifest,
    catalog: &'a Catalog,
    locale: &'a Locale,
    issues: Vec<ReconstructIssue>,
    out: String,
    /// Canonical table name → emitted OPY name, under the recorded
    /// mangling rule (identity when the name re-resolves to its
    /// declaration).
    global_names: HashMap<String, String>,
    player_names: HashMap<String, String>,
    subroutine_emitted: HashMap<String, String>,
    /// Canonical subroutine names, for dangling-reference checks.
    subroutine_names: HashSet<String>,
    /// Emitted subroutine names, for call-vs-subroutine ambiguity checks.
    emitted_subroutine_names: HashSet<String>,
}

type IndexedRule<'a> = (usize, &'a Rule);

struct RuleLayout<'a> {
    global_init: Option<IndexedRule<'a>>,
    player_init: Option<IndexedRule<'a>>,
    sub_rules: Vec<IndexedRule<'a>>,
    normal_rules: Vec<IndexedRule<'a>>,
}

impl<'a> Emitter<'a> {
    fn new(
        program: &'a Program,
        manifest: &'a Manifest,
        catalog: &'a Catalog,
        locale: &'a Locale,
    ) -> Self {
        let global_names = mangle_names(
            program
                .global_variables
                .iter()
                .map(|variable| variable.name.as_str()),
            NameNamespace::Global,
        );
        let player_names = mangle_names(
            program
                .player_variables
                .iter()
                .map(|variable| variable.name.as_str()),
            NameNamespace::Player,
        );
        let subroutine_emitted = mangle_names(
            program
                .subroutines
                .iter()
                .map(|subroutine| subroutine.name.as_str()),
            NameNamespace::Subroutine,
        );
        let emitted_subroutine_names = subroutine_emitted.values().cloned().collect();
        let subroutine_names = program
            .subroutines
            .iter()
            .map(|subroutine| subroutine.name.clone())
            .collect();
        Emitter {
            program,
            manifest,
            catalog,
            locale,
            issues: Vec::new(),
            out: String::new(),
            global_names,
            player_names,
            subroutine_emitted,
            subroutine_names,
            emitted_subroutine_names,
        }
    }

    fn run(&mut self) {
        self.validate_tables();
        if self.issues.is_empty() {
            let layout = self.classify_rules();
            if self.issues.is_empty() {
                self.emit_program(&layout);
            }
        }
    }
    fn issue(&mut self, code: &'static str, message: impl Into<String>, span: Option<Span>) {
        self.issues.push(ReconstructIssue {
            code,
            message: message.into(),
            span,
        });
    }

    fn report_disabled_rule(&mut self, rule_index: usize, name: &str) {
        self.issue(
            "unsupported-disabled-rule",
            format!("rule '{name}' is disabled; the OPY surface cannot express it"),
            self.program.rule_span(rule_index),
        );
    }

    fn validate_tables(&mut self) {
        if self.program.settings.is_some() {
            self.issue(
                "unsupported-settings",
                "custom-game-settings are outside the reconstruction surface",
                None,
            );
        }
        // Every table: names must be unique (duplicates cannot be declared
        // or referenced faithfully); indices must be unique and inside the
        // Workshop slot range so the emitted explicit-index declarations
        // re-lower to the same name → index binding. Names outside the OPY
        // surface are not a failure here — they are mangled on emission.
        let mut taken = HashSet::new();
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            self.check_duplicate_name(&variable.name, position, "global variable", None);
            self.check_index(
                variable.index.unwrap_or(position as u32),
                &mut taken,
                "global variable",
                &variable.name,
            );
        }
        taken.clear();
        for (position, variable) in self.program.player_variables.iter().enumerate() {
            self.check_duplicate_name(&variable.name, position, "player variable", None);
            self.check_index(
                variable.index.unwrap_or(position as u32),
                &mut taken,
                "player variable",
                &variable.name,
            );
        }
        taken.clear();
        for (position, subroutine) in self.program.subroutines.iter().enumerate() {
            self.check_duplicate_name(&subroutine.name, position, "subroutine", None);
            self.check_index(
                subroutine.index.unwrap_or(position as u32),
                &mut taken,
                "subroutine",
                &subroutine.name,
            );
        }
    }

    /// A declaration index the explicit-index declaration forms cannot
    /// emit: outside the `0..=127` Workshop slot range or already claimed
    /// by a table sibling (the frontend rejects index collisions on
    /// re-lowering).
    fn check_index(&mut self, index: u32, taken: &mut HashSet<u32>, kind: &str, name: &str) {
        if index > 127 {
            self.issue(
                "unsupported-index",
                format!("{kind} '{name}' index {index} is outside the 0-127 slot range"),
                None,
            );
        } else if !taken.insert(index) {
            self.issue(
                "unsupported-index",
                format!("{kind} '{name}' shares declaration index {index} with a sibling"),
                None,
            );
        }
    }

    /// Whether a name repeats an earlier entry of its table (duplicates
    /// cannot be declared or referenced faithfully on the OPY surface).
    fn check_duplicate_name(
        &mut self,
        name: &str,
        position: usize,
        kind: &str,
        span: Option<Span>,
    ) {
        let duplicate = match kind {
            "global variable" => self
                .program
                .global_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            "player variable" => self
                .program
                .player_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            _ => self
                .program
                .subroutines
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
        };
        if duplicate {
            self.issue(
                "unsupported-duplicate-name",
                format!("duplicate {kind} name '{name}'"),
                span,
            );
        }
    }

    /// The canonical rule layout: optional leading initializer rules, then
    /// all subroutine-body rules in subroutine table order, then the normal
    /// rules. Any other arrangement cannot be reproduced by the frontend's
    /// deterministic re-lowering and is rejected.
    fn classify_rules(&mut self) -> RuleLayout<'a> {
        let rules: Vec<IndexedRule<'a>> = self.program.rules.iter().enumerate().collect();
        let mut index = 0;
        let mut global_init = None;
        let mut player_init = None;
        if let Some(&(rule_index, rule)) = rules.first() {
            if rule.name == "Initialize global variables" {
                let candidate = self.canonical_init(rule_index, rule, true);
                if candidate.is_some() {
                    global_init = candidate;
                    index = 1;
                }
            } else if rule.name == "Initialize player variables" {
                let candidate = self.canonical_init(rule_index, rule, false);
                if candidate.is_some() {
                    player_init = candidate;
                    index = 1;
                }
            }
        }
        if index == 1 {
            if let Some(&(rule_index, rule)) = rules.get(1) {
                if rule.name == "Initialize player variables" && global_init.is_some() {
                    let candidate = self.canonical_init(rule_index, rule, false);
                    if candidate.is_some() {
                        player_init = candidate;
                        index = 2;
                    }
                }
            }
        }

        let mut sub_rules = Vec::new();
        let mut normal_rules = Vec::new();
        let mut in_sub_rules = true;
        for (rule_index, rule) in rules.iter().copied().skip(index) {
            match &rule.event {
                Event::Subroutine(_) => {
                    if !in_sub_rules {
                        self.issue(
                            "unsupported-rule-order",
                            format!(
                                "subroutine-body rule '{}' appears after a normal rule; \
                                 the frontend re-lowering emits subroutine rules first",
                                rule.name
                            ),
                            self.program.rule_span(rule_index),
                        );
                    }
                    if !rule.conditions.is_empty() {
                        self.issue(
                            "unsupported-rule-order",
                            format!(
                                "subroutine-body rule '{}' carries conditions; `def` \
                                 bodies cannot express them",
                                rule.name
                            ),
                            self.program.rule_span(rule_index),
                        );
                    }
                    sub_rules.push((rule_index, rule));
                }
                _ => {
                    in_sub_rules = false;
                    normal_rules.push((rule_index, rule));
                }
            }
        }

        // Subroutine rules must be in subroutine table order, and each rule
        // must carry the exact name the re-lowering synthesizes for its def.
        let mut expected = 0usize;
        for (rule_index, rule) in &sub_rules {
            let Event::Subroutine(subroutine) = &rule.event else {
                continue;
            };
            if self
                .program
                .subroutines
                .iter()
                .position(|definition| definition.name == *subroutine)
                != Some(expected)
            {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "subroutine-body rules must appear in subroutine table order; \
                         '{}' is out of order",
                        rule.name
                    ),
                    self.program.rule_span(*rule_index),
                );
            }
            expected += 1;
            if let Some(definition) = self
                .program
                .subroutines
                .iter()
                .find(|definition| definition.name == *subroutine)
            {
                let expected_name = format!("Subroutine {}", definition.name);
                if rule.name != expected_name {
                    self.issue(
                        "unsupported-rule-order",
                        format!(
                            "subroutine-body rule name '{}' does not match the def \
                             form '{}' the frontend synthesizes",
                            rule.name, expected_name
                        ),
                        self.program.rule_span(*rule_index),
                    );
                }
            }
        }

        RuleLayout {
            global_init,
            player_init,
            sub_rules,
            normal_rules,
        }
    }

    fn canonical_init(
        &mut self,
        rule_index: usize,
        rule: &'a Rule,
        global: bool,
    ) -> Option<IndexedRule<'a>> {
        if rule.disabled {
            return None;
        }
        let expected_name = if global {
            "Initialize global variables"
        } else {
            "Initialize player variables"
        };
        let canonical_event = if global {
            matches!(&rule.event, Event::Global)
        } else {
            matches!(&rule.event, Event::EachPlayer)
        };
        if !canonical_event {
            return None;
        }
        if !rule.conditions.is_empty() {
            self.issue(
                "unsupported-init-rule",
                format!(
                    "initializer rule '{expected_name}' carries conditions; the \
                     frontend synthesizes it from declarations with none"
                ),
                self.program.rule_span(rule_index),
            );
            return None;
        }
        for (action_index, action) in rule.actions.iter().enumerate() {
            let set = matches!(
                (global, action),
                (true, Action::SetGlobalVariable { .. })
                    | (false, Action::SetPlayerVariable { .. })
            );
            if !set {
                self.issue(
                    "unsupported-init-rule",
                    format!(
                        "initializer rule '{expected_name}' mixes non-Set actions; \
                         the frontend's synthesized initializer rule is all-Set"
                    ),
                    self.program.action_span(rule_index, action_index),
                );
                return None;
            }
        }
        Some((rule_index, rule))
    }

    fn emit_program(&mut self, layout: &RuleLayout) {
        let global_initializers = self.collect_initializers(layout.global_init, true);
        let player_initializers = self.collect_initializers(layout.player_init, false);
        let global_order = index_order(&self.program.global_variables);
        let player_order = index_order(&self.program.player_variables);
        self.check_initializer_slots(true, &global_order, &global_initializers);
        self.check_initializer_slots(false, &player_order, &player_initializers);

        // The strict-optimization directive is part of every reconstruction:
        // without it the frontend's default folding rewrites values such as
        // `Add(x, 0)` into a different canonical shape than the input's.
        self.out.push_str("#!optimizeStrict\n\n");
        self.emit_variable_declarations(true, &global_order, &global_initializers);
        self.emit_variable_declarations(false, &player_order, &player_initializers);
        if self.program.subroutines.is_empty() {
            self.out.push('\n');
        } else {
            // A bare `subroutine name` auto-allocates the lowest free index;
            // it reproduces the canonical binding only for the leading run
            // whose index already equals the table position. Every later
            // declaration carries its index explicitly.
            let prefix = self
                .program
                .subroutines
                .iter()
                .enumerate()
                .take_while(|(position, subroutine)| {
                    subroutine.index.unwrap_or(*position as u32) == *position as u32
                })
                .count();
            for (position, subroutine) in self.program.subroutines.iter().enumerate() {
                self.out.push_str("subroutine ");
                self.out
                    .push_str(emitted(&self.subroutine_emitted, &subroutine.name));
                if position >= prefix {
                    let index = subroutine.index.unwrap_or(position as u32);
                    self.out.push_str(&format!(" {index}"));
                }
                self.out.push('\n');
            }
            self.out.push('\n');
        }

        for (rule_index, rule) in &layout.sub_rules {
            if rule.disabled {
                self.report_disabled_rule(*rule_index, &rule.name);
                continue;
            }
            let Event::Subroutine(subroutine_name) = &rule.event else {
                continue;
            };
            let Some(definition) = self
                .program
                .subroutines
                .iter()
                .find(|definition| definition.name == *subroutine_name)
            else {
                continue;
            };
            self.out.push_str("def ");
            self.out
                .push_str(emitted(&self.subroutine_emitted, &definition.name));
            self.out.push_str("():\n");
            self.emit_actions(*rule_index, &rule.actions, 1);
            self.out.push('\n');
        }

        for (rule_index, rule) in &layout.normal_rules {
            if rule.disabled {
                self.report_disabled_rule(*rule_index, &rule.name);
                continue;
            }
            if rule.actions.is_empty() {
                continue;
            }
            self.out.push_str("rule ");
            self.emit_string_literal(&rule.name);
            self.out.push_str(":\n");
            match &rule.event {
                Event::Global => self.out.push_str("    @Event global\n"),
                Event::EachPlayer
                | Event::EachPlayerWithFilters {
                    team: EventTeam::All,
                    target: EventTarget::All,
                } => self.out.push_str("    @Event eachPlayer\n"),
                Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
                    self.issue(
                        "unsupported-rule-event",
                        format!("rule '{}' uses an event outside the OPY surface", rule.name),
                        self.program.rule_span(*rule_index),
                    );
                    continue;
                }
                Event::Subroutine(_) => {
                    self.issue(
                        "unsupported-rule-order",
                        format!(
                            "rule '{}' has a subroutine event outside the def layout",
                            rule.name
                        ),
                        self.program.rule_span(*rule_index),
                    );
                    continue;
                }
            }
            for (condition_index, condition) in rule.conditions.iter().enumerate() {
                if condition.disabled {
                    self.issue(
                        "unsupported-disabled-condition",
                        "disabled conditions are outside the OPY reconstruction surface",
                        self.program.condition_span(*rule_index, condition_index),
                    );
                    continue;
                }
                self.out.push_str("    @Condition ");
                self.emit_value(
                    &condition.value,
                    self.program.condition_span(*rule_index, condition_index),
                );
                self.out.push('\n');
            }
            self.emit_actions(*rule_index, &rule.actions, 1);
            self.out.push('\n');
        }
    }

    /// Declarations emit in canonical index order — the order the
    /// frontend's re-lowering sorts the table into — each with its explicit
    /// index unless an initializer carries it (`name = value` re-lowers to
    /// a slot `check_initializer_slots` verified).
    fn emit_variable_declarations(
        &mut self,
        global: bool,
        order: &[(usize, u32)],
        initializers: &HashMap<usize, (Value, Option<Span>)>,
    ) {
        let (kind, variables) = if global {
            ("globalvar ", &self.program.global_variables)
        } else {
            ("playervar ", &self.program.player_variables)
        };
        for &(position, index) in order {
            let variable = &variables[position];
            let name = emitted(
                if global {
                    &self.global_names
                } else {
                    &self.player_names
                },
                &variable.name,
            )
            .to_string();
            self.out.push_str(kind);
            self.out.push_str(&name);
            match initializers.get(&position) {
                Some(value) => {
                    self.out.push_str(" = ");
                    self.emit_initializer(&value.0, value.1);
                }
                None => {
                    self.out.push(' ');
                    self.out.push_str(&index.to_string());
                }
            }
            self.out.push('\n');
        }
    }

    /// Map initializer rule actions onto declaration positions (table order),
    /// validating the rule's Sets are in table order like the frontend's
    /// synthesized initializer rule.
    fn collect_initializers(
        &mut self,
        initializer: Option<IndexedRule<'_>>,
        global: bool,
    ) -> std::collections::HashMap<usize, (Value, Option<Span>)> {
        let mut initializers = std::collections::HashMap::new();
        let Some((rule_index, rule)) = initializer else {
            return initializers;
        };
        let mut previous: Option<u32> = None;
        for (action_index, action) in rule.actions.iter().enumerate() {
            let span = self.program.action_span(rule_index, action_index);
            let (player, variable, value) = match (global, action) {
                (true, Action::SetGlobalVariable { variable, value }) => (None, variable, value),
                (
                    false,
                    Action::SetPlayerVariable {
                        player,
                        variable,
                        value,
                    },
                ) => (Some(player), variable, value),
                _ => continue,
            };
            if player.is_some_and(|player| !self.is_event_player(player)) {
                self.issue(
                    "unsupported-init-rule",
                    "player initializer targets a non-event-player expression",
                    span,
                );
            }
            let variables = if global {
                &self.program.global_variables
            } else {
                &self.program.player_variables
            };
            let Some(variable_position) = variables
                .iter()
                .position(|declaration| declaration.name == *variable)
            else {
                self.issue(
                    "unsupported-dangling",
                    format!(
                        "unknown {} variable '{variable}'",
                        if global { "global" } else { "player" }
                    ),
                    span,
                );
                continue;
            };
            // Re-lowering emits the synthesized initializer rule in
            // index-sorted table order, so the canonical rule's Sets must
            // already be index-ordered to round-trip.
            let index = variables[variable_position]
                .index
                .unwrap_or(variable_position as u32);
            self.record_initializer_position(
                &mut previous,
                index,
                variable,
                if global { "global" } else { "player" },
                span,
            );
            initializers.insert(
                variable_position,
                (
                    value.clone(),
                    self.program.action_argument_span(
                        rule_index,
                        action_index,
                        usize::from(!global),
                    ),
                ),
            );
        }
        initializers
    }

    fn record_initializer_position(
        &mut self,
        previous: &mut Option<u32>,
        index: u32,
        variable: &str,
        table: &str,
        span: Option<Span>,
    ) {
        if previous.is_some_and(|previous| index <= previous) {
            self.issue(
                "unsupported-init-rule",
                format!(
                    "initializer rule Sets '{variable}' out of {table} index order; \
                     the frontend synthesizes initializers in declaration order"
                ),
                span,
            );
        }
        *previous = Some(index);
    }

    /// A declaration initializer: same value emission, but zero literals are
    /// spelled `0.0` because the frontend drops integer-`0` initializers
    /// (matching the reference adapter).
    fn emit_initializer(&mut self, value: &Value, span: Option<Span>) {
        if let Value::Number(number) = value {
            if *number == 0.0 {
                self.out.push_str("0.0");
                return;
            }
        }
        self.emit_value(value, span);
    }

    /// The OPY declaration `name = value` cannot carry an explicit slot, so
    /// the frontend re-lowering auto-allocates one: explicit indices are
    /// reserved upfront, `__x__`-wrapped names then take the highest free
    /// slots in declaration order, and the remaining initializer
    /// declarations fill the lowest free slots in declaration order.
    /// Simulate that allocation over the emitted (index-sorted) order: an
    /// initializer-bearing declaration is only representable when the slot
    /// the frontend would assign equals its Workshop index; otherwise the
    /// reconstructed table would differ.
    fn check_initializer_slots(
        &mut self,
        global: bool,
        order: &[(usize, u32)],
        initializers: &HashMap<usize, (Value, Option<Span>)>,
    ) {
        let kind = if global { "globalvar" } else { "playervar" };
        let (variables, names) = if global {
            (&self.program.global_variables, &self.global_names)
        } else {
            (&self.program.player_variables, &self.player_names)
        };
        let mut taken = HashSet::new();
        let mut pending = Vec::new();
        for &(position, index) in order {
            if initializers.contains_key(&position) {
                let name = emitted(names, &variables[position].name);
                pending.push((
                    position,
                    index,
                    name.starts_with("__") && name.ends_with("__"),
                ));
            } else {
                taken.insert(index);
            }
        }
        // The frontend's top-allocation pass runs before the lowest-free
        // pass, so `__x__` initializer declarations claim their slots first.
        for high in [true, false] {
            for &(position, index, dunder) in &pending {
                if dunder != high {
                    continue;
                }
                let slot = if high {
                    (0..=127u32).rev().find(|slot| !taken.contains(slot))
                } else {
                    (0..=127u32).find(|slot| !taken.contains(slot))
                };
                if slot != Some(index) {
                    self.issues.push(ReconstructIssue {
                        code: "unsupported-indexed-initializer",
                        message: format!(
                            "initializer-bearing {} '{}' occupies slot {} but the \
                             OPY `{kind} name = value` form assigns slot {} on \
                             re-lowering",
                            if global { "global" } else { "player" },
                            variables[position].name,
                            index,
                            slot.map_or_else(|| "none".to_string(), |slot| slot.to_string()),
                        ),
                        span: None,
                    });
                }
                if let Some(slot) = slot {
                    taken.insert(slot);
                }
            }
        }
    }

    fn emit_actions(&mut self, rule_index: usize, actions: &[Action], level: usize) {
        let mut position = 0;
        self.emit_action_block(rule_index, actions, &mut position, level);
        if position < actions.len() {
            self.issue(
                "unsupported-control-flow",
                "unexpected control-flow marker in Workshop action sequence",
                self.program.action_span(rule_index, position),
            );
        }
    }

    fn emit_action_block(
        &mut self,
        rule_index: usize,
        actions: &[Action],
        position: &mut usize,
        level: usize,
    ) {
        while let Some(action) = actions.get(*position) {
            match action {
                Action::ElseIf { .. } | Action::Else | Action::End => return,
                Action::If { condition } => {
                    let action_index = *position;
                    let span = self.program.action_span(rule_index, action_index);
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("if ");
                    self.emit_value(
                        condition,
                        self.program
                            .action_argument_span(rule_index, action_index, 0),
                    );
                    self.out.push_str(":\n");
                    *position += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    while let Some(Action::ElseIf { condition }) = actions.get(*position) {
                        let action_index = *position;
                        self.out.push_str(&Self::indent(level));
                        self.out.push_str("elif ");
                        self.emit_value(
                            condition,
                            self.program
                                .action_argument_span(rule_index, action_index, 0),
                        );
                        self.out.push_str(":\n");
                        *position += 1;
                        self.emit_action_block(rule_index, actions, position, level + 1);
                    }
                    if matches!(actions.get(*position), Some(Action::Else)) {
                        *position += 1;
                        self.out.push_str(&Self::indent(level));
                        self.out.push_str("else:\n");
                        self.emit_action_block(rule_index, actions, position, level + 1);
                    }
                    if matches!(actions.get(*position), Some(Action::End)) {
                        *position += 1;
                    } else {
                        self.issue("unsupported-control-flow", "if action is missing End", span);
                    }
                }
                Action::While { condition } => {
                    let action_index = *position;
                    let span = self.program.action_span(rule_index, action_index);
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("while ");
                    self.emit_value(
                        condition,
                        self.program
                            .action_argument_span(rule_index, action_index, 0),
                    );
                    self.out.push_str(":\n");
                    *position += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    self.require_end(rule_index, actions, position, "while", span);
                }
                Action::ForGlobalVariable {
                    variable,
                    start,
                    stop,
                    step,
                } => {
                    let action_index = *position;
                    let span = self.program.action_span(rule_index, action_index);
                    let Some(declaration) = self
                        .program
                        .global_variables
                        .iter()
                        .find(|declaration| declaration.name == *variable)
                    else {
                        self.issue(
                            "unsupported-dangling",
                            format!("unknown loop variable '{variable}'"),
                            span,
                        );
                        *position += 1;
                        self.emit_action_block(rule_index, actions, position, level + 1);
                        self.require_end(rule_index, actions, position, "for", span);
                        continue;
                    };
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("for ");
                    self.out
                        .push_str(emitted(&self.global_names, &declaration.name));
                    self.out.push_str(" in range(");
                    for (arg_index, value) in [start, stop, step].into_iter().enumerate() {
                        if arg_index > 0 {
                            self.out.push_str(", ");
                        }
                        self.emit_value(
                            value,
                            self.program
                                .action_argument_span(rule_index, action_index, arg_index),
                        );
                    }
                    self.out.push_str("):\n");
                    *position += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    self.require_end(rule_index, actions, position, "for", span);
                }
                Action::ForPlayerVariable { .. } => {
                    let action_index = *position;
                    let span = self.program.action_span(rule_index, action_index);
                    self.issue(
                        "unsupported-per-player-loop",
                        "For Player Variable is outside the reconstruction surface \
                         (the OPY `for` form binds a global variable)",
                        span,
                    );
                    *position += 1;
                    self.skip_action_block(actions, position);
                }
                _ => {
                    let action_index = *position;
                    *position += 1;
                    self.emit_action(rule_index, action_index, action, level);
                }
            }
        }
    }

    fn require_end(
        &mut self,
        rule_index: usize,
        actions: &[Action],
        position: &mut usize,
        kind: &str,
        span: Option<Span>,
    ) {
        if matches!(actions.get(*position), Some(Action::End)) {
            *position += 1;
        } else {
            self.issue(
                "unsupported-control-flow",
                format!("{kind} action is missing End"),
                span.or_else(|| self.program.action_span(rule_index, *position)),
            );
        }
    }

    fn skip_action_block(&self, actions: &[Action], position: &mut usize) {
        let mut depth = 1usize;
        while let Some(action) = actions.get(*position) {
            *position += 1;
            match action {
                Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. } => depth += 1,
                Action::End => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn indent(level: usize) -> String {
        "    ".repeat(level)
    }

    fn emit_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        action: &Action,
        level: usize,
    ) {
        let span = self.program.action_span(rule_index, action_index);
        let indent = Self::indent(level);
        match action {
            Action::SetGlobalVariable { variable, value } => {
                if self.set_has_modify_pattern(value, variable, true) {
                    self.issue(
                        "unsupported-set-binary",
                        format!(
                            "Set Global Variable('{}', <binary over the same variable>) \
                             re-lowers to a Modify action; emit the modify form",
                            variable
                        ),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out.push_str(emitted(&self.global_names, variable));
                self.out.push_str(" = ");
                self.emit_value(
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push('\n');
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
            } => {
                let variable = emitted(&self.global_names, variable).to_string();
                self.emit_modify(
                    level,
                    &variable,
                    *op,
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
            } => {
                if !self.is_event_player(player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "Set Player Variable targets a non-event-player expression; \
                         the OPY surface only exposes eventPlayer.member"
                            .to_string(),
                        span,
                    );
                    return;
                }
                if self.set_has_modify_pattern(value, variable, false) {
                    self.issue(
                        "unsupported-set-binary",
                        format!(
                            "Set Player Variable('{}', <binary over the same variable>) \
                             re-lowers to a Modify action; emit the modify form",
                            variable
                        ),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out.push_str("eventPlayer.");
                self.out.push_str(emitted(&self.player_names, variable));
                self.out.push_str(" = ");
                self.emit_value(
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 1),
                );
                self.out.push('\n');
            }
            Action::ModifyPlayerVariable {
                player,
                variable,
                op,
                value,
            } => {
                if !self.is_event_player(player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "Modify Player Variable targets a non-event-player expression; \
                         the OPY surface only exposes eventPlayer.member"
                            .to_string(),
                        span,
                    );
                    return;
                }
                let variable = emitted(&self.player_names, variable).to_string();
                self.emit_modify(
                    level,
                    &format!("eventPlayer.{variable}"),
                    *op,
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 1),
                );
            }
            Action::AssignMember { .. } => {
                self.issue(
                    "unsupported-member-assignment",
                    "dynamic member assignments are outside the OPY reconstruction surface",
                    span,
                );
            }
            Action::CallSubroutine { subroutine } => {
                if !self.subroutine_names.contains(subroutine) {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown subroutine '{subroutine}'"),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out
                    .push_str(emitted(&self.subroutine_emitted, subroutine));
                self.out.push_str("()\n");
            }
            Action::If { .. } | Action::ElseIf { .. } | Action::Else | Action::End => self.issue(
                "unsupported-control-flow",
                "unexpected control-flow marker in Workshop action sequence",
                span,
            ),
            Action::While { .. } => self.issue(
                "unsupported-control-flow",
                "unexpected while action in Workshop action sequence",
                span,
            ),
            Action::ForGlobalVariable { .. } => self.issue(
                "unsupported-control-flow",
                "unexpected for action in Workshop action sequence",
                span,
            ),
            Action::ForPlayerVariable { .. } => {
                self.issue(
                    "unsupported-per-player-loop",
                    "For Player Variable is outside the reconstruction surface \
                     (the OPY `for` form binds a global variable)",
                    span,
                );
            }
            Action::Disabled { .. } => self.issue(
                "unsupported-disabled-action",
                "disabled actions are outside the OPY reconstruction surface",
                span,
            ),
            Action::Call { name, args } => {
                let argument_spans = (0..args.len())
                    .map(|index| {
                        self.program
                            .action_argument_span(rule_index, action_index, index)
                    })
                    .collect::<Vec<_>>();
                self.emit_call_action(name, args, &indent, span, &argument_spans);
            }
        }
    }

    /// `x = x <op> v` (or the player form) re-lowers to a Modify action, so a
    /// Set whose value matches the pattern cannot be reconstructed as a Set.
    fn set_has_modify_pattern(&self, value: &Value, variable: &str, global: bool) -> bool {
        let Value::Call { name, args } = value else {
            return false;
        };
        if !matches!(name.as_str(), "+" | "-" | "*" | "/" | "%" | "**") {
            return false;
        }
        args.len() == 2 && args.iter().any(|operand| {
            if global {
                matches!(operand, Value::GlobalVariable(name) if name == variable)
            } else {
                matches!(operand, Value::PlayerVariable { variable: name, .. } if name == variable)
            }
        })
    }

    fn is_event_player(&self, value: &Value) -> bool {
        matches!(value, Value::EventPlayer)
    }

    fn emit_modify(
        &mut self,
        level: usize,
        name: &str,
        op: ModifyOp,
        value: &Value,
        span: Option<Span>,
    ) {
        let indent = Self::indent(level);
        match op {
            ModifyOp::AppendToArray => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(".append(");
                self.emit_value(value, span);
                self.out.push_str(")\n");
            }
            ModifyOp::RemoveFromArrayByValue => {
                self.issue(
                    "unsupported-modify-op",
                    "Modify ... Remove From Array is outside the reconstruction surface \
                     (the OPY surface has no remove-from-array form)",
                    span,
                );
            }
            ModifyOp::RemoveFromArrayByIndex => {
                self.issue(
                    "unsupported-modify-op",
                    "Modify ... Remove From Array By Index is outside the reconstruction \
                     surface (the OPY surface has no indexed remove-from-array form)",
                    span,
                );
            }
            ModifyOp::Min | ModifyOp::Max => {
                self.issue(
                    "unsupported-modify-op",
                    format!(
                        "Modify ... {} is outside the reconstruction surface \
                         (the OPY surface has no equivalent modification form)",
                        match op {
                            ModifyOp::Min => "Min",
                            ModifyOp::Max => "Max",
                            _ => unreachable!(),
                        }
                    ),
                    span,
                );
            }
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Multiply
            | ModifyOp::Divide
            | ModifyOp::Modulo
            | ModifyOp::RaiseToPower => {
                let operator = match op {
                    ModifyOp::Add => "+",
                    ModifyOp::Subtract => "-",
                    ModifyOp::Multiply => "*",
                    ModifyOp::Divide => "/",
                    ModifyOp::Modulo => "%",
                    ModifyOp::RaiseToPower => "**",
                    _ => unreachable!(),
                };
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(" = ");
                self.out.push_str(name);
                self.out.push(' ');
                self.out.push_str(operator);
                self.out.push(' ');
                self.emit_value(value, span);
                self.out.push('\n');
            }
            _ => self.issue(
                "unsupported-modify-op",
                format!("Workshop modification operation {op:?} has no OPY form"),
                span,
            ),
        }
    }

    /// A generic or member action call in statement position.
    fn emit_call_action(
        &mut self,
        name: &str,
        args: &[Value],
        indent: &str,
        span: Option<Span>,
        argument_spans: &[Option<Span>],
    ) {
        if DEDICATED_ACTION_NAMES.contains(&name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action call '{name}' is lowered to a dedicated Workshop action \
                     and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        let (entry, member) =
            match Self::resolve_call_entry(self.manifest, name, FunctionKind::Action, args.len()) {
                Ok(call) => call,
                Err(message) => {
                    self.issue("unsupported-action-call", message, span);
                    return;
                }
            };
        // A bare call spelled like a declared subroutine resolves to the
        // subroutine on re-lowering — at any arity — so an emitted call
        // whose spelling collides with an emitted subroutine name cannot
        // reproduce the builtin.
        if !member && self.emitted_subroutine_names.contains(entry.id.as_str()) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action '{name}' is ambiguous with a subroutine of the same \
                     emitted name on the OPY surface"
                ),
                span,
            );
            return;
        }
        self.out.push_str(indent);
        self.emit_manifest_call(entry, args, member, span, argument_spans);
        self.out.push('\n');
    }

    fn resolve_call_entry<'manifest>(
        manifest: &'manifest Manifest,
        name: &str,
        expected: FunctionKind,
        args_len: usize,
    ) -> Result<(&'manifest Function, bool), String> {
        let expected_action = expected.is_action();
        // `name` is a canonical Workshop call identity, so an entry that
        // links to it through `catalogId` is the same builtin under its
        // upstream source spelling (`id`). Distinct spellings may share one
        // canonical id (`getPlayers` aliases `getAllPlayers`, and
        // `getRealPlayersInRadius` expands to a filtered `getPlayersInRadius`
        // call); prefer the entry whose own id is the canonical name so the
        // emitted spelling reproduces the same bare call.
        let by_catalog = |entry: &&Function| {
            entry.catalog_id.as_deref() == Some(name)
                && entry.kind.is_action() == expected_action
                && {
                    let provided = args_len.saturating_sub(usize::from(entry.kind.is_member()));
                    provided == entry.params.len()
                }
        };
        let (entry, member) = match manifest
            .functions()
            .iter()
            .filter(by_catalog)
            .min_by_key(|entry| entry.id.as_str() != name)
        {
            Some(entry) => (entry, entry.kind.is_member()),
            None => match manifest.resolve_function(name) {
                Some(entry) => (entry, false),
                None => match manifest.resolve_member(name) {
                    Some(entry) => (entry, true),
                    None => {
                        let message = if expected_action {
                            format!(
                                "action call '{name}' has no OPY source form on the \
                                 reconstruction surface"
                            )
                        } else {
                            format!(
                                "value call '{name}' has no OPY source form on the \
                                 reconstruction surface"
                            )
                        };
                        return Err(message);
                    }
                },
            },
        };
        if expected_action != entry.kind.is_action() {
            let message = match (expected_action, member) {
                (true, true) => format!(
                    "member value '{name}' cannot be emitted as an action on \
                     the reconstruction surface"
                ),
                (true, false) => format!(
                    "value function '{name}' cannot be emitted as an action on \
                     the reconstruction surface"
                ),
                (false, true) => format!(
                    "member action '{name}' cannot be emitted as a value on \
                     the reconstruction surface"
                ),
                (false, false) => format!(
                    "action function '{name}' cannot be emitted as a value on \
                     the reconstruction surface"
                ),
            };
            return Err(message);
        }
        Ok((entry, member))
    }

    /// Emit a manifest function call with explicit full-arity arguments, no
    /// indent and no trailing newline (the caller frames the line). The OPY
    /// frontend fills declared defaults at recompile time, so any Workshop call
    /// that omits a defaulted or required parameter cannot be reconstructed
    /// identically and is rejected.
    fn emit_manifest_call(
        &mut self,
        entry: &Function,
        args: &[Value],
        member: bool,
        span: Option<Span>,
        argument_spans: &[Option<Span>],
    ) {
        let (receiver, params) = if member {
            match args.split_first() {
                Some((receiver, rest)) => (Some(receiver), rest),
                None => {
                    self.issue(
                        "unsupported-invalid-arity",
                        format!("member '{}' requires a receiver argument", entry.id),
                        span,
                    );
                    return;
                }
            }
        } else {
            (None, args)
        };
        let name = entry.id.as_str();
        if params.len() > entry.params.len() {
            self.issue(
                "unsupported-invalid-arity",
                format!(
                    "{} '{}' expects at most {} arguments but the Workshop call carries {}",
                    kind_label(entry.kind),
                    name,
                    entry.params.len(),
                    params.len()
                ),
                span,
            );
            return;
        }
        // Every parameter beyond the provided arguments must be omittable
        // (`optional`). A required parameter (with or without a declared
        // default) cannot be omitted: the OPY frontend would reject it or
        // fill its default, changing the resulting Workshop program.
        for (_index, param) in entry.params.iter().enumerate().skip(params.len()) {
            if !param.optional {
                self.issue(
                    "unsupported-missing-argument",
                    format!(
                        "{} '{}' omits parameter '{}'; the OPY frontend would \
                         reject or default-fill it and change the resulting Workshop program",
                        kind_label(entry.kind),
                        name,
                        param.name
                    ),
                    span,
                );
            }
        }

        if let Some(receiver) = receiver {
            let receiver_span = argument_spans.first().copied().flatten().or(span);
            self.emit_value(receiver, receiver_span);
            self.out.push('.');
        }
        self.out.push_str(name);
        self.out.push('(');
        // Cross-check through the Workshop catalog: a manifest entry with a
        // declared `catalogId` must resolve there under the matching kind and
        // the reconstruction locale (mirroring the manifest's own catalog
        // cross-check test), so the reconstruction identity layer never
        // drifts from the catalog.
        if let Some(catalog_id) = &entry.catalog_id {
            let expected_kind = match entry.kind {
                FunctionKind::Action | FunctionKind::MemberAction => {
                    workshop_rs::catalog::Kind::Action
                }
                FunctionKind::Value | FunctionKind::MemberValue => {
                    workshop_rs::catalog::Kind::Value
                }
            };
            if self
                .catalog
                .spelling(expected_kind, self.locale, catalog_id)
                .is_none()
            {
                self.issue(
                    "catalog-error",
                    format!(
                        "manifest entry '{}' links catalogId '{catalog_id}' which is \
                         missing from the Workshop catalog",
                        entry.id
                    ),
                    span,
                );
            }
        }
        for (index, arg) in params.iter().enumerate() {
            if index > 0 {
                self.out.push_str(", ");
            }
            let argument_index = index + if member { 1 } else { 0 };
            let argument_span = argument_spans
                .get(argument_index)
                .copied()
                .flatten()
                .or(span);
            self.check_param_argument(entry, index, arg, argument_span);
            self.emit_value(arg, argument_span);
        }
        self.out.push(')');
    }

    /// Validate a provided argument against its manifest parameter: enum
    /// domains are enforced (like the frontend) and variable-required
    /// parameters must be variable references. The variable requirement is
    /// typed policy (`policy::variable_args`); the manifest's
    /// `param.variable` flag records the same fact descriptively and does
    /// not select this check (issue #458).
    fn check_param_argument(
        &mut self,
        entry: &Function,
        index: usize,
        arg: &Value,
        span: Option<Span>,
    ) {
        if let Some(domain) = entry
            .params
            .get(index)
            .and_then(|param| param.domain.as_ref())
        {
            match arg {
                Value::Enum { value_type, value } if value_type == domain => {
                    if !self.enum_member_in_domain(domain, value) {
                        self.issue(
                            "unsupported-enum-member",
                            format!(
                                "argument {} of '{}' uses enum member '{domain}.{value}' \
                                 which is outside the manifest's declared domain",
                                index + 1,
                                entry.id
                            ),
                            span,
                        );
                    }
                }
                Value::Enum { value_type, .. } => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects enum domain '{domain}' but \
                             the Workshop value carries '{value_type}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
                _ => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects an enum member of domain \
                             '{domain}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
            }
        }
        if policy::variable_args(&entry.id).contains(&index) {
            let is_variable =
                matches!(arg, Value::GlobalVariable(_) | Value::PlayerVariable { .. });
            if !is_variable {
                self.issue(
                    "unsupported-invalid-argument",
                    format!(
                        "argument {} of '{}' must be a variable reference",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
        }
    }

    fn enum_member_in_domain(&self, domain: &str, member: &str) -> bool {
        self.catalog.enum_domain(domain).is_some_and(|domain| {
            domain
                .members
                .iter()
                .any(|candidate| candidate.member == member)
        })
    }

    fn emit_value(&mut self, value: &Value, span: Option<Span>) {
        match value {
            Value::Number(value) => {
                if !value.is_finite() {
                    self.issue(
                        "unsupported-non-finite-number",
                        format!("non-finite number literal '{value}' has no OPY spelling"),
                        span,
                    );
                } else if *value < 0.0 {
                    self.issue(
                        "unsupported-negative-number",
                        format!(
                            "negative number literal '{}' has no OPY literal form \
                             (the lexer has no negative-number token)",
                            workshop_rs::format::format_number(*value)
                        ),
                        span,
                    );
                } else {
                    self.out
                        .push_str(&workshop_rs::format::format_number(*value));
                }
            }
            Value::String(value) => self.emit_string_literal(value),
            Value::LocalizedString(value) => {
                self.issue(
                    "unsupported-localized-string",
                    format!("localized Workshop preset string '{value}' has no OPY source representation"),
                    span,
                );
            }
            Value::Bool(value) => {
                self.out.push_str(if *value { "true" } else { "false" });
            }
            Value::Null => {
                self.out.push_str("null");
            }
            Value::Array(elements) => {
                self.out.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_value(element, span);
                }
                self.out.push(']');
            }
            Value::Vector { x, y, z } => {
                self.out.push_str("vect(");
                self.emit_value(x, span);
                self.out.push_str(", ");
                self.emit_value(y, span);
                self.out.push_str(", ");
                self.emit_value(z, span);
                self.out.push(')');
            }
            Value::Enum { value_type, value } => {
                self.emit_enum(value_type, value, span);
            }
            Value::GlobalVariable(variable) => {
                if !self
                    .program
                    .global_variables
                    .iter()
                    .any(|declaration| declaration.name == *variable)
                {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown global variable '{variable}'"),
                        span,
                    );
                    return;
                }
                self.out.push_str(emitted(&self.global_names, variable));
            }
            Value::PlayerVariable { player, variable } => {
                if !self.is_event_player(player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "a player-variable access on a non-event-player expression is \
                         outside the reconstruction surface (only eventPlayer.member \
                         is representable)",
                        span,
                    );
                    return;
                }
                if !self
                    .program
                    .player_variables
                    .iter()
                    .any(|declaration| declaration.name == *variable)
                {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown player variable '{variable}'"),
                        span,
                    );
                    return;
                }
                self.out.push_str("eventPlayer.");
                self.out.push_str(emitted(&self.player_names, variable));
            }
            Value::Subroutine(_) => {
                self.issue(
                    "unsupported-subroutine-value",
                    "subroutine values are outside the OPY reconstruction surface",
                    span,
                );
            }
            Value::EventPlayer => {
                self.out.push_str("eventPlayer");
            }
            Value::Call { name, args } => {
                self.emit_value_call(name, args, span);
            }
        }
    }

    fn emit_enum(&mut self, value_type: &str, value: &str, span: Option<Span>) {
        let Some(domain) = self.catalog.enum_domain(value_type) else {
            self.issue(
                "unsupported-enum-domain",
                format!(
                    "enum domain '{value_type}' is outside the manifest's declared \
                     reconstruction surface"
                ),
                span,
            );
            return;
        };
        if !domain.members.iter().any(|member| member.member == value) {
            self.issue(
                "unsupported-enum-member",
                format!(
                    "enum member '{value_type}.{value}' is outside the manifest's \
                     declared domain"
                ),
                span,
            );
            return;
        }
        // Reconstruction must emit the OverPy source spelling, not the
        // canonical catalog id (`Hero.SOLDIER`, `Map.ROUTE66`, `Clip.NONE`;
        // issue #466).
        let Some((source_name, spelling)) = crate::enums::spelling_of_member(value_type, value)
        else {
            self.issue(
                "unsupported-enum-member",
                format!("enum member '{value_type}.{value}' has no OverPy source spelling"),
                span,
            );
            return;
        };
        self.out.push_str(source_name);
        self.out.push('.');
        self.out.push_str(spelling);
    }

    fn emit_value_call(&mut self, name: &str, args: &[Value], span: Option<Span>) {
        if name == "localPlayer" && args.is_empty() {
            self.out.push_str(name);
            return;
        }
        // Binary and unary operator calls keep their source spelling.
        if BINARY_OPS.contains(&name) && args.len() == 2 {
            self.out.push('(');
            self.emit_value(&args[0], span);
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push(' ');
            self.emit_value(&args[1], span);
            self.out.push(')');
            return;
        }
        if name == "not" && args.len() == 1 {
            self.out.push_str("(not ");
            self.emit_value(&args[0], span);
            self.out.push(')');
            return;
        }
        if name == "-" && args.len() == 1 {
            self.out.push_str("(-");
            self.emit_value(&args[0], span);
            self.out.push(')');
            return;
        }
        // Interpolated strings lower to `customString`; emit the OPY member
        // form `"text".format(args...)`.
        if name == "customString" {
            let Some(first) = args.first() else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a receiver is outside the reconstruction surface",
                    span,
                );
                return;
            };
            let Value::String(text) = first else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a string receiver is outside the \
                     reconstruction surface",
                    span,
                );
                return;
            };
            self.emit_string_literal(text);
            self.out.push_str(".format(");
            for (index, arg) in args.iter().skip(1).enumerate() {
                if index > 0 {
                    self.out.push_str(", ");
                }
                self.emit_value(arg, span);
            }
            self.out.push(')');
            return;
        }
        if DEDICATED_VALUE_NAMES.contains(&name) {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is lowered to a dedicated Workshop value \
                     and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        let (entry, member) =
            match Self::resolve_call_entry(self.manifest, name, FunctionKind::Value, args.len()) {
                Ok(call) => call,
                Err(message) => {
                    self.issue("unsupported-value-call", message, span);
                    return;
                }
            };
        if !member && crate::lower::policy::function_context(&entry.id).is_some() {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is only valid as a for-loop iterable on \
                     the OPY surface"
                ),
                span,
            );
            return;
        }
        // Same subroutine-shadowing rule as actions: a bare call spelled
        // like an emitted subroutine name resolves to the subroutine.
        if !member && self.emitted_subroutine_names.contains(entry.id.as_str()) {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value '{name}' is ambiguous with a subroutine of the same \
                     emitted name on the OPY surface"
                ),
                span,
            );
            return;
        }
        self.emit_manifest_call(entry, args, member, span, &[]);
    }

    fn emit_string_literal(&mut self, value: &str) {
        self.out.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => self.out.push_str("\\\\"),
                '"' => self.out.push_str("\\\""),
                '\n' => self.out.push_str("\\n"),
                '\t' => self.out.push_str("\\t"),
                '\r' => self.out.push_str("\\r"),
                other if other.is_control() => {
                    self.out.push_str(&format!("\\u{:04X}", other as u32));
                }
                other => self.out.push(other),
            }
        }
        self.out.push('"');
    }
}

/// Emitted declaration order: table positions sorted by canonical
/// Workshop index (stable, so equal slots — impossible after the index
/// checks — would keep table order). Re-lowering sorts the table by
/// index, so emitting in index order reproduces the canonical binding.
fn index_order(variables: &[Variable]) -> Vec<(usize, u32)> {
    let mut order: Vec<(usize, u32)> = variables
        .iter()
        .enumerate()
        .map(|(position, variable)| (position, variable.index.unwrap_or(position as u32)))
        .collect();
    order.sort_by_key(|&(_, index)| index);
    order
}

/// The OPY spelling recorded for a canonical table name. `mangle_names`
/// covers every declared name; a missing entry means a duplicate name the
/// caller already reports, so falling back to the canonical spelling is
/// unreachable on a successful reconstruction.
fn emitted<'a>(map: &'a HashMap<String, String>, name: &'a str) -> &'a str {
    map.get(name).map_or(name, String::as_str)
}

/// Whether a canonical name must be mangled to appear on the OPY surface:
/// it is not a well-formed identifier, the pinned reserved-name table
/// refuses it in this declaration namespace, or — for subroutines — a
/// `name()` invocation would resolve to a translation call, a special
/// form, `async`/`startRule`, `compressed`, `vect`, or `range` before the
/// declared-subroutine check runs.
fn needs_mangling(name: &str, namespace: NameNamespace) -> bool {
    !is_identifier(name)
        || is_reserved_name(name, namespace)
        || (namespace == NameNamespace::Subroutine
            && (SHADOWED_SUBROUTINE_NAMES.contains(&name)
                || SpecialValueCall::from_name(name).is_some()))
}

/// Map every declared name to the OPY spelling reconstruction emits for
/// it, under the rule recorded in `docs/architecture/reconstruction.md`:
///
/// * a name that already re-resolves to its declaration (a well-formed
///   identifier outside the reserved and shadowed sets) keeps its
///   spelling;
/// * any other name is mangled deterministically — every identifier
///   continuation character is kept, every other character becomes
///   `_xHH_` (lowercase hex codepoint), and the result is prefixed `v_`
///   when it is empty or does not start with an identifier-start
///   character;
/// * a candidate still rejected by `needs_mangling`, already taken by a
///   table sibling, or equal to another canonical name in the table gains
///   `_opy1`, then `_opy2`, `_opy3`, … until it resolves uniquely.
///
/// A duplicate canonical name is only entered once; the caller reports
/// `unsupported-duplicate-name` for it.
fn mangle_names<'a>(
    names: impl Iterator<Item = &'a str>,
    namespace: NameNamespace,
) -> HashMap<String, String> {
    let names: Vec<&str> = names.collect();
    let canonical: HashSet<&str> = names.iter().copied().collect();
    let mut emitted_names = HashMap::new();
    let mut taken = HashSet::new();
    for name in names {
        if emitted_names.contains_key(name) {
            continue;
        }
        let stem = if needs_mangling(name, namespace) {
            mangle_base(name)
        } else {
            name.to_string()
        };
        let mut emitted_name = stem.clone();
        let mut counter = 0u32;
        while needs_mangling(&emitted_name, namespace)
            || taken.contains(emitted_name.as_str())
            || (emitted_name != name && canonical.contains(emitted_name.as_str()))
        {
            counter += 1;
            emitted_name = format!("{stem}_opy{counter}");
        }
        taken.insert(emitted_name.clone());
        emitted_names.insert(name.to_string(), emitted_name);
    }
    emitted_names
}

/// The stem of a mangled name: identifier continuation characters kept,
/// every other character escaped as `_xHH_` (lowercase hex codepoint),
/// prefixed `v_` when the result cannot start an identifier.
fn mangle_base(name: &str) -> String {
    let mut mangled = String::with_capacity(name.len() + 2);
    for ch in name.chars() {
        if is_ident_continue(ch) {
            mangled.push(ch);
        } else {
            mangled.push_str(&format!("_x{:x}_", ch as u32));
        }
    }
    if !mangled.starts_with(is_ident_start) {
        mangled.insert_str(0, "v_");
    }
    mangled
}

fn kind_label(kind: FunctionKind) -> &'static str {
    match kind {
        FunctionKind::Action => "action",
        FunctionKind::Value => "value",
        FunctionKind::MemberAction => "member action",
        FunctionKind::MemberValue => "member value",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::reconstruct;
    use crate::compiler::Compiler;
    use workshop_rs::catalog::{Catalog, Locale};
    use workshop_rs::source::{Position, SourceFile, Span};
    use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Subroutine, Value, Variable};

    fn compile_to_program(source: &str, path: &str) -> Program {
        let artifact = Compiler::new()
            .expect("compiler loads")
            .compile_source_artifact(source, path, Path::new("."))
            .expect("reconstructed OPY compiles");
        let catalog = Catalog::builtin().expect("catalog loads");
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &Locale::new("en-US"))
            .expect("compiler output parses through the public Workshop parser")
    }

    #[test]
    fn reconstructs_public_program_control_flow() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::If {
                    condition: Value::call(
                        "==",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                })
                .action(Action::ElseIf {
                    condition: Value::call(
                        "<",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(2.0),
                })
                .action(Action::Else)
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(3.0),
                })
                .action(Action::End)
                .action(Action::While {
                    condition: Value::call(
                        "<",
                        [Value::global_variable("score"), Value::number(10.0)],
                    ),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End)
                .action(Action::ForGlobalVariable {
                    variable: "score".into(),
                    start: Value::number(0.0),
                    stop: Value::number(2.0),
                    step: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("canonical Program should reconstruct");
        assert!(source.contains("if (score == 0):"));
        assert!(source.contains("elif (score < 0):"));
        assert!(source.contains("else:"));
        assert!(source.contains("while (score < 10):"));
        assert!(source.contains("for score in range(0, 2, 1):"));

        let reparsed = compile_to_program(&source, "reconstructed.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn initializer_name_with_another_event_remains_an_ordinary_rule() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::EachPlayer).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("ordinary rule should reconstruct");
        assert!(source.contains("rule \"Initialize global variables\":"));
        assert!(source.contains("@Event eachPlayer"));
        let reparsed = compile_to_program(&source, "initializer-name.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_initializer_is_not_reconstructed_as_a_declaration_initializer() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        let mut initializer = Rule::new("Initialize global variables", Event::Global).action(
            Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            },
        );
        initializer.disabled = true;
        program.rule(initializer);

        let error = reconstruct(&program).expect_err("disabled initializer cannot be emitted");
        assert_eq!(error.issues[0].code, "unsupported-disabled-rule");
    }

    #[test]
    fn disabled_subroutine_rule_is_not_reconstructed_as_an_active_definition() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 0));
        let mut subroutine = Rule::new("Subroutine reset", Event::Subroutine("reset".into()))
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(0.0),
            });
        subroutine.disabled = true;
        program.rule(subroutine);

        let error = reconstruct(&program).expect_err("disabled subroutine cannot be emitted");
        assert_eq!(error.issues[0].code, "unsupported-disabled-rule");
    }

    #[test]
    fn escaped_rule_names_remain_valid_opy_strings() {
        let rule_name = "quoted \"rule\" \\ path\nnext\u{0001}";
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(rule_name, Event::Global).action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("special characters should be escaped");
        let artifact = Compiler::new()
            .expect("compiler loads")
            .compile_source_artifact(&source, "escaped-name.opy", Path::new("."))
            .expect("escaped OPY rule name parses");
        assert_eq!(artifact.wir.rules[0].name, rule_name);
    }

    #[test]
    fn reconstructs_public_subroutine_names() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(0.0),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::Global).action(Action::CallSubroutine {
                subroutine: "reset".into(),
            }),
        );

        let source = reconstruct(&program).expect("canonical subroutine should reconstruct");
        assert!(source.contains("def reset():"));
        assert!(source.contains("reset()"));

        let reparsed = compile_to_program(&source, "subroutine.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn reconstructs_pinned_subroutine_index() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 3));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(0.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("pinned subroutine index should reconstruct");
        assert!(source.contains("subroutine reset 3\n"), "{source}");

        let reparsed = compile_to_program(&source, "subroutine-index.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn action_argument_diagnostics_use_public_argument_spans() {
        let mut program = Program::new();
        let file = program.add_file(SourceFile::new("main.opy"));
        program.rule(Rule::new("main", Event::Global).action(Action::call(
            "chaseAtRate",
            [
                Value::number(1.0),
                Value::number(10.0),
                Value::number(1.0),
                Value::Enum {
                    value_type: "ChaseRateReeval".into(),
                    value: "DESTINATION_AND_RATE".into(),
                },
            ],
        )));
        let action_span = Span::new(file, Position::new(1, 1), Position::new(1, 25));
        let argument_span = Span::new(file, Position::new(1, 13), Position::new(1, 14));
        program.set_action_span(0, 0, Some(action_span)).unwrap();
        program
            .set_action_argument_span(0, 0, 0, Some(argument_span))
            .unwrap();

        let error = reconstruct(&program).expect_err("the first parameter must be a variable");
        assert_eq!(error.issues[0].code, "unsupported-invalid-argument");
        assert_eq!(error.issues[0].span, Some(argument_span));
    }

    #[test]
    fn rejects_public_program_actions_without_an_opy_form() {
        let mut program = Program::new();
        program.rule(
            Rule::new("main", Event::Global).action(Action::AssignMember {
                target: Value::EventPlayer,
                op: None,
                value: Value::number(1.0),
            }),
        );

        let error = reconstruct(&program).expect_err("dynamic member assignment is unsupported");
        assert_eq!(error.issues[0].code, "unsupported-member-assignment");
    }

    #[test]
    fn catalog_identity_prefers_the_canonical_source_spelling() {
        // `getRealPlayersInRadius` shares `getPlayersInRadius`'s catalog id; a
        // bare canonical call is the unfiltered builtin, not the macro that
        // would recompile to an extra `Filtered Array` wrapper.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call(
                    "getPlayersInRadius",
                    [
                        Value::EventPlayer,
                        Value::number(3.0),
                        Value::Enum {
                            value_type: "Team".into(),
                            value: "ALL".into(),
                        },
                        Value::Enum {
                            value_type: "LosCheck".into(),
                            value: "OFF".into(),
                        },
                    ],
                ),
            }),
        );

        let source = reconstruct(&program).expect("canonical call should reconstruct");
        assert!(source.contains("getPlayersInRadius("));
        assert!(!source.contains("getRealPlayersInRadius"));

        let reparsed = compile_to_program(&source, "players-in-radius.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn null_values_reconstruct_with_the_null_spelling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::Null,
            }),
        );

        let source = reconstruct(&program).expect("null should reconstruct");
        assert!(source.contains("g = null"), "{source}");
        assert!(!source.contains("None"), "{source}");

        let reparsed = compile_to_program(&source, "null.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn custom_string_calls_reconstruct_as_format() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call(
                    "customString",
                    [Value::string("hp: {0}"), Value::EventPlayer],
                ),
            }),
        );

        let source = reconstruct(&program).expect("customString should reconstruct");
        assert!(source.contains("\"hp: {0}\".format(eventPlayer)"));

        let reparsed = compile_to_program(&source, "format.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn reconstruction_begins_with_optimize_strict() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("canonical Program should reconstruct");
        assert!(source.starts_with("#!optimizeStrict\n"), "{source}");
    }

    #[test]
    fn optimize_strict_preserves_add_zero() {
        // Without `#!optimizeStrict` the frontend's default folding would
        // rewrite `h + 0` to `h`, changing the canonical program.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.global_variable(Variable::with_index("h", 1));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call("+", [Value::global_variable("h"), Value::number(0.0)]),
            }),
        );

        let source = reconstruct(&program).expect("Add(x, 0) should reconstruct");
        assert!(source.starts_with("#!optimizeStrict"), "{source}");
        assert!(source.contains("h + 0"), "{source}");

        let reparsed = compile_to_program(&source, "add-zero.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn out_of_order_global_indices_reconstruct_sorted_with_explicit_indices() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("zed", 3));
        program.global_variable(Variable::with_index("alpha", 0));
        program.global_variable(Variable::with_index("mid", 2));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "zed".into(),
                    value: Value::number(1.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "alpha".into(),
                    value: Value::global_variable("mid"),
                }),
        );

        let source = reconstruct(&program).expect("out-of-order indices should reconstruct");
        let alpha = source
            .find("globalvar alpha 0")
            .expect("alpha declares its index");
        let mid = source
            .find("globalvar mid 2")
            .expect("mid declares its index");
        let zed = source
            .find("globalvar zed 3")
            .expect("zed declares its index");
        assert!(alpha < mid && mid < zed, "{source}");

        let reparsed = compile_to_program(&source, "out-of-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn out_of_order_player_indices_reconstruct_sorted_with_explicit_indices() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("zed", 3));
        program.player_variable(Variable::with_index("alpha", 0));
        program.rule(
            Rule::new("main", Event::EachPlayer).action(Action::SetPlayerVariable {
                player: Value::EventPlayer,
                variable: "alpha".into(),
                value: Value::player_variable(Value::EventPlayer, "zed"),
            }),
        );

        let source = reconstruct(&program).expect("out-of-order player indices reconstruct");
        let alpha = source
            .find("playervar alpha 0")
            .expect("alpha declares its index");
        let zed = source
            .find("playervar zed 3")
            .expect("zed declares its index");
        assert!(alpha < zed, "{source}");

        let reparsed = compile_to_program(&source, "player-out-of-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn names_outside_the_opy_surface_reconstruct_with_recorded_mangling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("for", 0));
        program.global_variable(Variable::with_index("1st place", 1));
        program.player_variable(Variable::with_index("x", 0));
        program.subroutine(Subroutine::with_index("sorted", 0));
        program.rule(
            Rule::new("Subroutine sorted", Event::Subroutine("sorted".into())).action(
                Action::SetGlobalVariable {
                    variable: "for".into(),
                    value: Value::global_variable("1st place"),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::EachPlayer)
                .action(Action::SetPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "x".into(),
                    value: Value::number(0.0),
                })
                .action(Action::CallSubroutine {
                    subroutine: "sorted".into(),
                }),
        );

        let source = reconstruct(&program).expect("mangled names should reconstruct");
        assert!(source.contains("globalvar for_opy1 0"), "{source}");
        assert!(source.contains("globalvar v_1st_x20_place 1"), "{source}");
        assert!(source.contains("playervar x_opy1 0"), "{source}");
        assert!(source.contains("subroutine sorted_opy1\n"), "{source}");
        assert!(source.contains("def sorted_opy1():"), "{source}");
        assert!(source.contains("for_opy1 = v_1st_x20_place"), "{source}");
        assert!(source.contains("eventPlayer.x_opy1 = 0"), "{source}");
        assert!(source.contains("sorted_opy1()"), "{source}");

        let reparsed = compile_to_program(&source, "mangled.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));

        // The mangling rule is deterministic: the same input program always
        // maps to the same output.
        let again = reconstruct(&program).expect("reconstruction is deterministic");
        assert_eq!(source, again);
    }

    #[test]
    fn duplicate_table_names_still_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("dup", 0));
        program.global_variable(Variable::with_index("dup", 1));

        let error = reconstruct(&program).expect_err("duplicate names refuse");
        assert_eq!(error.issues[0].code, "unsupported-duplicate-name");
    }

    #[test]
    fn declaration_indices_outside_the_slot_range_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 200));

        let error = reconstruct(&program).expect_err("out-of-range index refuses");
        assert_eq!(error.issues[0].code, "unsupported-index");
    }

    #[test]
    fn colliding_declaration_indices_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("a", 0));
        program.global_variable(Variable::with_index("b", 0));

        let error = reconstruct(&program).expect_err("colliding indices refuse");
        assert_eq!(error.issues[0].code, "unsupported-index");
    }
}

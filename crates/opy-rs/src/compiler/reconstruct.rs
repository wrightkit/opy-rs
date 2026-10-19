use std::collections::{HashMap, HashSet};
use std::fmt;

use workshop_rs::Program;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::program::{Action, Event, EventTarget, EventTeam, ModifyOp, Rule, Value};
use workshop_rs::source::Span;

use crate::lexer::is_identifier;
use crate::lower::policy;
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

/// OPY names the parser treats as keywords or literals; a Workshop table name
/// that collides with one of these cannot be referenced or declared faithfully.
const RESERVED_NAMES: &[&str] = &[
    "true",
    "false",
    "null",
    "eventPlayer",
    "rule",
    "def",
    "globalvar",
    "playervar",
    "subroutine",
    "enum",
    "macro",
    "if",
    "for",
    "while",
    "pass",
    "elif",
    "else",
    "in",
    "and",
    "or",
    "not",
];

/// Binary operator spellings the OPY frontend lowers to `Value::Call`s with
/// the same name (source operators, not Workshop spellings like `add`).
const BINARY_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "and", "or",
];

/// Canonical arithmetic call identities that reconstruct as source-level
/// binary operators (`Add(x, y)` ↔ `x + y`).
const ARITHMETIC_OPS: &[(&str, &str)] = &[
    ("add", "+"),
    ("subtract", "-"),
    ("multiply", "*"),
    ("divide", "/"),
    ("modulo", "%"),
    ("raiseToPower", "**"),
];

/// Call names the frontend lowers to dedicated Workshop actions or values.
const DEDICATED_ACTION_NAMES: &[&str] = &["append"];
const DEDICATED_VALUE_NAMES: &[&str] = &["vect", "range", "chase"];

/// Operation argument spellings an indexed `modify*AtIndex` call carries as a
/// zero-argument canonical call; the OPY spelling is the assignment operator
/// that re-lowers to the same indexed modification.
const INDEXED_MODIFY_OPS: &[(&str, &str)] = &[
    ("add", "+="),
    ("subtract", "-="),
    ("multiply", "*="),
    ("divide", "/="),
    ("modulo", "%="),
    ("raiseToPower", "**="),
    ("min", "min="),
    ("max", "max="),
];

struct Emitter<'a> {
    program: &'a Program,
    manifest: &'a Manifest,
    catalog: &'a Catalog,
    locale: &'a Locale,
    issues: Vec<ReconstructIssue>,
    out: String,
    /// Subroutine names, for call-vs-subroutine ambiguity checks.
    subroutine_names: HashSet<String>,
    /// Every declared variable/subroutine name; generated identifiers must
    /// not collide with them.
    declared_names: HashSet<String>,
    /// Generated `goto` labels by flat action index inside the rule currently
    /// being emitted (the boundary the label statement precedes).
    labels: HashMap<usize, Vec<String>>,
    /// Generated label name by skip/skipIf action index.
    skip_labels: HashMap<usize, String>,
    /// Deterministic counters for generated label and lambda binder names.
    generated_counter: usize,
    /// Lambda binder names for nested `map`/`filter`/`sorted`/`all`/`any`
    /// emission: `(element, index)` per enclosing generated lambda.
    lambda_binders: Vec<(String, Option<String>)>,
    /// Nesting depth of `while`/`for` bodies currently being emitted; the
    /// canonical `break` action only has an OPY form inside a loop.
    loop_depth: usize,
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
        let subroutine_names = program
            .subroutines
            .iter()
            .map(|subroutine| subroutine.name.clone())
            .collect();
        let declared_names = program
            .global_variables
            .iter()
            .chain(&program.player_variables)
            .map(|variable| variable.name.clone())
            .chain(program.subroutines.iter().map(|s| s.name.clone()))
            .collect();
        Emitter {
            program,
            manifest,
            catalog,
            locale,
            issues: Vec::new(),
            out: String::new(),
            subroutine_names,
            declared_names,
            labels: HashMap::new(),
            skip_labels: HashMap::new(),
            generated_counter: 0,
            lambda_binders: Vec::new(),
            loop_depth: 0,
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
        // Global table: unique names, valid OPY identifiers, non-decreasing
        // slot order (the frontend's re-lowering sorts the table by index,
        // so only slot-ordered input reproduces the same table).
        let mut previous_index: Option<u32> = None;
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            let index = variable.index.unwrap_or(position as u32);
            self.check_variable_name(&variable.name, None, "global variable");
            self.check_duplicate_name(&variable.name, position, "global variable", None);
            if let Some(previous) = previous_index {
                if index < previous {
                    self.issue(
                        "unsupported-global-order",
                        format!(
                            "global variables must be in ascending index order \
                             (slot {} precedes slot {})",
                            previous, index
                        ),
                        None,
                    );
                }
            }
            previous_index = Some(index);
        }
        // Player table: unique names and valid identifiers; player slots are
        // explicit in the `playervar name <index>` form, so no order rule.
        for (position, variable) in self.program.player_variables.iter().enumerate() {
            self.check_variable_name(&variable.name, None, "player variable");
            self.check_duplicate_name(&variable.name, position, "player variable", None);
        }
        // Subroutine table: unique names and valid identifiers; a slot that
        // differs from the table position is carried by `subroutine name <index>`.
        for (position, subroutine) in self.program.subroutines.iter().enumerate() {
            self.check_variable_name(&subroutine.name, None, "subroutine");
            self.check_duplicate_name(&subroutine.name, position, "subroutine", None);
        }
    }

    fn check_variable_name(&mut self, name: &str, span: Option<Span>, kind: &str) {
        if !is_identifier(name) {
            self.issue(
                "unsupported-name",
                format!(
                    "{kind} name '{name}' is not a valid OPY identifier on the \
                     reconstruction surface"
                ),
                span,
            );
        } else if RESERVED_NAMES.contains(&name) {
            self.issue(
                "unsupported-name",
                format!(
                    "{kind} name '{name}' collides with an OPY keyword or literal \
                     and cannot be referenced on the reconstruction surface"
                ),
                span,
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
        self.check_initializer_slot(&global_initializers);

        self.emit_variable_declarations(true, &global_initializers);
        self.emit_variable_declarations(false, &player_initializers);
        if self.program.subroutines.is_empty() {
            self.out.push('\n');
        } else {
            for (position, subroutine) in self.program.subroutines.iter().enumerate() {
                self.out.push_str("subroutine ");
                self.out.push_str(&subroutine.name);
                if let Some(index) = subroutine.index.filter(|index| *index as usize != position) {
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
            self.out.push_str(&definition.name);
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

    fn emit_variable_declarations(
        &mut self,
        global: bool,
        initializers: &std::collections::HashMap<usize, (Value, Option<Span>)>,
    ) {
        let (kind, variables) = if global {
            ("globalvar ", &self.program.global_variables)
        } else {
            ("playervar ", &self.program.player_variables)
        };
        for (position, variable) in variables.iter().enumerate() {
            self.out.push_str(kind);
            self.out.push_str(&variable.name);
            match initializers.get(&position) {
                Some(value) => {
                    self.out.push_str(" = ");
                    self.emit_initializer(&value.0, value.1);
                }
                None => {
                    self.out.push(' ');
                    self.out
                        .push_str(&variable.index.unwrap_or(position as u32).to_string());
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
        let mut previous: Option<usize> = None;
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
            self.record_initializer_position(
                &mut previous,
                variable_position,
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
        previous: &mut Option<usize>,
        position: usize,
        variable: &str,
        table: &str,
        span: Option<Span>,
    ) {
        if previous.is_some_and(|previous| position <= previous) {
            self.issue(
                "unsupported-init-rule",
                format!(
                    "initializer rule Sets '{variable}' out of {table} table order; \
                     the frontend synthesizes initializers in declaration order"
                ),
                span,
            );
        }
        *previous = Some(position);
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

    /// The OPY declaration `globalvar name = value` cannot carry an explicit
    /// slot, so the frontend re-lowering assigns the lowest free slot. An
    /// initializer-bearing global is only representable when that slot equals
    /// its Workshop index; otherwise the reconstructed table would differ.
    fn check_initializer_slot(
        &mut self,
        initializers: &std::collections::HashMap<usize, (Value, Option<Span>)>,
    ) {
        let mut taken: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            if initializers.contains_key(&position) {
                let mut next_free = 0u32;
                while taken.contains(&next_free) {
                    next_free += 1;
                }
                let index = variable.index.unwrap_or(position as u32);
                if next_free != index {
                    self.issues.push(ReconstructIssue {
                        code: "unsupported-indexed-initializer",
                        message: format!(
                            "initializer-bearing global '{}' occupies slot {} but the \
                             OPY `globalvar name = value` form assigns the lowest free \
                             slot ({}) on re-lowering",
                            variable.name, index, next_free
                        ),
                        span: None,
                    });
                }
                taken.insert(next_free);
            } else {
                taken.insert(variable.index.unwrap_or(position as u32));
            }
        }
    }

    fn emit_actions(&mut self, rule_index: usize, actions: &[Action], level: usize) {
        self.labels.clear();
        self.skip_labels.clear();
        self.plan_labels(rule_index, actions);
        let mut position = 0;
        self.emit_action_block(rule_index, actions, &mut position, level);
        self.emit_labels_at(actions.len(), level);
        if position < actions.len() {
            self.issue(
                "unsupported-control-flow",
                "unexpected control-flow marker in Workshop action sequence",
                self.program.action_span(rule_index, position),
            );
        }
    }

    /// `skip`/`skipIf` carry a native-action distance; every canonical action
    /// in the flat sequence (including `If`/`Else`/`End` markers) is exactly
    /// one native line, so the target boundary is `index + 1 + distance`.
    /// For each literal-distance skip we reserve a generated label at that
    /// boundary; the label statement is emitted by `emit_labels_at`.
    /// Generated names avoid every declared program identifier.
    ///
    /// A label is only usable when its boundary sits in the skip's own
    /// statement list or an enclosing one — the OPY goto resolution rejects
    /// jumps into a nested sibling block. Deeper targets fall back to the
    /// `goto loc+<distance>` spelling, which re-lowers to the same canonical
    /// `skip(distance)` exactly (issue #602).
    fn plan_labels(&mut self, rule_index: usize, actions: &[Action]) {
        let _ = rule_index;
        let mut depth = vec![0usize; actions.len() + 1];
        for (index, action) in actions.iter().enumerate() {
            depth[index + 1] = match action {
                Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. } => depth[index] + 1,
                Action::End => depth[index].saturating_sub(1),
                _ => depth[index],
            };
        }
        for (index, action) in actions.iter().enumerate() {
            let Action::Call { name, args } = action else {
                continue;
            };
            let distance_arg = match (name.as_str(), args.as_slice()) {
                ("skip", [distance]) => distance,
                ("skipIf", [_, distance]) => distance,
                _ => continue,
            };
            let Some(distance) = literal_positive_int(distance_arg) else {
                continue; // dynamic distance → `goto loc+<expr>` needs no label
            };
            let target = (index + 1 + distance).min(actions.len());
            let source_depth = depth[index];
            let target_depth = depth[target];
            if target_depth > source_depth {
                continue;
            }
            // The enclosing frame of the target boundary must stay open (or
            // open further) along the whole jump, otherwise the label lands
            // inside a sibling block the goto cannot see.
            let dips_below_target =
                ((index + 1)..=target).any(|boundary| depth[boundary] < target_depth);
            if dips_below_target {
                continue;
            }
            let label = self.fresh_generated_name("__op_label");
            self.skip_labels.insert(index, label.clone());
            self.labels.entry(target).or_default().push(label);
        }
    }

    /// Deterministic generated identifier that never collides with a declared
    /// program name.
    fn fresh_generated_name(&mut self, prefix: &str) -> String {
        loop {
            self.generated_counter += 1;
            let name = format!("{prefix}_{}", self.generated_counter);
            if !self.declared_names.contains(&name) {
                return name;
            }
        }
    }

    /// Emit any generated `name:` label statements registered for this flat
    /// action boundary.
    fn emit_labels_at(&mut self, position: usize, level: usize) {
        let Some(labels) = self.labels.remove(&position) else {
            return;
        };
        for label in labels {
            self.out.push_str(&Self::indent(level));
            self.out.push_str(&label);
            self.out.push_str(":\n");
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
            self.emit_labels_at(*position, level);
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
                    self.loop_depth += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    self.loop_depth -= 1;
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
                        self.loop_depth += 1;
                        self.emit_action_block(rule_index, actions, position, level + 1);
                        self.loop_depth -= 1;
                        self.require_end(rule_index, actions, position, "for", span);
                        continue;
                    };
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("for ");
                    self.out.push_str(&declaration.name);
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
                    self.loop_depth += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    self.loop_depth -= 1;
                    self.require_end(rule_index, actions, position, "for", span);
                }
                Action::ForPlayerVariable {
                    player,
                    variable,
                    start,
                    stop,
                    step,
                } => {
                    let action_index = *position;
                    let span = self.program.action_span(rule_index, action_index);
                    if !self
                        .program
                        .player_variables
                        .iter()
                        .any(|declaration| declaration.name == *variable)
                    {
                        self.issue(
                            "unsupported-dangling",
                            format!("unknown loop variable '{variable}'"),
                            span,
                        );
                        *position += 1;
                        self.loop_depth += 1;
                        self.emit_action_block(rule_index, actions, position, level + 1);
                        self.loop_depth -= 1;
                        self.require_end(rule_index, actions, position, "for", span);
                        continue;
                    }
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("for ");
                    self.emit_player_variable_receiver(
                        player,
                        self.program
                            .action_argument_span(rule_index, action_index, 0),
                    );
                    self.out.push('.');
                    self.out.push_str(variable);
                    self.out.push_str(" in range(");
                    for (arg_index, value) in [start, stop, step].into_iter().enumerate() {
                        if arg_index > 0 {
                            self.out.push_str(", ");
                        }
                        self.emit_value(
                            value,
                            self.program.action_argument_span(
                                rule_index,
                                action_index,
                                arg_index + 2,
                            ),
                        );
                    }
                    self.out.push_str("):\n");
                    *position += 1;
                    self.loop_depth += 1;
                    self.emit_action_block(rule_index, actions, position, level + 1);
                    self.loop_depth -= 1;
                    self.require_end(rule_index, actions, position, "for", span);
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
            // A self-referential Set (`x = x + v`) re-lowers to a Modify
            // action; the comparator accepts the Set↔Modify difference, so
            // the source-level Set form is emitted directly (issue #602).
            Action::SetGlobalVariable { variable, value } => {
                self.out.push_str(&indent);
                self.out.push_str(variable);
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
                self.emit_modify(
                    level,
                    variable,
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
                let player_span = self
                    .program
                    .action_argument_span(rule_index, action_index, 0);
                let target = self.player_variable_target(player, variable, player_span);
                self.out.push_str(&indent);
                self.out.push_str(&target);
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
                let player_span = self
                    .program
                    .action_argument_span(rule_index, action_index, 0);
                let target = self.player_variable_target(player, variable, player_span);
                self.emit_modify(
                    level,
                    &target,
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
                self.out.push_str(subroutine);
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
                self.emit_call_action(rule_index, action_index, name, args, &indent, span);
            }
        }
    }

    fn is_event_player(&self, value: &Value) -> bool {
        matches!(value, Value::EventPlayer)
    }

    /// Emit `eventPlayer` or a parenthesized arbitrary player expression as a
    /// member receiver (issue #602).
    fn emit_player_variable_receiver(&mut self, player: &Value, span: Option<Span>) {
        if self.is_event_player(player) {
            self.out.push_str("eventPlayer");
        } else {
            self.out.push('(');
            self.emit_value(player, span);
            self.out.push(')');
        }
    }

    /// The `(<player>).<variable>` assignment target for Set/Modify/indexed
    /// player-variable forms.
    fn player_variable_target(
        &mut self,
        player: &Value,
        variable: &str,
        span: Option<Span>,
    ) -> String {
        let receiver = self.render_value_as(player, span, |this, player, span| {
            this.emit_player_variable_receiver(player, span);
        });
        format!("{receiver}.{variable}")
    }

    /// Render a value through `emit_value` into a fresh string so it can be
    /// composed into a target spelling.
    fn render_value_as(
        &mut self,
        value: &Value,
        span: Option<Span>,
        emit: impl FnOnce(&mut Self, &Value, Option<Span>),
    ) -> String {
        let saved = std::mem::take(&mut self.out);
        emit(self, value, span);
        std::mem::replace(&mut self.out, saved)
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
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(".remove(");
                self.emit_value(value, span);
                self.out.push_str(")\n");
            }
            ModifyOp::RemoveFromArrayByIndex => {
                self.out.push_str(&indent);
                self.out.push_str("del ");
                self.out.push_str(name);
                self.out.push('[');
                self.emit_value(value, span);
                self.out.push_str("]\n");
            }
            ModifyOp::Min | ModifyOp::Max => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(match op {
                    ModifyOp::Min => " min= ",
                    ModifyOp::Max => " max= ",
                    _ => unreachable!(),
                });
                self.emit_value(value, span);
                self.out.push('\n');
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
        rule_index: usize,
        action_index: usize,
        name: &str,
        args: &[Value],
        indent: &str,
        span: Option<Span>,
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
        // Syntax-produced actions lower through canonical calls; emit the
        // source form that re-lowers to the same action (issue #602).
        if self.emit_syntax_action(rule_index, action_index, name, args, indent, span) {
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
        if !member && args.is_empty() && self.subroutine_names.contains(name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action '{name}' with no arguments is ambiguous with a subroutine \
                     of the same name on the OPY surface"
                ),
                span,
            );
            return;
        }
        let argument_spans: Vec<Option<Span>> = (0..args.len())
            .map(|index| {
                self.program
                    .action_argument_span(rule_index, action_index, index)
            })
            .collect();
        self.out.push_str(indent);
        self.emit_manifest_call(entry, args, member, span, &argument_spans);
        self.out.push('\n');
    }

    /// Syntax-produced action calls → the OPY statement form that re-lowers
    /// to the same canonical action (issue #602). Returns true when handled.
    fn emit_syntax_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        name: &str,
        args: &[Value],
        indent: &str,
        span: Option<Span>,
    ) -> bool {
        let arg_span = |i: usize| {
            self.program
                .action_argument_span(rule_index, action_index, i)
        };
        match name {
            // startRule(sub, behavior) → `async(<subroutine>, <behavior>)`
            "startRule" => {
                let [subroutine, behavior] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "startRule requires subroutine and behavior arguments",
                        span,
                    );
                    return true;
                };
                let Value::Subroutine(sub_name) = subroutine else {
                    self.issue(
                        "unsupported-action-call",
                        "startRule subroutine argument is not a subroutine value",
                        arg_span(0).or(span),
                    );
                    return true;
                };
                if !self.subroutine_names.contains(sub_name.as_str()) {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown subroutine '{sub_name}'"),
                        arg_span(0).or(span),
                    );
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("async(");
                self.out.push_str(sub_name);
                self.out.push_str(", ");
                self.emit_value(behavior, arg_span(1));
                self.out.push_str(")\n");
                true
            }
            // skip(n) → `goto <generated-label>` (label planned at the target
            // boundary); dynamic distance → `goto loc+<expr>`
            "skip" => {
                let [distance] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "skip requires exactly one distance argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                if let Some(label) = self.skip_labels.get(&action_index) {
                    self.out.push_str("goto ");
                    self.out.push_str(&label.clone());
                    self.out.push('\n');
                } else {
                    self.out.push_str("goto loc+");
                    self.emit_value(distance, arg_span(0));
                    self.out.push('\n');
                }
                true
            }
            // skipIf(cond, n) → `if <cond>: goto <generated-label>`
            "skipIf" => {
                let [condition, distance] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "skipIf requires condition and distance arguments",
                        span,
                    );
                    return true;
                };
                if let Some(label) = self.skip_labels.get(&action_index).cloned() {
                    self.out.push_str(indent);
                    self.out.push_str("if ");
                    self.emit_value(condition, arg_span(0));
                    self.out.push_str(":\n");
                    self.out.push_str(indent);
                    self.out.push_str("    goto ");
                    self.out.push_str(&label);
                    self.out.push('\n');
                } else {
                    self.out.push_str(indent);
                    self.out.push_str("if ");
                    self.emit_value(condition, arg_span(0));
                    self.out.push_str(":\n");
                    self.out.push_str(indent);
                    self.out.push_str("    goto loc+");
                    self.emit_value(distance, arg_span(1));
                    self.out.push('\n');
                }
                true
            }
            // abortIf*/loopIf* → single-statement if forms
            "abort" => {
                if !args.is_empty() {
                    self.issue("unsupported-action-call", "abort takes no arguments", span);
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("return\n");
                true
            }
            "abortIf" => {
                let [condition] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "abortIf requires exactly one condition argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                self.emit_value(condition, arg_span(0));
                self.out.push_str(":\n");
                self.out.push_str(indent);
                self.out.push_str("    return\n");
                true
            }
            "loop" => {
                if !args.is_empty() {
                    self.issue("unsupported-action-call", "loop takes no arguments", span);
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("loop()\n");
                true
            }
            "loopIf" => {
                let [condition] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "loopIf requires exactly one condition argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                self.emit_value(condition, arg_span(0));
                self.out.push_str(":\n");
                self.out.push_str(indent);
                self.out.push_str("    loop()\n");
                true
            }
            // `break` re-lowers to the same canonical call inside a loop body;
            // outside one the OPY surface rejects it, so it stays refused.
            "break" => {
                if self.loop_depth == 0 {
                    return false;
                }
                self.out.push_str(indent);
                self.out.push_str("break\n");
                true
            }
            "__abortIfConditionIsTrue__"
            | "__abortIfConditionIsFalse__"
            | "loopIfConditionIsTrue"
            | "__loopIfConditionIsFalse__" => {
                if !args.is_empty() {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} takes no arguments"),
                        span,
                    );
                    return true;
                }
                let (positive, is_loop) = match name {
                    "__abortIfConditionIsTrue__" => (true, false),
                    "__abortIfConditionIsFalse__" => (false, false),
                    "loopIfConditionIsTrue" => (true, true),
                    _ => (false, true),
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                if !positive {
                    self.out.push_str("not ");
                }
                self.out.push_str("ruleCondition():\n");
                self.out.push_str(indent);
                self.out.push_str(if is_loop {
                    "    loop()\n"
                } else {
                    "    return\n"
                });
                true
            }
            // Indexed assignment/modification → `target[i] = v` / `target[i] op= v`
            "setGlobalVariableAtIndex" | "setPlayerVariableAtIndex" => {
                let [variable, index, value] = args else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} requires variable, index and value arguments"),
                        span,
                    );
                    return true;
                };
                let Some(target) = self.indexed_target(variable, arg_span(0)) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} variable argument is not a variable value"),
                        arg_span(0).or(span),
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str(&target);
                self.out.push('[');
                self.emit_value(index, arg_span(1));
                self.out.push_str("] = ");
                self.emit_value(value, arg_span(2));
                self.out.push('\n');
                true
            }
            "modifyGlobalVariableAtIndex" | "modifyPlayerVariableAtIndex" => {
                let [variable, index, op, value] = args else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} requires variable, index, operation and value arguments"),
                        span,
                    );
                    return true;
                };
                let Some(target) = self.indexed_target(variable, arg_span(0)) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} variable argument is not a variable value"),
                        arg_span(0).or(span),
                    );
                    return true;
                };
                self.out.push_str(indent);
                // Array-mutation ops on an indexed receiver keep their member
                // form: appendToArray → `a[i].append(v)`, removeFromArray →
                // `a[i].remove(v)`, removeFromArrayByIndex → `del a[i][j]`.
                if let Value::Call {
                    name: op_name,
                    args: op_args,
                } = op
                {
                    if matches!(
                        op_name.as_str(),
                        "removeFromArray" | "removeFromArrayByIndex" | "appendToArray"
                    ) && op_args.is_empty()
                    {
                        match op_name.as_str() {
                            "appendToArray" | "removeFromArray" => {
                                self.out.push_str(&target);
                                self.out.push('[');
                                self.emit_value(index, arg_span(1));
                                self.out.push_str("].");
                                self.out.push_str(if op_name == "appendToArray" {
                                    "append("
                                } else {
                                    "remove("
                                });
                                self.emit_value(value, arg_span(3));
                                self.out.push_str(")\n");
                            }
                            _ => {
                                self.out.push_str("del ");
                                self.out.push_str(&target);
                                self.out.push('[');
                                self.emit_value(index, arg_span(1));
                                self.out.push(']');
                                self.out.push('[');
                                self.emit_value(value, arg_span(3));
                                self.out.push(']');
                                self.out.push('\n');
                            }
                        }
                        return true;
                    }
                }
                let Some(operator) = indexed_modify_operator(op) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} operation argument has no compound-assignment form"),
                        arg_span(2).or(span),
                    );
                    return true;
                };
                self.out.push_str(&target);
                self.out.push('[');
                self.emit_value(index, arg_span(1));
                self.out.push_str("] ");
                self.out.push_str(operator);
                self.out.push(' ');
                self.emit_value(value, arg_span(3));
                self.out.push('\n');
                true
            }
            _ => false,
        }
    }

    /// `(player).var` or `var` indexed-assignment target for `set*/modify*AtIndex`.
    fn indexed_target(&mut self, variable: &Value, span: Option<Span>) -> Option<String> {
        match variable {
            Value::GlobalVariable(name) => Some(name.clone()),
            Value::PlayerVariable { player, variable } => {
                let receiver = self.render_value_as(player, span, |this, player, span| {
                    this.emit_player_variable_receiver(player, span);
                });
                Some(format!("{receiver}.{variable}"))
            }
            _ => None,
        }
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
                    // Negative finite numbers emit as unary negation over the
                    // positive literal (issue #603).
                    self.out.push('-');
                    self.out
                        .push_str(&workshop_rs::format::format_number(-*value));
                } else {
                    self.out
                        .push_str(&workshop_rs::format::format_number(*value));
                }
            }
            Value::String(value) => self.emit_string_literal(value),
            Value::LocalizedString(value) => {
                self.emit_localized_literal(value, span);
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
                self.out.push_str(variable);
            }
            Value::PlayerVariable { player, variable } => {
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
                self.emit_player_variable_receiver(player, span);
                self.out.push('.');
                self.out.push_str(variable);
            }
            Value::Subroutine(name) => {
                // Bare subroutine names are only accepted where the OPY
                // grammar resolves them (async args, call statements);
                // anywhere else stays a structured refusal (issue #603).
                self.issue(
                    "unsupported-subroutine-value",
                    format!(
                        "subroutine value '{name}' is not representable in this \
                         value position on the OPY surface"
                    ),
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

    /// `Value::LocalizedString` → `l"<en-US spelling>"` (issue #603). The
    /// `l` literal resolves against the catalog's en-US strings.
    fn emit_localized_literal(&mut self, id: &str, span: Option<Span>) {
        let Some(spelling) = self
            .catalog
            .localized_string_spelling(&Locale::new("en-US"), id)
        else {
            self.issue(
                "unsupported-localized-string",
                format!("localized string id '{id}' has no catalog spelling"),
                span,
            );
            return;
        };
        self.out.push('l');
        self.emit_string_literal(spelling);
    }

    /// Emit `body` wrapped as `lambda <element>[, <index>]: <body>` with
    /// deterministic binder names pushed for `currentArrayElement`/`Index`.
    fn emit_lambda_body(
        &mut self,
        member_call: &str,
        receiver: &Value,
        body: &Value,
        span: Option<Span>,
    ) {
        let uses_index = uses_call(body, "currentArrayIndex");
        let element = self.fresh_generated_name("__elem");
        let index = uses_index.then(|| self.fresh_generated_name("__idx"));
        self.out.push('(');
        self.emit_value(receiver, span);
        self.out.push_str(").");
        self.out.push_str(member_call);
        self.out.push_str("(lambda ");
        self.out.push_str(&element);
        if let Some(index) = &index {
            self.out.push_str(", ");
            self.out.push_str(index);
        }
        self.out.push_str(": ");
        self.lambda_binders.push((element, index));
        self.emit_value(body, span);
        self.lambda_binders.pop();
        self.out.push(')');
    }

    /// Emit `sorted(...)`/generic `all`/`any` forms which take the lambda as a
    /// plain argument rather than on a member call.
    fn emit_sorted(&mut self, array: &Value, key: &Value, span: Option<Span>) {
        if matches!(key, Value::Call { name, args } if name == "currentArrayElement" && args.is_empty())
        {
            self.out.push_str("sorted(");
            self.emit_value(array, span);
            self.out.push(')');
            return;
        }
        let uses_index = uses_call(key, "currentArrayIndex");
        let element = self.fresh_generated_name("__elem");
        let index = uses_index.then(|| self.fresh_generated_name("__idx"));
        self.out.push_str("sorted(");
        self.emit_value(array, span);
        self.out.push_str(", lambda ");
        self.out.push_str(&element);
        if let Some(index) = &index {
            self.out.push_str(", ");
            self.out.push_str(index);
        }
        self.out.push_str(": ");
        self.lambda_binders.push((element, index));
        self.emit_value(key, span);
        self.lambda_binders.pop();
        self.out.push(')');
    }

    /// Syntax-produced value calls → the OPY source form that re-lowers to
    /// the same canonical call (issue #603). Returns true when handled.
    fn emit_syntax_value(&mut self, name: &str, args: &[Value], span: Option<Span>) -> bool {
        // Canonical arithmetic identities (`Add`, `Subtract`, …) emit as
        // source-level binary operators.
        if args.len() == 2
            && let Some((_, operator)) = ARITHMETIC_OPS.iter().find(|(n, _)| *n == name)
        {
            self.out.push('(');
            self.emit_value(&args[0], span);
            self.out.push(' ');
            self.out.push_str(operator);
            self.out.push(' ');
            self.emit_value(&args[1], span);
            self.out.push(')');
            return true;
        }
        match name {
            // valueInArray(a, i) → `a[i]`; a literal-zero index is
            // unreachable: every `a[i]` spelling folding to 0 normalizes to
            // `firstOf`, so it stays a structured refusal.
            "valueInArray" => {
                let [array, index] = args else {
                    return false;
                };
                if matches!(index, Value::Number(n) if *n == 0.0) {
                    self.issue(
                        "unsupported-value-call",
                        "valueInArray with a literal zero index re-lowers to firstOf \
                         and cannot round-trip",
                        span,
                    );
                    return true;
                }
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(")[");
                self.emit_value(index, span);
                self.out.push(']');
                true
            }
            "firstOf" => {
                let [array] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(")[0]");
                true
            }
            "emptyArray" if args.is_empty() => {
                self.out.push_str("[]");
                true
            }
            // slice(a, start, count) → `a.slice(start[, count])`; the count
            // parameter defaults to 999999999999 on the source side.
            "slice" => {
                let [array, start, count] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(").slice(");
                self.emit_value(start, span);
                if !matches!(count, Value::Number(n) if *n == 999_999_999_999.0) {
                    self.out.push_str(", ");
                    self.emit_value(count, span);
                }
                self.out.push(')');
                true
            }
            // ifThenElse(c, t, e) → `(t if c else e)`
            "ifThenElse" => {
                let [condition, then_value, else_value] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(then_value, span);
                self.out.push_str(" if ");
                self.emit_value(condition, span);
                self.out.push_str(" else ");
                self.emit_value(else_value, span);
                self.out.push(')');
                true
            }
            // roundToInteger(v, Rounding.X) → `ceil(v)`/`floor(v)`/`round(v)`
            "roundToInteger" => {
                let [value, rounding] = args else {
                    return false;
                };
                let spelling = match rounding {
                    Value::Enum { value_type, value } if value_type == "Rounding" => {
                        match value.as_str() {
                            "UP" => "ceil",
                            "DOWN" => "floor",
                            "NEAREST" => "round",
                            _ => return false,
                        }
                    }
                    _ => return false,
                };
                self.out.push_str(spelling);
                self.out.push('(');
                self.emit_value(value, span);
                self.out.push(')');
                true
            }
            // Array callbacks carry their binder through lambda syntax.
            "sortedArray" => {
                let [array, key] = args else {
                    return false;
                };
                self.emit_sorted(array, key, span);
                true
            }
            "mappedArray" => {
                let [array, body] = args else {
                    return false;
                };
                self.emit_lambda_body("map", array, body, span);
                true
            }
            "filteredArray" => {
                let [array, condition] = args else {
                    return false;
                };
                self.emit_lambda_body("filter", array, condition, span);
                true
            }
            "isTrueForAll" | "isTrueForAny" => {
                let [array, condition] = args else {
                    return false;
                };
                let member = if name == "isTrueForAll" { "all" } else { "any" };
                if matches!(condition, Value::Call { name, args } if name == "currentArrayElement" && args.is_empty())
                {
                    self.out.push_str(member);
                    self.out.push('(');
                    self.emit_value(array, span);
                    self.out.push(')');
                } else {
                    self.emit_lambda_body(member, array, condition, span);
                }
                true
            }
            // arrayContains(a, x) → `(x in a)`
            "arrayContains" => {
                let [array, needle] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(needle, span);
                self.out.push_str(" in ");
                self.emit_value(array, span);
                self.out.push(')');
                true
            }
            // `string(localized, fmt...)` → `l"...".format(...)` / bare `l"..."`
            "string" => {
                let [localized, ..] = args else {
                    return false;
                };
                let Value::LocalizedString(id) = localized else {
                    return false;
                };
                self.emit_localized_literal(id, span);
                if args.len() > 1 {
                    self.out.push_str(".format(");
                    for (index, arg) in args.iter().skip(1).enumerate() {
                        if index > 0 {
                            self.out.push_str(", ");
                        }
                        self.emit_value(arg, span);
                    }
                    self.out.push(')');
                }
                true
            }
            // `memberAccess(recv, "m")` → `(recv).m`; vector components are
            // the dedicated `.x`/`.y`/`.z` member spellings.
            "memberAccess" => {
                let [receiver, member] = args else {
                    return false;
                };
                let Value::String(member) = member else {
                    return false;
                };
                if !is_simple_ident(member) {
                    self.issue(
                        "unsupported-value-call",
                        format!("member name '{member}' is not a plain OPY identifier"),
                        span,
                    );
                    return true;
                }
                self.out.push('(');
                self.emit_value(receiver, span);
                self.out.push_str(").");
                self.out.push_str(member);
                true
            }
            "__xComponentOf__" | "__yComponentOf__" | "__zComponentOf__" => {
                let [receiver] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(receiver, span);
                self.out.push_str(").");
                self.out.push_str(match name {
                    "__xComponentOf__" => "x",
                    "__yComponentOf__" => "y",
                    _ => "z",
                });
                true
            }
            // Lambda binders resolve to the deterministic names the enclosing
            // generated lambda declared; outside one they stay refused.
            "currentArrayElement" if args.is_empty() => {
                match self.lambda_binders.last() {
                    Some((element, _)) => {
                        self.out.push_str(&element.clone());
                    }
                    None => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayElement outside an array callback has no OPY binder",
                            span,
                        );
                    }
                }
                true
            }
            "currentArrayIndex" if args.is_empty() => {
                match self.lambda_binders.last() {
                    Some((_, Some(index))) => {
                        self.out.push_str(&index.clone());
                    }
                    Some(_) => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayIndex used but the callback has no index binder",
                            span,
                        );
                    }
                    None => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayIndex outside an array callback has no OPY binder",
                            span,
                        );
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn emit_value_call(&mut self, name: &str, args: &[Value], span: Option<Span>) {
        if name == "localPlayer" && args.is_empty() {
            self.out.push_str(name);
            return;
        }
        // Syntax-produced value calls → the source form that re-lowers to the
        // same canonical call (issue #603).
        if self.emit_syntax_value(name, args, span) {
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

/// Whether a canonical value tree contains a call with the given name
/// (used to size generated lambda binders).
fn uses_call(value: &Value, name: &str) -> bool {
    match value {
        Value::Call {
            name: call_name,
            args,
        } => call_name == name || args.iter().any(|arg| uses_call(arg, name)),
        Value::Array(elements) => elements.iter().any(|element| uses_call(element, name)),
        Value::Vector { x, y, z } => uses_call(x, name) || uses_call(y, name) || uses_call(z, name),
        _ => false,
    }
}

/// The compound-assignment spelling for a `modify*AtIndex` operation argument
/// (a zero-argument canonical call), or None when it has no assignment form.
fn indexed_modify_operator(op: &Value) -> Option<&'static str> {
    let Value::Call { name, args } = op else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    INDEXED_MODIFY_OPS
        .iter()
        .find(|(op_name, _)| *op_name == name)
        .map(|(_, operator)| *operator)
}

/// A positive whole-number `Value::Number` (skip/skipIf distances).
fn literal_positive_int(value: &Value) -> Option<usize> {
    let Value::Number(number) = value else {
        return None;
    };
    (number.is_finite() && number.fract() == 0.0 && *number >= 0.0).then_some(*number as usize)
}

/// Whether `name` is safe to emit as a bare member/identifier spelling.
fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
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
    fn smoke_syntax_produced_actions() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.subroutine(Subroutine::with_index("sub", 0));
        program.rule(
            Rule::new("Subroutine sub", Event::Subroutine("sub".into())).action(
                Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(0.0),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::EachPlayer)
                // for <player>.p in range(0, 5, 1)
                .action(Action::ForPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    start: Value::number(0.0),
                    stop: Value::number(5.0),
                    step: Value::number(1.0),
                })
                .action(Action::SetPlayerVariable {
                    player: Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    ),
                    variable: "p".into(),
                    value: Value::number(-3.0),
                })
                .action(Action::End)
                // self-referential set
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("add", [Value::global_variable("g"), Value::number(2.0)]),
                })
                // min/max/remove
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Min,
                    value: Value::number(9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Max,
                    value: Value::number(-9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByValue,
                    value: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByIndex,
                    value: Value::number(0.0),
                })
                // indexed assign + modify
                .action(Action::call(
                    "setGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::number(7.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("add", Vec::<Value>::new()),
                        Value::number(3.0),
                    ],
                ))
                // async
                .action(Action::call(
                    "startRule",
                    [
                        Value::Subroutine("sub".into()),
                        Value::Enum {
                            value_type: "StartRuleBehavior".into(),
                            value: "RESTART".into(),
                        },
                    ],
                ))
                // skipIf crossing an if block: skipIf(cond, 5) If..End x=1
                .action(Action::call(
                    "skipIf",
                    [
                        Value::call("==", [Value::global_variable("g"), Value::number(1.0)]),
                        Value::number(5.0),
                    ],
                ))
                .action(Action::If {
                    condition: Value::Bool(true),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::Else)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(5.0),
                })
                .action(Action::End)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(2.0),
                })
                // abortIf + loopIf
                .action(Action::call(
                    "abortIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(3.0)],
                    )],
                ))
                .action(Action::call(
                    "loopIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(4.0)],
                    )],
                )),
        );

        let source = reconstruct(&program).expect("program should reconstruct");
        assert!(source.contains("for eventPlayer.p in range(0, 5, 1):"));
        assert!(source.contains("g min= 9"));
        assert!(source.contains("g max= -9"));
        assert!(source.contains("g.remove(1)"));
        assert!(source.contains("del g[0]"));
        assert!(source.contains("g[1] = 7"));
        assert!(source.contains("g[1] += 3"));
        assert!(source.contains("async(sub, StartRuleBehavior.RESTART)"));
        assert!(source.contains("__op_label_1"));
        assert_behaviorally_equivalent(&program, &source);
    }

    /// A single-action program `rule "main": @Event global; <action>` for
    /// focused round-trip assertions.
    fn one_action_program(action: Action) -> Program {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.rule(Rule::new("main", Event::Global).action(action));
        program
    }

    fn one_value_program(value: Value) -> Program {
        one_action_program(Action::SetGlobalVariable {
            variable: "g".into(),
            value,
        })
    }

    fn assert_error_code(program: &Program, code: &str) {
        let error = reconstruct(program).expect_err("program should be refused");
        assert!(
            error.issues.iter().any(|issue| issue.code == code),
            "expected '{code}' in {error:?}"
        );
    }

    // ---- issue #602: syntax-produced actions ------------------------------

    #[test]
    fn nested_player_variable_for_loops_round_trip() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("p", 0));
        program.player_variable(Variable::with_index("q", 1));
        program.rule(
            Rule::new("main", Event::EachPlayer)
                .action(Action::ForPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    start: Value::number(0.0),
                    stop: Value::number(3.0),
                    step: Value::number(1.0),
                })
                .action(Action::ForPlayerVariable {
                    player: Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    ),
                    variable: "q".into(),
                    start: Value::number(1.0),
                    stop: Value::number(4.0),
                    step: Value::number(2.0),
                })
                .action(Action::ModifyPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End)
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("nested for should reconstruct");
        assert!(
            source.contains("for eventPlayer.p in range(0, 3, 1):"),
            "{source}"
        );
        assert!(
            source.contains("for ((getPlayers(Team.ALL))[1]).q in range(1, 4, 2):"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn player_variable_reads_and_writes_on_arbitrary_players() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("p", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetPlayerVariable {
                player: Value::call(
                    "getClosestPlayer",
                    [
                        Value::call(
                            "vector",
                            [Value::number(0.0), Value::number(0.0), Value::number(0.0)],
                        ),
                        Value::Enum {
                            value_type: "Team".into(),
                            value: "ALL".into(),
                        },
                    ],
                ),
                variable: "p".into(),
                value: Value::PlayerVariable {
                    player: Box::new(Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    )),
                    variable: "p".into(),
                },
            }),
        );

        let source = reconstruct(&program).expect("player var access should reconstruct");
        assert!(
            source.contains(
                "(getClosestPlayer(vect(0, 0, 0), Team.ALL)).p = ((getPlayers(Team.ALL))[1]).p"
            ),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn self_referential_set_and_modify_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("add", [Value::global_variable("g"), Value::number(2.0)]),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::AppendToArray,
                    value: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByValue,
                    value: Value::number(2.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByIndex,
                    value: Value::number(0.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Min,
                    value: Value::number(9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Max,
                    value: Value::number(-9.0),
                }),
        );

        let source = reconstruct(&program).expect("modify forms should reconstruct");
        assert!(source.contains("g = (g + 2)"), "{source}");
        assert!(source.contains("g.append(1)"), "{source}");
        assert!(source.contains("g.remove(2)"), "{source}");
        assert!(source.contains("del g[0]"), "{source}");
        assert!(source.contains("g min= 9"), "{source}");
        assert!(source.contains("g max= -9"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn indexed_assignment_and_compound_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call(
                    "setGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::number(7.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("multiply", Vec::<Value>::new()),
                        Value::number(3.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(0.0),
                        Value::call("min", Vec::<Value>::new()),
                        Value::number(5.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(2.0),
                        Value::call("appendToArray", Vec::<Value>::new()),
                        Value::number(8.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(2.0),
                        Value::call("removeFromArray", Vec::<Value>::new()),
                        Value::number(8.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("removeFromArrayByIndex", Vec::<Value>::new()),
                        Value::number(0.0),
                    ],
                ))
                .action(Action::call(
                    "setPlayerVariableAtIndex",
                    [
                        Value::PlayerVariable {
                            player: Box::new(Value::EventPlayer),
                            variable: "p".into(),
                        },
                        Value::number(1.0),
                        Value::number(4.0),
                    ],
                )),
        );

        let source = reconstruct(&program).expect("indexed forms should reconstruct");
        assert!(source.contains("g[1] = 7"), "{source}");
        assert!(source.contains("g[1] *= 3"), "{source}");
        assert!(source.contains("g[0] min= 5"), "{source}");
        assert!(source.contains("g[2].append(8)"), "{source}");
        assert!(source.contains("g[2].remove(8)"), "{source}");
        assert!(source.contains("del g[1][0]"), "{source}");
        assert!(source.contains("eventPlayer.p[1] = 4"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn skip_and_skipif_emit_deterministic_labels() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(1.0)]))
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::call(
                    "skipIf",
                    [
                        Value::call("==", [Value::global_variable("g"), Value::number(2.0)]),
                        Value::number(1.0),
                    ],
                ))
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(3.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(5.0),
                }),
        );

        let source = reconstruct(&program).expect("skips should reconstruct");
        assert!(source.contains("goto __op_label_1\n"), "{source}");
        assert!(source.contains("goto __op_label_2\n"), "{source}");
        assert!(source.contains("__op_label_1:"), "{source}");
        assert!(source.contains("__op_label_2:"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn generated_labels_avoid_declared_names() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("__op_label_1", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(1.0)]))
                .action(Action::SetGlobalVariable {
                    variable: "__op_label_1".into(),
                    value: Value::number(1.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "__op_label_1".into(),
                    value: Value::number(2.0),
                }),
        );

        let source = reconstruct(&program).expect("labels should dodge declared names");
        assert!(source.contains("goto __op_label_2\n"), "{source}");
        assert!(source.contains("__op_label_2:"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn skip_into_a_nested_block_uses_relative_goto() {
        // A skip whose target boundary sits inside a nested sibling block
        // cannot name a label the goto resolution can see; `goto loc+N`
        // re-lowers to the identical `skip(N)` (issue #602).
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(2.0)]))
                .action(Action::If {
                    condition: Value::Bool(true),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("skip should reconstruct");
        assert!(source.contains("goto loc+2\n"), "{source}");
        assert!(!source.contains("__op_label"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn terminal_abort_and_loop_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("abort", Vec::<Value>::new()))
                .action(Action::call(
                    "abortIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(1.0)],
                    )],
                ))
                .action(Action::call("loop", Vec::<Value>::new()))
                .action(Action::call(
                    "loopIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(2.0)],
                    )],
                ))
                .action(Action::call(
                    "__abortIfConditionIsTrue__",
                    Vec::<Value>::new(),
                ))
                .action(Action::call("loopIfConditionIsTrue", Vec::<Value>::new()))
                // One observable action keeps the rule alive on reparse.
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(9.0),
                }),
        );

        let source = reconstruct(&program).expect("terminal forms should reconstruct");
        assert!(source.contains("return\n"), "{source}");
        assert!(source.contains("    loop()\n"), "{source}");
        assert!(source.contains("if (g == 1):"), "{source}");
        assert!(source.contains("if (g == 2):"), "{source}");
        assert!(source.contains("if ruleCondition():"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn break_inside_a_while_loop_round_trips() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::While {
                    condition: Value::call("<", [Value::global_variable("g"), Value::number(10.0)]),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::call("break", Vec::<Value>::new()))
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("break should reconstruct");
        assert!(source.contains("while (g < 10):"), "{source}");
        assert!(source.contains("    break\n"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn member_assignment_stays_refused() {
        let program = one_action_program(Action::AssignMember {
            target: Value::call(
                "memberAccess",
                [Value::global_variable("g"), Value::string("health")],
            ),
            op: Some(ModifyOp::Add),
            value: Value::number(1.0),
        });
        assert_error_code(&program, "unsupported-member-assignment");
    }

    // ---- issue #603: syntax-produced values -------------------------------

    #[test]
    fn syntax_produced_values_round_trip() {
        let array = Value::call(
            "allPlayers",
            [Value::Enum {
                value_type: "Team".into(),
                value: "ALL".into(),
            }],
        );
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                // (a)[i]
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("valueInArray", [array.clone(), Value::number(1.0)]),
                })
                // sorted(a), sorted(a, key lambda)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "sortedArray",
                        [
                            array.clone(),
                            Value::call("currentArrayElement", Vec::<Value>::new()),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "sortedArray",
                        [
                            array.clone(),
                            Value::call(
                                "getSpeed",
                                [Value::call("currentArrayElement", Vec::<Value>::new())],
                            ),
                        ],
                    ),
                })
                // map + filter with element and index binders
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "mappedArray",
                        [
                            array.clone(),
                            Value::call(
                                "+",
                                [
                                    Value::call("currentArrayElement", Vec::<Value>::new()),
                                    Value::call("currentArrayIndex", Vec::<Value>::new()),
                                ],
                            ),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "filteredArray",
                        [
                            array.clone(),
                            Value::call(
                                ">",
                                [
                                    Value::call("currentArrayIndex", Vec::<Value>::new()),
                                    Value::number(0.0),
                                ],
                            ),
                        ],
                    ),
                })
                // x in a
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("arrayContains", [array.clone(), Value::EventPlayer]),
                })
                // a[0], []
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("firstOf", [array.clone()]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("emptyArray", Vec::<Value>::new()),
                })
                // a.slice / conditional
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "slice",
                        [array.clone(), Value::number(1.0), Value::number(3.0)],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "ifThenElse",
                        [
                            Value::call("==", [Value::global_variable("g"), Value::number(1.0)]),
                            Value::number(1.0),
                            Value::number(2.0),
                        ],
                    ),
                })
                // round/floor/ceil
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "NEAREST".into(),
                            },
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "DOWN".into(),
                            },
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "UP".into(),
                            },
                        ],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("syntax values should reconstruct");
        // `allPlayers(team)` reconstructs through the deterministic sanctioned
        // alias `getPlayers(Team.ALL)` (documented in the reconstruction notes).
        assert!(source.contains("(getPlayers(Team.ALL))[1]"), "{source}");
        assert!(source.contains("sorted(getPlayers(Team.ALL))"), "{source}");
        assert!(
            source.contains("sorted(getPlayers(Team.ALL), lambda __elem"),
            "{source}"
        );
        assert!(source.contains(".map(lambda __elem"), "{source}");
        assert!(source.contains(".filter(lambda __elem"), "{source}");
        assert!(
            source.contains("(eventPlayer in getPlayers(Team.ALL))"),
            "{source}"
        );
        assert!(source.contains("(getPlayers(Team.ALL))[0]"), "{source}");
        assert!(source.contains("g = []"), "{source}");
        assert!(
            source.contains("getPlayers(Team.ALL)).slice(1, 3)"),
            "{source}"
        );
        assert!(source.contains("(1 if (g == 1) else 2)"), "{source}");
        assert!(source.contains("round(g)"), "{source}");
        assert!(source.contains("floor(g)"), "{source}");
        assert!(source.contains("ceil(g)"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn mapped_array_lambda_uses_element_and_index_binders() {
        let program = one_value_program(Value::call(
            "mappedArray",
            [
                Value::global_variable("g"),
                Value::call(
                    "add",
                    [
                        Value::call("currentArrayElement", Vec::<Value>::new()),
                        Value::call("currentArrayIndex", Vec::<Value>::new()),
                    ],
                ),
            ],
        ));

        let source = reconstruct(&program).expect("lambda should reconstruct");
        assert!(
            source.contains("g = (g).map(lambda __elem_1, __idx_2: (__elem_1 + __idx_2))"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn localized_string_literals_round_trip() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("string", [Value::LocalizedString("hello".into())]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "string",
                        [Value::LocalizedString("goodbye".into()), Value::EventPlayer],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("localized strings should reconstruct");
        assert!(source.contains("l\"Hello\""), "{source}");
        assert!(
            source.contains("l\"Goodbye\".format(eventPlayer)"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn negative_numbers_emit_unary_negation() {
        let program = one_value_program(Value::number(-3.5));
        let source = reconstruct(&program).expect("negative number should reconstruct");
        assert!(source.contains("g = -3.5"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn member_access_and_vector_components() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("__xComponentOf__", [Value::global_variable("g")]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "memberAccess",
                        [Value::global_variable("g"), Value::string("health")],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("member access should reconstruct");
        assert!(source.contains("(g).x"), "{source}");
        assert!(source.contains("(g).health"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn nested_lambdas_keep_distinct_binders() {
        let inner = Value::call(
            "filteredArray",
            [
                Value::call("currentArrayElement", Vec::<Value>::new()),
                Value::call(
                    "==",
                    [
                        Value::call("currentArrayIndex", Vec::<Value>::new()),
                        Value::number(0.0),
                    ],
                ),
            ],
        );
        let program = one_value_program(Value::call(
            "mappedArray",
            [Value::global_variable("g"), inner],
        ));

        let source = reconstruct(&program).expect("nested lambdas should reconstruct");
        assert!(source.contains(".map(lambda __elem_"), "{source}");
        assert!(source.contains(".filter(lambda __elem_"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    // ---- refusals -----------------------------------------------------------

    #[test]
    fn refusals_stay_diagnostic() {
        // Non-finite numbers have no OPY spelling.
        assert_error_code(
            &one_value_program(Value::number(f64::NAN)),
            "unsupported-non-finite-number",
        );
        assert_error_code(
            &one_value_program(Value::number(f64::INFINITY)),
            "unsupported-non-finite-number",
        );

        // Enum domains outside the manifest's source surface stay refused.
        assert_error_code(
            &one_value_program(Value::Enum {
                value_type: "NotARealDomain".into(),
                value: "X".into(),
            }),
            "unsupported-enum-domain",
        );

        // Value calls with no OPY form stay refused.
        assert_error_code(
            &one_value_program(Value::call("definitelyNotAValue", [Value::number(1.0)])),
            "unsupported-value-call",
        );

        // `valueInArray` with a literal zero index folds to `firstOf` on
        // reparse, so the direct spelling cannot round-trip.
        assert_error_code(
            &one_value_program(Value::call(
                "valueInArray",
                [Value::global_variable("g"), Value::number(0.0)],
            )),
            "unsupported-value-call",
        );

        // Bare subroutine values outside `async` arguments stay refused.
        assert_error_code(
            &one_value_program(Value::Subroutine("sub".into())),
            "unsupported-subroutine-value",
        );

        // Lambda binders outside an array callback stay refused.
        assert_error_code(
            &one_value_program(Value::call("currentArrayElement", Vec::<Value>::new())),
            "unsupported-value-call",
        );
        assert_error_code(
            &one_value_program(Value::call("currentArrayIndex", Vec::<Value>::new())),
            "unsupported-value-call",
        );

        // Dynamic member names that are not identifiers stay refused.
        assert_error_code(
            &one_value_program(Value::call(
                "memberAccess",
                [Value::global_variable("g"), Value::string("not an ident")],
            )),
            "unsupported-value-call",
        );
    }

    fn assert_behaviorally_equivalent(program: &Program, source: &str) {
        let reparsed = match compile_to_program_checked(source) {
            Ok(program) => program,
            Err(message) => panic!("recompiled failed:\n{source}\n{message}"),
        };
        assert!(
            workshop_rs::compare::equivalent(program, &reparsed),
            "reconstructed source is not behaviorally equivalent:\n{source}\ndifference: {:?}",
            workshop_rs::compare::difference(program, &reparsed),
        );
    }

    fn compile_to_program_checked(source: &str) -> Result<Program, String> {
        let artifact = Compiler::new()
            .map_err(|e| e.to_string())?
            .compile_source_artifact(source, "reconstructed.opy", Path::new("."))
            .map_err(|e| format!("{e:?}"))?;
        let catalog = Catalog::builtin().map_err(|e| e.to_string())?;
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &Locale::new("en-US"))
            .map_err(|e| format!("{e:?}"))
    }
}

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
    /// Subroutine names, for call-vs-subroutine ambiguity checks.
    subroutine_names: std::collections::HashSet<String>,
}

type IndexedRule<'a> = (usize, &'a Rule);

/// The declaration initializers a canonical initializer rule carries:
/// declaration table position → (value, provenance span of the Set's value).
type Initializers = std::collections::HashMap<usize, (Value, Option<Span>)>;

struct RuleLayout<'a> {
    global_init: Option<Initializers>,
    player_init: Option<Initializers>,
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
        Emitter {
            program,
            manifest,
            catalog,
            locale,
            issues: Vec::new(),
            out: String::new(),
            subroutine_names,
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

    /// The canonical rule layout: optional leading initializer rules in the
    /// frontend's synthesized position, then the subroutine-body rules and
    /// the normal rules (each partition keeps program order). Subroutine
    /// rules need no positional constraint — the comparator ignores their
    /// relative order — and their rule names are free (rule names are
    /// display metadata). An initializer-named rule outside the frontend's
    /// canonical shape or position is an ordinary rule at its own position:
    /// initializer rules are never relocated (#597).
    fn classify_rules(&mut self) -> RuleLayout<'a> {
        let rules: Vec<IndexedRule<'a>> = self.program.rules.iter().enumerate().collect();
        let mut index = 0;
        let mut global_init = None;
        let mut player_init = None;
        if let Some(&(rule_index, rule)) = rules.first() {
            if rule.name == "Initialize global variables" {
                if let Some(initializers) = self.canonical_init(rule_index, rule, true) {
                    global_init = Some(initializers);
                    index = 1;
                }
            } else if rule.name == "Initialize player variables" {
                if let Some(initializers) = self.canonical_init(rule_index, rule, false) {
                    player_init = Some(initializers);
                    index = 1;
                }
            }
        }
        if global_init.is_some() {
            if let Some(&(rule_index, rule)) = rules.get(1) {
                if rule.name == "Initialize player variables" {
                    if let Some(initializers) = self.canonical_init(rule_index, rule, false) {
                        player_init = Some(initializers);
                        index = 2;
                    }
                }
            }
        }

        let mut sub_rules = Vec::new();
        let mut normal_rules = Vec::new();
        for (rule_index, rule) in rules.iter().copied().skip(index) {
            match &rule.event {
                Event::Subroutine(_) => {
                    if !rule.disabled && !rule.conditions.is_empty() {
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
                _ => normal_rules.push((rule_index, rule)),
            }
        }

        RuleLayout {
            global_init,
            player_init,
            sub_rules,
            normal_rules,
        }
    }

    /// The declaration initializers a candidate initializer rule carries
    /// when it is exactly the frontend's canonical shape: not disabled, the
    /// canonical event, no conditions, and all-`Set` actions in declaration
    /// order on declared variables (`eventPlayer`-targeted for player
    /// initializers). Any other shape is `None` — the rule reconstructs as
    /// an ordinary rule at its own position.
    fn canonical_init(&self, rule_index: usize, rule: &Rule, global: bool) -> Option<Initializers> {
        if rule.disabled {
            return None;
        }
        let canonical_event = if global {
            matches!(&rule.event, Event::Global)
        } else {
            matches!(&rule.event, Event::EachPlayer)
        };
        if !canonical_event || !rule.conditions.is_empty() {
            return None;
        }
        let variables = if global {
            &self.program.global_variables
        } else {
            &self.program.player_variables
        };
        let mut initializers = Initializers::new();
        let mut previous: Option<usize> = None;
        for (action_index, action) in rule.actions.iter().enumerate() {
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
                _ => return None,
            };
            if player.is_some_and(|player| !self.is_event_player(player)) {
                return None;
            }
            let position = variables
                .iter()
                .position(|declaration| declaration.name == *variable)?;
            if previous.is_some_and(|previous| position <= previous) {
                return None;
            }
            previous = Some(position);
            initializers.insert(
                position,
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
        Some(initializers)
    }

    fn emit_program(&mut self, layout: &RuleLayout) {
        self.check_initializer_slot(layout.global_init.as_ref());

        self.emit_variable_declarations(true, layout.global_init.as_ref());
        self.emit_variable_declarations(false, layout.player_init.as_ref());
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

        let mut defined = std::collections::HashSet::new();
        for (rule_index, rule) in &layout.sub_rules {
            let Event::Subroutine(subroutine_name) = &rule.event else {
                continue;
            };
            if !self.subroutine_names.contains(subroutine_name) {
                self.issue(
                    "unsupported-dangling",
                    format!("unknown subroutine '{subroutine_name}'"),
                    self.program.rule_span(*rule_index),
                );
                continue;
            }
            if !defined.insert(subroutine_name.as_str()) {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "subroutine '{subroutine_name}' has more than one def rule; \
                         the comparator accepts reordering, not duplication"
                    ),
                    self.program.rule_span(*rule_index),
                );
                continue;
            }
            self.out.push_str("def ");
            self.out.push_str(subroutine_name);
            self.out.push_str("():\n");
            if rule.disabled {
                // A disabled subroutine-body rule reconstructs as an inert
                // `def`: a `def` cannot carry `@Disabled`, so the annotation
                // and the whole body are emitted as `#`-commented lines plus
                // a live `pass`. The comments make the disable explicit and
                // keep the body text for fidelity; the behavior comparator
                // treats disabled and no-op bodies the same.
                self.out.push_str("    # @Disabled\n");
                if !rule.conditions.is_empty() {
                    for (condition_index, condition) in rule.conditions.iter().enumerate() {
                        let scratch = std::mem::take(&mut self.out);
                        self.out.push_str("    @Condition ");
                        self.emit_value(
                            &condition.value,
                            self.program.condition_span(*rule_index, condition_index),
                        );
                        self.out.push('\n');
                        self.flush_commented(scratch, 1);
                    }
                }
                self.emit_commented_actions(*rule_index, &rule.actions);
                self.out.push_str("    pass\n");
            } else {
                self.emit_actions(*rule_index, &rule.actions, 1);
                if rule.actions.is_empty() {
                    self.out.push_str("    pass\n");
                }
            }
            self.out.push('\n');
        }

        for (rule_index, rule) in &layout.normal_rules {
            if !rule.disabled && rule.actions.is_empty() {
                continue;
            }
            self.out.push_str("rule ");
            self.emit_string_literal(&rule.name);
            self.out.push_str(":\n");
            self.emit_event_annotation(*rule_index, rule);
            if rule.disabled {
                self.out.push_str("    @Disabled\n");
            }
            for (condition_index, condition) in rule.conditions.iter().enumerate() {
                if condition.disabled {
                    // A disabled condition never executes, so it reconstructs
                    // as a `#`-commented line: the comparator treats disabled
                    // conditions as inert, and the comment keeps the
                    // condition text for fidelity.
                    let scratch = std::mem::take(&mut self.out);
                    self.out.push_str("    @Condition ");
                    self.emit_value(
                        &condition.value,
                        self.program.condition_span(*rule_index, condition_index),
                    );
                    self.out.push('\n');
                    self.flush_commented(scratch, 1);
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

    /// Emit the `@Event` line and its `@Team`/`@Hero`/`@Slot` filters for a
    /// normal rule. `EachPlayer` events carry filters through
    /// `EachPlayerWithFilters`; `Player` events carry their catalog id plus
    /// filters; `Global` has no filters.
    fn emit_event_annotation(&mut self, rule_index: usize, rule: &Rule) {
        let (kind, filters) = match &rule.event {
            Event::Global => ("global", None),
            Event::EachPlayer => ("eachPlayer", None),
            Event::EachPlayerWithFilters { team, target } => ("eachPlayer", Some((team, target))),
            Event::Player { kind, team, target } => (kind.catalog_id(), Some((team, target))),
            Event::Subroutine(_) => {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "rule '{}' has a subroutine event outside the def layout",
                        rule.name
                    ),
                    self.program.rule_span(rule_index),
                );
                return;
            }
        };
        self.out.push_str("    @Event ");
        self.out.push_str(kind);
        self.out.push('\n');
        if let Some((team, target)) = filters {
            self.emit_event_filters(rule_index, rule, team, target);
        }
    }

    /// Emit `@Team`/`@Hero`/`@Slot` filter annotations for an `EachPlayer` or
    /// `Player` event. `All` entries emit nothing; a hero target uses the
    /// hero member's OPY spelling (`@Hero` is the only enum-member filter the
    /// surface accepts, since slot targets use `@Slot` and the two cannot be
    /// combined).
    fn emit_event_filters(
        &mut self,
        rule_index: usize,
        rule: &Rule,
        team: &EventTeam,
        target: &EventTarget,
    ) {
        match team {
            EventTeam::All => {}
            EventTeam::Team1 => self.out.push_str("    @Team 1\n"),
            EventTeam::Team2 => self.out.push_str("    @Team 2\n"),
        }
        match target {
            EventTarget::All => {}
            EventTarget::Slot(slot) => {
                self.out.push_str(&format!("    @Slot {slot}\n"));
            }
            EventTarget::Hero(member) => {
                let Some((_, spelling)) = crate::enums::spelling_of_member("Hero", member) else {
                    self.issue(
                        "unsupported-enum-member",
                        format!(
                            "hero filter '{member}' on rule '{}' has no OPY spelling",
                            rule.name
                        ),
                        self.program.rule_span(rule_index),
                    );
                    return;
                };
                self.out.push_str("    @Hero ");
                self.out
                    .push_str(&crate::enums::upper_case_to_camel_case(spelling));
                self.out.push('\n');
            }
        }
    }

    fn emit_variable_declarations(&mut self, global: bool, initializers: Option<&Initializers>) {
        let (kind, variables) = if global {
            ("globalvar ", &self.program.global_variables)
        } else {
            ("playervar ", &self.program.player_variables)
        };
        for (position, variable) in variables.iter().enumerate() {
            self.out.push_str(kind);
            self.out.push_str(&variable.name);
            match initializers.and_then(|initializers| initializers.get(&position)) {
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
    fn check_initializer_slot(&mut self, initializers: Option<&Initializers>) {
        let mut taken: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            if initializers.is_some_and(|initializers| initializers.contains_key(&position)) {
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

    /// Move the fragment emitted into `out` since `prior` was swapped out to
    /// `#`-commented lines at `level`. A disabled action or condition never
    /// executes, so the comparator treats it as inert; the comment keeps the
    /// emitted text for fidelity without making it live. (A `def` body
    /// cannot carry `@Disabled`, and commenting a whole control-flow group
    /// covers `elif`/`else`/`end` lines too.)
    fn flush_commented(&mut self, prior: String, level: usize) {
        let fragment = std::mem::replace(&mut self.out, prior);
        let indent = Self::indent(level);
        for line in fragment.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                self.out.push('\n');
            } else {
                self.out.push_str(&indent);
                self.out.push_str("# ");
                self.out.push_str(trimmed);
                self.out.push('\n');
            }
        }
    }

    /// Emit an action sequence as `#`-commented lines (used for disabled
    /// subroutine bodies, which cannot carry `@Disabled` on the `def`).
    fn emit_commented_actions(&mut self, rule_index: usize, actions: &[Action]) {
        if actions.is_empty() {
            return;
        }
        let scratch = std::mem::take(&mut self.out);
        self.emit_actions(rule_index, actions, 1);
        self.flush_commented(scratch, 1);
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
                Action::Disabled { action: inner } => {
                    let action_index = *position;
                    *position += 1;
                    match &**inner {
                        Action::If { .. }
                        | Action::While { .. }
                        | Action::ForGlobalVariable { .. }
                        | Action::ForPlayerVariable { .. } => {
                            // A disabled block marker comments out the whole
                            // group; consume the nested stream exactly like
                            // the live form does so `elif`/`else`/`end` are
                            // covered by the comment.
                            let scratch = std::mem::take(&mut self.out);
                            self.emit_construct(
                                rule_index,
                                action_index,
                                inner,
                                actions,
                                position,
                                level,
                            );
                            self.flush_commented(scratch, level);
                        }
                        Action::ElseIf { .. }
                        | Action::Else
                        | Action::End
                        | Action::Disabled { .. } => {
                            self.issue(
                                "unsupported-control-flow",
                                "the disabled modifier applies to a single \
                                 executable action",
                                self.program.action_span(rule_index, action_index),
                            );
                        }
                        _ => {
                            let scratch = std::mem::take(&mut self.out);
                            self.emit_action(rule_index, action_index, inner, level);
                            self.flush_commented(scratch, level);
                        }
                    }
                }
                _ => {
                    let action_index = *position;
                    *position += 1;
                    self.emit_construct(rule_index, action_index, action, actions, position, level);
                }
            }
        }
    }

    /// Emit one action sequence element at `action_index` (already consumed
    /// in `actions` when it opens a block): block markers emit their header
    /// and consume the nested stream; everything else is a single statement.
    fn emit_construct(
        &mut self,
        rule_index: usize,
        action_index: usize,
        action: &Action,
        actions: &[Action],
        position: &mut usize,
        level: usize,
    ) {
        match action {
            Action::If { condition } => {
                let span = self.program.action_span(rule_index, action_index);
                self.out.push_str(&Self::indent(level));
                self.out.push_str("if ");
                self.emit_value(
                    condition,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push_str(":\n");
                self.emit_action_block(rule_index, actions, position, level + 1);
                while let Some(Action::ElseIf { condition }) = actions.get(*position) {
                    let action_index = *position;
                    *position += 1;
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("elif ");
                    self.emit_value(
                        condition,
                        self.program
                            .action_argument_span(rule_index, action_index, 0),
                    );
                    self.out.push_str(":\n");
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
                let span = self.program.action_span(rule_index, action_index);
                self.out.push_str(&Self::indent(level));
                self.out.push_str("while ");
                self.emit_value(
                    condition,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push_str(":\n");
                self.emit_action_block(rule_index, actions, position, level + 1);
                self.require_end(rule_index, actions, position, "while", span);
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
            } => {
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
                    self.skip_action_block(actions, position);
                    return;
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
                self.emit_action_block(rule_index, actions, position, level + 1);
                self.require_end(rule_index, actions, position, "for", span);
            }
            Action::ForPlayerVariable { .. } => {
                let span = self.program.action_span(rule_index, action_index);
                self.issue(
                    "unsupported-per-player-loop",
                    "For Player Variable is outside the reconstruction surface \
                     (the OPY `for` form binds a global variable)",
                    span,
                );
                self.skip_action_block(actions, position);
            }
            _ => self.emit_action(rule_index, action_index, action, level),
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
                self.out.push_str(variable);
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
            Action::Disabled { action: inner } => {
                // Defensive path: `emit_action_block` intercepts disabled
                // actions before they reach `emit_action`, so a bare
                // `Disabled` landing here is commented out like the rest.
                let scratch = std::mem::take(&mut self.out);
                self.emit_action(rule_index, action_index, inner, level);
                self.flush_commented(scratch, level);
            }
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
                self.out.push_str(variable);
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
                self.out.push_str(variable);
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
    use workshop_rs::{
        Action, Condition, Event, EventTarget, EventTeam, ModifyOp, PlayerEventKind, Program, Rule,
        Subroutine, Value, Variable,
    };

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
    fn disabled_initializer_reconstructs_as_a_disabled_ordinary_rule() {
        // A disabled initializer-named rule is no declaration initializer,
        // so it stays an ordinary rule at its position — with `@Disabled`.
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

        let source = reconstruct(&program).expect("disabled initializer reconstructs");
        assert!(
            source.contains(
                "rule \"Initialize global variables\":\n    @Event global\n    @Disabled\n"
            ),
            "{source}"
        );
        assert!(!source.contains("globalvar score ="), "{source}");

        let reparsed = compile_to_program(&source, "disabled-initializer.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_subroutine_rule_reconstructs_as_an_inert_def() {
        // `def` cannot carry `@Disabled`; a disabled subroutine-body rule
        // reconstructs as commented lines plus a live `pass` — behaviorally
        // inert like the disabled body.
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

        let source = reconstruct(&program).expect("disabled subroutine reconstructs");
        assert!(source.contains("def reset():\n"), "{source}");
        assert!(source.contains("    # @Disabled\n"), "{source}");
        assert!(source.contains("# score = 0"), "{source}");
        assert!(source.contains("    pass\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-subroutine.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
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
    fn each_player_filters_emit_team_and_slot_annotations() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "filtered",
                Event::EachPlayerWithFilters {
                    team: EventTeam::Team1,
                    target: EventTarget::Slot(0),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("filtered event reconstructs");
        assert!(
            source.contains("    @Event eachPlayer\n    @Team 1\n    @Slot 0\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "each-player-filters.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn each_player_hero_filter_emits_the_source_hero_spelling() {
        // `SOLDIER_76` spells `Hero.SOLDIER` in source; the `@Hero` argument
        // uses the camel-case keyword (`soldier`).
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "hero",
                Event::EachPlayerWithFilters {
                    team: EventTeam::All,
                    target: EventTarget::Hero("SOLDIER_76".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("hero filter reconstructs");
        assert!(
            source.contains("    @Event eachPlayer\n    @Hero soldier\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "each-player-hero.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn player_event_emits_the_catalog_id_and_filters() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "died",
                Event::Player {
                    kind: PlayerEventKind::Died,
                    team: EventTeam::Team2,
                    target: EventTarget::Hero("CASSIDY".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("player event reconstructs");
        assert!(
            source.contains("    @Event playerDied\n    @Team 2\n    @Hero cassidy\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "player-event.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn unspellable_hero_filter_is_refused() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "hero",
                Event::EachPlayerWithFilters {
                    team: EventTeam::All,
                    target: EventTarget::Hero("DOCTRINE".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let error = reconstruct(&program).expect_err("a hero with no OPY spelling is refused");
        assert_eq!(error.issues[0].code, "unsupported-enum-member");
    }

    #[test]
    fn disabled_rule_emits_the_disabled_annotation() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        let mut rule = Rule::new("off", Event::Global).action(Action::SetGlobalVariable {
            variable: "score".into(),
            value: Value::number(1.0),
        });
        rule.disabled = true;
        program.rule(rule);

        let source = reconstruct(&program).expect("disabled rule reconstructs");
        assert!(
            source.contains("rule \"off\":\n    @Event global\n    @Disabled\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "disabled-rule.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_condition_emits_a_commented_line() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .condition(Value::call(
                    "==",
                    [Value::global_variable("score"), Value::number(0.0)],
                ))
                .condition(Condition::disabled(Value::call(
                    "<",
                    [Value::global_variable("score"), Value::number(1.0)],
                )))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                }),
        );

        let source = reconstruct(&program).expect("disabled condition reconstructs");
        assert!(source.contains("    @Condition (score == 0)\n"), "{source}");
        assert!(
            source.contains("    # @Condition (score < 1)\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "disabled-condition.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{source}\n{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn disabled_action_emits_a_commented_line() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(Rule::new("main", Event::Global).action(Action::disabled(
            Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            },
        )));

        let source = reconstruct(&program).expect("disabled action reconstructs");
        assert!(source.contains("    # score = 1\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-action.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_control_flow_emits_a_commented_block() {
        // `Disabled` wraps the `If` marker only; the whole group comments
        // out — `elif`/`else`/`end` lines included.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::disabled(Action::If {
                    condition: Value::call(
                        "==",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                }))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                })
                .action(Action::Else)
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("disabled control flow reconstructs");
        assert!(source.contains("    # if (score == 0):\n"), "{source}");
        assert!(source.contains("    # score = 1\n"), "{source}");
        assert!(source.contains("    # else:\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-if.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rule_without_actions_emits_pass() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(Rule::new(
            "Subroutine reset",
            Event::Subroutine("reset".into()),
        ));

        let source = reconstruct(&program).expect("empty subroutine rule reconstructs");
        assert!(source.contains("def reset():\n    pass\n"), "{source}");

        let reparsed = compile_to_program(&source, "empty-subroutine.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rules_reconstruct_in_def_form_in_any_order() {
        // Subroutine-event rules may appear anywhere and carry any rule
        // name: the comparator ignores their relative order, and rule names
        // are display metadata.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("first", 0));
        program.subroutine(Subroutine::with_index("second", 1));
        program.rule(
            Rule::new("main", Event::Global).action(Action::CallSubroutine {
                subroutine: "first".into(),
            }),
        );
        program.rule(
            Rule::new("my second body", Event::Subroutine("second".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                },
            ),
        );
        program.rule(
            Rule::new("my first body", Event::Subroutine("first".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("out-of-order defs reconstruct");
        assert!(source.contains("def second():"), "{source}");
        assert!(source.contains("def first():"), "{source}");

        let reparsed = compile_to_program(&source, "def-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rule_with_conditions_stays_refused() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into()))
                .condition(Value::number(1.0))
                .action(Action::CallSubroutine {
                    subroutine: "reset".into(),
                }),
        );

        let error = reconstruct(&program).expect_err("subroutine conditions stay refused");
        assert_eq!(error.issues[0].code, "unsupported-rule-order");
    }

    #[test]
    fn duplicate_subroutine_rules_stay_refused() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        for name in ["first def", "second def"] {
            program.rule(Rule::new(name, Event::Subroutine("reset".into())));
        }

        let error = reconstruct(&program).expect_err("one subroutine, one def");
        assert_eq!(error.issues[0].code, "unsupported-rule-order");
    }

    #[test]
    fn initializer_named_rule_after_an_ongoing_rule_keeps_its_position() {
        // Initializer rules are never relocated: an initializer-named rule
        // that is not the leading canonical initializer reconstructs as an
        // ordinary rule where it sits (#597).
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(2.0),
            }),
        );
        program.rule(
            Rule::new("Initialize global variables", Event::Global).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("demoted initializer reconstructs");
        let main = source.find("rule \"main\":").expect("main rule emitted");
        let init = source
            .find("rule \"Initialize global variables\":")
            .expect("initializer-named rule emitted");
        assert!(main < init, "{source}");
        assert!(!source.contains("globalvar score ="), "{source}");

        let reparsed = compile_to_program(&source, "init-position.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn initializer_named_rule_with_conditions_stays_an_ordinary_rule() {
        // Conditions are not part of the canonical initializer shape, so
        // the rule demotes to an ordinary rule instead of refusing.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::Global)
                .condition(Value::call(
                    "==",
                    [Value::global_variable("score"), Value::number(0.0)],
                ))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                }),
        );

        let source = reconstruct(&program).expect("conditioned initializer-name reconstructs");
        assert!(
            source.contains("rule \"Initialize global variables\":"),
            "{source}"
        );
        assert!(source.contains("    @Condition (score == 0)\n"), "{source}");

        let reparsed = compile_to_program(&source, "init-condition.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{source}\n{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn initializer_named_rule_with_out_of_order_sets_stays_an_ordinary_rule() {
        // The canonical initializer Sets in declaration order; any other
        // arrangement demotes to an ordinary rule.
        let mut program = Program::new();
        program.global_variable(Variable::with_index("first", 0));
        program.global_variable(Variable::with_index("second", 1));
        program.rule(
            Rule::new("Initialize global variables", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "second".into(),
                    value: Value::number(2.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "first".into(),
                    value: Value::number(1.0),
                }),
        );

        let source = reconstruct(&program).expect("out-of-order initializer-name reconstructs");
        assert!(
            source.contains("rule \"Initialize global variables\":"),
            "{source}"
        );
        assert!(!source.contains("globalvar second ="), "{source}");

        let reparsed = compile_to_program(&source, "init-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn canonical_initializer_still_folds_into_declarations() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::Global).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("canonical initializer reconstructs");
        assert!(source.contains("globalvar score = 1\n"), "{source}");
        assert!(!source.contains("rule \""), "{source}");

        let reparsed = compile_to_program(&source, "canonical-init.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }
}

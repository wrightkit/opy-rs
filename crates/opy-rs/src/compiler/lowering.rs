use super::operator_optimization::OperatorOptimizer;
use super::size_optimization::SizeOptimizer;
use super::*;
use crate::hir::OptimizationState;

type ValueId = usize;
type ActionId = usize;
type GlobalVarId = usize;
type PlayerVarId = usize;
type SubroutineId = usize;
use workshop_rs::{Event, EventTarget, EventTeam, ModifyOp, PlayerEventKind};

const COMPRESSION_ALPHABET_NAME: &str = "__compressionAlphabet__";
const EMPTY_STRING_NAME: &str = "__emptyString__";

use super::blizzard_global;

fn is_cased_color_tag(text: &[char], index: usize) -> Option<usize> {
    let remaining = text[index..].iter().collect::<String>();
    let is_tag = remaining
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<fg"))
        || remaining
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("</fg>"));
    if !is_tag {
        return None;
    }
    remaining
        .find('>')
        .map(|offset| index + remaining[..=offset].chars().count())
}

fn cased_line(text: &str, text_count: usize) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut text_without_tags = String::new();
    let mut plain_index = 0;
    while plain_index < characters.len() {
        if let Some(end) = is_cased_color_tag(&characters, plain_index) {
            plain_index = end;
        } else {
            text_without_tags.push(characters[plain_index]);
            plain_index += 1;
        }
    }
    let mut text_width = 0;
    let mut found_lowercase = false;
    for character in text_without_tags.chars() {
        if let Some(glyph) = blizzard_global::cased_glyph(character) {
            found_lowercase = true;
            if !matches!(character, 'i' | 'j' | 'l') {
                text_width = (glyph.lower_xmin - text_width).max(0);
                break;
            }
        } else {
            text_width += blizzard_global::width(character);
        }
    }
    if !found_lowercase {
        return vec![text.to_string(); text_count];
    }

    let mut outputs = vec![String::new(); text_count];
    let mut widths = vec![0; text_count];
    let mut text_index = 0;
    let mut last_character = None;
    let mut index = 0;
    while index < characters.len() {
        if let Some(end) = is_cased_color_tag(&characters, index) {
            let tag = characters[index..end].iter().collect::<String>();
            for output in &mut outputs {
                output.push_str(&tag);
            }
            text_index = (text_index + 1) % text_count;
            index = end;
            continue;
        }
        let character = characters[index];
        if let Some(glyph) = blizzard_global::cased_glyph(character) {
            text_index = (text_index + 1) % text_count;
            let padding = (text_width - widths[text_index] - glyph.lower_xmin + glyph.xmin).max(0);
            outputs[text_index].push_str(&blizzard_global::spaces(padding));
            widths[text_index] += padding;
            outputs[text_index].push_str(glyph.lower);
            widths[text_index] += glyph.lower_width;
            last_character = Some(character);
        } else if character != ' ' {
            if outputs[text_index].is_empty()
                || last_character
                    .and_then(blizzard_global::cased_glyph)
                    .is_some()
                || last_character == Some(' ')
            {
                text_index = (text_index + 1) % text_count;
                let padding = (text_width - widths[text_index]).max(0);
                outputs[text_index].push_str(&blizzard_global::spaces(padding));
                widths[text_index] += padding;
            }
            outputs[text_index].push(character);
            widths[text_index] += blizzard_global::width(character);
            last_character = Some(character);
        } else {
            last_character = Some(character);
        }
        text_width += blizzard_global::cased_glyph(character)
            .map_or_else(|| blizzard_global::width(character), |glyph| glyph.width);
        index += 1;
    }
    let maximum = widths
        .iter()
        .copied()
        .max()
        .unwrap_or_default()
        .max(text_width);
    for (output, width) in outputs.iter_mut().zip(widths) {
        output.push_str(&blizzard_global::spaces(maximum - width));
    }
    outputs
}

#[derive(Debug, Clone)]
enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Array(Vec<ValueId>),
    Vector { x: ValueId, y: ValueId, z: ValueId },
    Enum { value_type: String, value: String },
    GlobalVariable(String),
    PlayerVariable { player: ValueId, variable: String },
    Subroutine(String),
    EventPlayer,
    Call { name: String, args: Vec<ValueId> },
}

#[derive(Debug, Clone)]
enum Action {
    SetGlobalVariable {
        variable: String,
        value: ValueId,
    },
    ModifyGlobalVariable {
        variable: String,
        op: ModifyOp,
        value: ValueId,
    },
    SetPlayerVariable {
        player: ValueId,
        variable: String,
        value: ValueId,
    },
    ModifyPlayerVariable {
        player: ValueId,
        variable: String,
        op: ModifyOp,
        value: ValueId,
    },
    CallSubroutine {
        subroutine: String,
    },
    If {
        condition: ValueId,
    },
    ElseIf {
        condition: ValueId,
    },
    Else,
    While {
        condition: ValueId,
    },
    ForGlobalVariable {
        variable: String,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
    },
    ForPlayerVariable {
        player: ValueId,
        variable: String,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
    },
    End,
    Call {
        name: String,
        args: Vec<ValueId>,
    },
}

pub(crate) struct Lowering<'a> {
    compiler: &'a Compiler,
    hir: &'a hir::Program,
    pub(super) program: Program,
    values: Vec<Value>,
    actions: Vec<Action>,
    action_origins: Vec<Option<HirSpan>>,
    action_argument_origins: Vec<Vec<Option<HirSpan>>>,
    globals: HashMap<String, GlobalVarId>,
    global_names: Vec<String>,
    players: HashMap<String, PlayerVarId>,
    player_names: Vec<String>,
    subroutines: HashMap<String, SubroutineId>,
    subroutine_names: Vec<String>,
    constants: HashMap<String, &'a Expr>,
    defined_subroutines: HashSet<SubroutineId>,
    array_bindings: Vec<ArrayBinding>,
    current_rule_conditions: Option<Vec<ValueId>>,
    visible_labels: Vec<HashSet<String>>,
    deferred_gotos: Vec<(ActionId, String, Option<HirSpan>, usize)>,
    translation_uses: Vec<(String, Option<String>)>,
    optimized_nodes: HashMap<ValueId, bool>,
}

#[derive(Debug, Clone)]
struct ArrayBinding {
    element: String,
    index: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum BreakTarget {
    Loop,
    DoWhile,
    Switch,
}

type SwitchBreak = (usize, HirSpan);
type LoweredSwitchBody = (Vec<ActionId>, Option<SwitchBreak>);
type LoweredSwitchArm<'a> = (Option<&'a Expr>, Vec<ActionId>, Option<SwitchBreak>);

fn pure_continue_conditions(statement: &Stmt) -> Option<Vec<&Expr>> {
    match statement {
        Stmt::Continue { .. } => Some(Vec::new()),
        Stmt::If {
            branches,
            r#else: None,
            ..
        } if branches.len() == 1 && branches[0].body.len() == 1 => {
            let mut conditions = pure_continue_conditions(&branches[0].body[0])?;
            conditions.insert(0, &branches[0].condition);
            Some(conditions)
        }
        _ => None,
    }
}

fn pure_goto_conditions(statement: &Stmt) -> Option<(Vec<&Expr>, &str)> {
    match statement {
        Stmt::Goto {
            label: Some(label),
            offset: None,
            rule_start: false,
            ..
        } => Some((Vec::new(), label.as_str())),
        Stmt::If {
            branches,
            r#else: None,
            ..
        } if branches.len() == 1 && branches[0].body.len() == 1 => {
            let (mut conditions, label) = pure_goto_conditions(&branches[0].body[0])?;
            conditions.insert(0, &branches[0].condition);
            Some((conditions, label))
        }
        _ => None,
    }
}

fn direct_conditional_goto(statement: &Stmt) -> Option<(&Expr, &str, Option<HirSpan>)> {
    let Stmt::If {
        branches,
        r#else: None,
        span,
    } = statement
    else {
        return None;
    };
    let [branch] = branches.as_slice() else {
        return None;
    };
    let [
        Stmt::Goto {
            label: Some(label),
            offset: None,
            rule_start: false,
            ..
        },
    ] = branch.body.as_slice()
    else {
        return None;
    };
    Some((&branch.condition, label.as_str(), *span))
}

fn direct_conditional_dynamic_goto(statement: &Stmt) -> Option<(&Expr, &Expr, Option<HirSpan>)> {
    let Stmt::If {
        branches,
        r#else: None,
        span,
    } = statement
    else {
        return None;
    };
    let [branch] = branches.as_slice() else {
        return None;
    };
    let [
        Stmt::Goto {
            label: None,
            offset: Some(offset),
            rule_start: false,
            ..
        },
    ] = branch.body.as_slice()
    else {
        return None;
    };
    Some((&branch.condition, offset, *span))
}

fn contains_loop_continue(statement: &Stmt) -> bool {
    match statement {
        Stmt::Continue { .. } => true,
        Stmt::If {
            branches, r#else, ..
        } => {
            branches
                .iter()
                .any(|branch| branch.body.iter().any(contains_loop_continue))
                || r#else
                    .as_ref()
                    .is_some_and(|body| body.iter().any(contains_loop_continue))
        }
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => false,
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| match arm {
            SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                body.iter().any(contains_loop_continue)
            }
        }),
        _ => false,
    }
}

fn switch_body_is_noop(statements: &[Stmt]) -> bool {
    statements.iter().all(|statement| match statement {
        Stmt::Pass { .. } | Stmt::Break { .. } => true,
        Stmt::If {
            branches, r#else, ..
        } => {
            branches
                .iter()
                .all(|branch| switch_body_is_noop(&branch.body))
                && r#else.as_ref().is_none_or(|body| switch_body_is_noop(body))
        }
        Stmt::Switch { arms, .. } => arms.iter().all(|arm| match arm {
            SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                switch_body_is_noop(body)
            }
        }),
        _ => false,
    })
}

impl<'a> Lowering<'a> {
    pub(super) fn new(
        compiler: &'a Compiler,
        hir: &'a hir::Program,
    ) -> Result<Self, IntegrationError> {
        Ok(Self {
            compiler,
            hir,
            program: Program::default(),
            values: Vec::new(),
            actions: Vec::new(),
            action_origins: Vec::new(),
            action_argument_origins: Vec::new(),
            globals: HashMap::new(),
            global_names: Vec::new(),
            players: HashMap::new(),
            player_names: Vec::new(),
            subroutines: HashMap::new(),
            subroutine_names: Vec::new(),
            constants: HashMap::new(),
            defined_subroutines: HashSet::new(),
            array_bindings: Vec::new(),
            current_rule_conditions: None,
            optimized_nodes: HashMap::new(),
            visible_labels: Vec::new(),
            deferred_gotos: Vec::new(),
            translation_uses: Vec::new(),
        })
    }

    pub(super) fn copy_files(&mut self) -> Result<(), IntegrationError> {
        for file in &self.hir.files {
            self.program
                .add_file(workshop_rs::source::SourceFile::new(file.path.clone()));
        }
        let settings_constants = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Constant { name, value, .. } => {
                    Some((name.clone(), value.as_ref()))
                }
                _ => None,
            })
            .collect();
        self.program.settings = super::settings::merge_extensions(
            self.hir.settings.clone().map(|settings| {
                super::settings::expand_settings_constants(settings, &settings_constants)
            }),
            &self.hir.preprocessing.directives,
        )?;
        Ok(())
    }

    fn translation_helper_index(
        &self,
        reserved: &HashSet<u32>,
    ) -> Result<Option<u32>, IntegrationError> {
        if self.hir.preprocessing.translations.is_none() {
            return Ok(None);
        }
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    "no available global variable index remains for translations",
                    self.hir
                        .preprocessing
                        .translations
                        .as_ref()
                        .and_then(|value| value.span),
                )
            })
    }

    fn compression_alphabet_index(
        &self,
        reserved: &HashSet<u32>,
    ) -> Result<Option<u32>, IntegrationError> {
        if !has_directive(self.hir, "useVariableForCompressionAlphabet") {
            return Ok(None);
        }
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    "no available global variable index remains for the compression alphabet",
                    self.hir
                        .preprocessing
                        .directives
                        .iter()
                        .find(|directive| directive.name == "useVariableForCompressionAlphabet")
                        .and_then(|directive| directive.span),
                )
            })
    }

    fn helper_global_index(
        &self,
        reserved: &HashSet<u32>,
        directive: &str,
        message: &str,
    ) -> Result<Option<u32>, IntegrationError> {
        if !has_directive(self.hir, directive) {
            return Ok(None);
        }
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    message,
                    self.hir
                        .preprocessing
                        .directives
                        .iter()
                        .find(|item| item.name == directive)
                        .and_then(|item| item.span),
                )
            })
    }

    pub(super) fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
        let (implicit_globals, implicit_players) = implicit_default_variables(self.hir);
        for declaration in &self.hir.declarations {
            if let hir::Declaration::GlobalVariable {
                name,
                index: Some(index),
                span,
                ..
            } = declaration
            {
                for (implicit_name, implicit_span) in &implicit_globals {
                    if default_var_index(implicit_name) == Some(*index) {
                        return Err(IntegrationError::new(
                            "index-collision",
                            format!(
                                "duplicate use of index {index} for global variables '{implicit_name}' and '{name}'"
                            ),
                            implicit_span.or(*span),
                        ));
                    }
                }
            }
            if let hir::Declaration::PlayerVariable {
                name,
                index: Some(index),
                span,
                ..
            } = declaration
            {
                for (implicit_name, implicit_span) in &implicit_players {
                    if implicit_player_index(implicit_name) == *index {
                        return Err(IntegrationError::new(
                            "index-collision",
                            format!(
                                "duplicate use of index {index} for player variables '{implicit_name}' and '{name}'"
                            ),
                            implicit_span.or(*span),
                        ));
                    }
                }
            }
        }

        let globals = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::GlobalVariable { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let players = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::PlayerVariable { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let subroutines = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Subroutine { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let implicit_reserved = implicit_globals
            .keys()
            .map(|name| default_var_index(name).expect("implicit default variable names resolve"))
            .collect::<HashSet<_>>();
        let implicit_player_reserved = implicit_players
            .keys()
            .map(|name| implicit_player_index(name))
            .collect::<HashSet<_>>();
        let mut helper_reserved = implicit_reserved.clone();
        helper_reserved.extend(self.hir.declarations.iter().filter_map(|declaration| {
            match declaration {
                hir::Declaration::GlobalVariable {
                    index: Some(index), ..
                } => Some(*index),
                _ => None,
            }
        }));
        let translation_helper_index = self.translation_helper_index(&helper_reserved)?;
        let mut global_reserved = implicit_reserved.clone();
        if let Some(index) = translation_helper_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let compression_alphabet_index = self.compression_alphabet_index(&helper_reserved)?;
        if let Some(index) = compression_alphabet_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let empty_string_index = self.helper_global_index(
            &helper_reserved,
            "replaceEmptyStringByVariable",
            "no available global variable index remains for the empty-string replacement",
        )?;
        if let Some(index) = empty_string_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let empty = HashSet::new();
        let global_indices = allocate_indices(&globals, &global_reserved, "global variable")?;
        let player_indices =
            allocate_indices(&players, &implicit_player_reserved, "player variable")?;
        let subroutine_indices = allocate_indices(&subroutines, &empty, "subroutine")?;
        let mut global_index = 0;
        let mut player_index = 0;
        let mut subroutine_index = 0;

        // Declared variables in source order (for duplicate detection and
        // initializer action order), then merged with the implicit default
        // variables and created in Workshop index order so the emitted
        // variable tables are reference-compatible.
        let mut declared_globals: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
        let mut global_initializers = Vec::new();
        let mut declared_players: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
        let mut player_initializers = Vec::new();
        let mut declared_subroutines: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> =
            Vec::new();

        for declaration in &self.hir.declarations {
            match declaration {
                hir::Declaration::GlobalVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    let assigned = global_indices[global_index];
                    global_index += 1;
                    if declared_globals
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        ));
                    }
                    declared_globals.push((name, assigned, *span, *name_span));
                    if let Some(init) = initializer {
                        if !is_zero_initializer(init) {
                            global_initializers.push((name, init, *span, *name_span));
                        }
                    }
                }
                hir::Declaration::PlayerVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    let assigned = player_indices[player_index];
                    player_index += 1;
                    if declared_players
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        ));
                    }
                    declared_players.push((name, assigned, *span, *name_span));
                    if let Some(init) = initializer {
                        if !is_zero_initializer(init) {
                            player_initializers.push((name, init, *span, *name_span));
                        }
                    }
                }
                hir::Declaration::Subroutine {
                    name,
                    span,
                    name_span,
                    ..
                } => {
                    let assigned = subroutine_indices[subroutine_index];
                    subroutine_index += 1;
                    if declared_subroutines
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate subroutine '{name}'"),
                            *span,
                        ));
                    }
                    declared_subroutines.push((name, assigned, *span, *name_span));
                }
                hir::Declaration::Constant { name, value, span } => {
                    if self.constants.insert(name.clone(), value).is_some() {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate constant '{name}'"),
                            *span,
                        ));
                    }
                }
                hir::Declaration::Macro { .. } => {}
            }
        }

        let mut planned_globals: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> =
            declared_globals
                .into_iter()
                .map(|(name, index, span, name_span)| (name.to_string(), index, span, name_span))
                .collect();
        planned_globals.extend(implicit_globals.iter().map(|(name, span)| {
            (
                name.clone(),
                default_var_index(name).expect("implicit default variable names resolve"),
                *span,
                None,
            )
        }));
        if let Some(index) = translation_helper_index {
            planned_globals.push((TRANSLATION_HELPER_NAME.to_string(), index, None, None));
        }
        if let Some(index) = compression_alphabet_index {
            planned_globals.push((COMPRESSION_ALPHABET_NAME.to_string(), index, None, None));
        }
        if let Some(index) = empty_string_index {
            planned_globals.push((EMPTY_STRING_NAME.to_string(), index, None, None));
        }
        planned_globals.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_globals {
            let _ = (span, name_span);
            let id = self.global_names.len();
            self.global_names.push(name.clone());
            self.globals.insert(name.clone(), id);
            self.program
                .global_variables
                .push(workshop_rs::Variable::with_index(name, assigned));
            self.program
                .set_global_variable_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
        }

        let mut planned_players: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> =
            declared_players
                .into_iter()
                .map(|(name, index, span, name_span)| (name.to_string(), index, span, name_span))
                .collect();
        planned_players.extend(
            implicit_players
                .iter()
                .map(|(name, span)| (name.clone(), implicit_player_index(name), *span, None)),
        );
        planned_players.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_players {
            let _ = (span, name_span);
            let id = self.player_names.len();
            self.player_names.push(name.clone());
            self.players.insert(name, id);
            self.program
                .player_variables
                .push(workshop_rs::Variable::with_index(
                    self.player_names[id].clone(),
                    assigned,
                ));
            self.program
                .set_player_variable_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
        }

        declared_subroutines.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in declared_subroutines {
            let _ = (span, name_span);
            let id = self.subroutine_names.len();
            self.subroutine_names.push(name.to_string());
            self.subroutines.insert(name.to_string(), id);
            self.program
                .subroutines
                .push(workshop_rs::Subroutine::with_index(name, assigned));
            self.program
                .set_subroutine_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
        }

        let empty_string_initializer = empty_string_index
            .map(|_| {
                let variable = *self
                    .globals
                    .get(EMPTY_STRING_NAME)
                    .expect("empty string helper variable is created");
                let empty_array = self.push_call("emptyArray", Vec::new());
                let null = self.push_value(Value::Null);
                let value = self.push_call("charAt", vec![empty_array, null]);
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                Ok(action)
            })
            .transpose()?;

        if has_directive(self.hir, "disableInspector") {
            let action = self.push_call_action("disableInspector", &[]);
            self.push_generated_rule("Disable inspector", Event::Global, vec![action])?;
        }
        let translation_initializer = self
            .hir
            .preprocessing
            .translations
            .as_ref()
            .map(|translations| {
                let variable = *self
                    .globals
                    .get(TRANSLATION_HELPER_NAME)
                    .expect("translation helper variable is created");
                let value = self.lower_translation_helper(translations)?;
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), translations.span);
                self.mark_action_argument_origins(action, [translations.span]);
                Ok(action)
            })
            .transpose()?;

        let compression_alphabet_initializer = compression_alphabet_index
            .map(|_| {
                let variable = *self
                    .globals
                    .get(COMPRESSION_ALPHABET_NAME)
                    .expect("compression alphabet variable is created");
                let value = self.lower_custom_string(compression_alphabet(), None)?;
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                Ok(action)
            })
            .transpose()?;

        let (uses_player_translation_var, no_detection_rule, no_tl_err) =
            self.translation_player_options();
        if uses_player_translation_var && !no_detection_rule {
            let translations = self
                .hir
                .preprocessing
                .translations
                .clone()
                .expect("player translation mode requires translations");
            self.lower_translation_detection_rule(no_tl_err, &translations)?;
        }

        if translation_initializer.is_some()
            || empty_string_initializer.is_some()
            || compression_alphabet_initializer.is_some()
            || !global_initializers.is_empty()
        {
            let mut actions = Vec::with_capacity(
                global_initializers.len()
                    + usize::from(translation_initializer.is_some())
                    + usize::from(empty_string_initializer.is_some())
                    + usize::from(compression_alphabet_initializer.is_some()),
            );
            if let Some(action) = translation_initializer {
                actions.push(action);
            }
            if let Some(action) = compression_alphabet_initializer {
                actions.push(action);
            }
            if let Some(action) = empty_string_initializer {
                actions.push(action);
            }
            for (name, init_expr, span, _target_span) in global_initializers {
                let variable = *self.globals.get(name).expect("declared global is created");
                let value = self.lower_value(init_expr)?;
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), span);
                self.mark_action_argument_origins(action, [init_expr.span().copied()]);
                actions.push(action);
            }
            let rule_index = self.program.rules.len();
            self.program.rules.push(workshop_rs::Rule {
                name: self.global_initializer_rule_name(),
                disabled: false,
                event: workshop_rs::Event::Global,
                conditions: Vec::new(),
                actions: self.public_actions(&actions),
            });
            let action_provenance = self.action_provenance(&actions);
            self.set_rule_provenance(rule_index, None, std::iter::empty(), action_provenance)?;
        }

        if uses_player_translation_var || !player_initializers.is_empty() {
            let mut actions = Vec::with_capacity(
                player_initializers.len() + usize::from(uses_player_translation_var),
            );
            if uses_player_translation_var {
                let variable = *self
                    .players
                    .get("__languageIndex__")
                    .expect("translation player variable is created");
                let player = self.push_value(Value::EventPlayer);
                let value = self.push_number(
                    if no_tl_err { 0.1 } else { 1.1 },
                    if no_tl_err { "0.1" } else { "1.1" },
                );
                actions.push(self.push_action(Action::SetPlayerVariable {
                    player,
                    variable: self.player_names[variable].clone(),
                    value,
                }));
            }
            for (name, init_expr, span, _target_span) in player_initializers {
                let variable = *self
                    .players
                    .get(name)
                    .expect("declared player variable is created");
                let player = self.push_value(Value::EventPlayer);
                let value = self.lower_value(init_expr)?;
                let action = self.push_action(Action::SetPlayerVariable {
                    player,
                    variable: self.player_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), span);
                self.mark_action_argument_origins(action, [None, init_expr.span().copied()]);
                actions.push(action);
            }
            let rule_index = self.program.rules.len();
            self.program.rules.push(workshop_rs::Rule {
                name: self.player_initializer_rule_name(),
                disabled: false,
                event: workshop_rs::Event::EachPlayer,
                conditions: Vec::new(),
                actions: self.public_actions(&actions),
            });
            let action_provenance = self.action_provenance(&actions);
            self.set_rule_provenance(rule_index, None, std::iter::empty(), action_provenance)?;
        }

        Ok(())
    }

    fn push_generated_rule(
        &mut self,
        name: &str,
        event: Event,
        actions: Vec<ActionId>,
    ) -> Result<(), IntegrationError> {
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: name.to_string(),
            disabled: false,
            event,
            conditions: Vec::new(),
            actions: self.public_actions(&actions),
        });
        self.set_rule_provenance(
            rule_index,
            None,
            std::iter::empty(),
            self.action_provenance(&actions),
        )
    }

    fn translation_player_options(&self) -> (bool, bool, bool) {
        let Some(directive) = self
            .hir
            .preprocessing
            .directives
            .iter()
            .find(|directive| directive.name == "translateWithPlayerVar")
        else {
            return (false, false, false);
        };
        let options = directive.value.as_deref().unwrap_or_default();
        (
            true,
            options
                .split_whitespace()
                .any(|option| option == "noDetectionRule"),
            options.split_whitespace().any(|option| option == "noTlErr"),
        )
    }

    fn lower_translation_detection_rule(
        &mut self,
        no_tl_err: bool,
        translations: &hir::TranslationState,
    ) -> Result<(), IntegrationError> {
        let variable = self
            .players
            .get("__languageIndex__")
            .copied()
            .expect("translation player variable is created");
        let player = self.push_value(Value::EventPlayer);
        let language = self.push_value(Value::PlayerVariable {
            player,
            variable: self.player_names[variable].clone(),
        });
        let initial = self.push_number(
            if no_tl_err { 0.1 } else { 1.1 },
            if no_tl_err { "0.1" } else { "1.1" },
        );
        let has_spawned = self.push_call("hasSpawned", vec![player]);
        let is_dummy = self.push_call("isDummy", vec![player]);
        let false_value = self.push_value(Value::Bool(false));
        let not_dummy = self.push_call("==", vec![is_dummy, false_value]);
        let initial_language = self.push_call("==", vec![language, initial]);

        let facing = self.push_call("getFacingDirection", vec![player]);
        let append = self.push_action(Action::ModifyPlayerVariable {
            player,
            variable: self.player_names[variable].clone(),
            op: ModifyOp::AppendToArray,
            value: facing,
        });
        let ten = self.push_number(10.0, "10");
        let direction_index = self.translation_language_index(translations)?;
        let horizontal = self.push_call("multiply", vec![ten, direction_index]);
        let vertical = self.push_number(5.0, "5");
        let direction = self.push_call("directionFromAngles", vec![horizontal, vertical]);
        let turn_rate = self.push_number(999_999_999_999.0, "999999999999");
        let to_world = self.push_value(Value::Enum {
            value_type: "Relativity".to_string(),
            value: "TO_WORLD".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "FacingReeval".to_string(),
            value: "DIRECTION_AND_TURN_RATE".to_string(),
        });
        let start_facing = self.push_call_action(
            "startFacing",
            &[player, direction, turn_rate, to_world, reevaluation],
        );

        let horizontal_angle = self.push_call("getHorizontalFacingAngle", vec![player]);
        let one_hundred = self.push_number(100.0, "100");
        let horizontal_times_hundred =
            self.push_call("multiply", vec![horizontal_angle, one_hundred]);
        let nearest = self.push_value(Value::Enum {
            value_type: "Rounding".to_string(),
            value: "NEAREST".to_string(),
        });
        let rounded_horizontal =
            self.push_call("roundToInteger", vec![horizontal_times_hundred, nearest]);
        let thousand = self.push_number(1000.0, "1000");
        let modulo = self.push_call("modulo", vec![rounded_horizontal, thousand]);
        let zero = self.push_number(0.0, "0");
        let modulo_zero = self.push_call("not", vec![modulo]);
        let vertical_angle = self.push_call("getVerticalFacingAngle", vec![player]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let tolerance = self.push_number(0.01, "0.01");
        let vertical_close = self.push_call("<", vec![vertical_delta, tolerance]);
        let wait_condition = self.push_call("and", vec![modulo_zero, vertical_close]);
        let timeout = self.push_number(15.0, "15");
        let wait = self.push_call_action("waitUntil", &[wait_condition, timeout]);

        let ten_for_angle = self.push_number(10.0, "10");
        let horizontal_divided = self.push_call("divide", vec![horizontal_angle, ten_for_angle]);
        let rounded_angle = self.push_call("roundToInteger", vec![horizontal_divided, nearest]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let vertical_match = self.push_call("<", vec![vertical_delta, tolerance]);
        let one = self.push_number(1.0, "1");
        let matched_language = self.push_call("multiply", vec![vertical_match, rounded_angle]);
        let language_value = self.push_call("max", vec![one, matched_language]);
        let set_index = self.push_call_action(
            "setPlayerVariableAtIndex",
            &[language, zero, language_value],
        );
        let stop_facing = self.push_call_action("stopFacing", &[player]);
        let last = self.push_call("lastOf", vec![language]);
        let set_facing = self.push_call_action("setFacing", &[player, last, to_world]);
        let finish = if no_tl_err {
            self.push_action(Action::ModifyPlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
                op: ModifyOp::Subtract,
                value: one,
            })
        } else {
            let final_value = self.push_call("firstOf", vec![language]);
            self.push_action(Action::SetPlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
                value: final_value,
            })
        };

        let actions = [
            append,
            start_facing,
            wait,
            set_index,
            stop_facing,
            set_facing,
            finish,
        ];
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: "OverPy translation setup - Determine the player's language".to_string(),
            disabled: false,
            event: Event::EachPlayer,
            conditions: vec![
                workshop_rs::Condition::new(self.materialize_value(has_spawned)),
                workshop_rs::Condition::new(self.materialize_value(not_dummy)),
                workshop_rs::Condition::new(self.materialize_value(initial_language)),
            ],
            actions: self.public_actions(&actions),
        });
        self.set_rule_provenance(
            rule_index,
            None,
            [None, None, None],
            self.action_provenance(&actions),
        )?;
        Ok(())
    }

    pub(super) fn lower_rules(&mut self) -> Result<(), IntegrationError> {
        for entry in &self.hir.rules {
            match entry {
                RuleEntry::Rule(rule) => self.lower_rule(rule)?,
                RuleEntry::SubroutineDef {
                    name,
                    source_name,
                    span,
                    name_span,
                    body,
                    annotations,
                    ..
                } => {
                    self.lower_subroutine(name, source_name, *span, *name_span, body, annotations)?
                }
            }
        }
        for rule in &mut self.program.rules {
            rule.name = escape_rule_name(&rule.name);
        }
        Ok(())
    }

    fn lower_rule(&mut self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        self.reject_rule_metadata(rule)?;
        let event = self.lower_event(&rule.event, &rule.annotations)?;
        let mut condition_exprs = Vec::new();
        for expr in &rule.conditions {
            Self::split_rule_condition(expr, &mut condition_exprs);
        }
        let conditions = condition_exprs
            .iter()
            .map(|expr| self.lower_condition(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let previous_conditions = self.current_rule_conditions.replace(conditions.clone());
        let lowered_actions = self.lower_actions(&rule.actions, None);
        self.current_rule_conditions = previous_conditions;
        let mut actions = Vec::new();
        actions.extend(lowered_actions?);
        let optimization = self.optimization_state_at(rule.span.as_ref());
        if optimization.enabled
            && !rule.delimiter
            && !self.has_meaningful_rule_action(&actions, &event)
        {
            return Ok(());
        }
        let elide_noop_switch = actions.is_empty()
            && rule.actions.len() == 1
            && matches!(rule.actions.first(), Some(Stmt::Switch { .. }));
        if elide_noop_switch && !rule.disabled {
            return Ok(());
        }
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: rule.name.clone(),
            disabled: rule.disabled,
            event,
            conditions: conditions
                .iter()
                .zip(&condition_exprs)
                .map(|(value, expr)| {
                    let mut condition = self.materialize_value(*value);
                    let optimization = self.optimization_state_at(expr.span());
                    if optimization.enabled && optimization.for_size {
                        SizeOptimizer::new(self.compiler).condition(&mut condition);
                    }
                    workshop_rs::Condition::new(
                        OperatorOptimizer::new(self.compiler, optimization.strict)
                            .wrap_condition(condition),
                    )
                })
                .collect(),
            actions: self.public_actions(&actions),
        });
        let action_provenance = self.action_provenance(&actions);
        self.set_rule_provenance(
            rule_index,
            rule.span,
            condition_exprs.iter().map(|expr| expr.span().copied()),
            action_provenance,
        )?;
        Ok(())
    }

    fn split_rule_condition<'expr>(expr: &'expr Expr, conditions: &mut Vec<&'expr Expr>) {
        match expr {
            Expr::Binary {
                op, left, right, ..
            } if op == "and" => {
                Self::split_rule_condition(left, conditions);
                Self::split_rule_condition(right, conditions);
            }
            Expr::Binary {
                op, left, right, ..
            } if op == "=="
                && matches!(right.as_ref(), Expr::Bool { value: true, .. })
                && matches!(left.as_ref(), Expr::Binary { op, .. } if op == "and") =>
            {
                Self::split_rule_condition(left, conditions);
            }
            _ => conditions.push(expr),
        }
    }

    fn has_meaningful_rule_action(&self, actions: &[ActionId], event: &Event) -> bool {
        actions
            .iter()
            .any(|action| match self.actions.get(*action) {
                Some(
                    Action::If { .. }
                    | Action::ElseIf { .. }
                    | Action::Else
                    | Action::While { .. }
                    | Action::End,
                ) => false,
                Some(Action::CallSubroutine { .. }) => true,
                Some(Action::Call { name, .. }) => match name.as_str() {
                    "abortIf" | "break" | "continue" | "loop" | "loopIf" | "return" | "skip"
                    | "skipIf" => false,
                    "wait" => matches!(event, Event::Subroutine(_)),
                    _ => true,
                },
                Some(_) => true,
                None => false,
            })
    }

    fn lower_subroutine(
        &mut self,
        name: &str,
        source_name: &str,
        span: Option<HirSpan>,
        name_span: Option<HirSpan>,
        body: &[Stmt],
        annotations: &[hir::Annotation],
    ) -> Result<(), IntegrationError> {
        self.reject_subroutine_metadata(annotations)?;
        let source_name = if source_name.is_empty() {
            name
        } else {
            source_name
        };
        let subroutine = *self.subroutines.get(source_name).ok_or_else(|| {
            self.unsupported(
                format!("subroutine definition '{source_name}' has no declaration"),
                name_span.or(span),
            )
        })?;
        if !self.defined_subroutines.insert(subroutine) {
            return Err(self.unsupported(
                format!("subroutine '{source_name}' has multiple definitions"),
                name_span.or(span),
            ));
        }
        let mut actions = Vec::new();
        actions.extend(self.lower_actions(body, None)?);
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: self.subroutine_rule_name(name),
            disabled: false,
            event: Event::Subroutine(self.subroutine_names[subroutine].clone()),
            conditions: Vec::new(),
            actions: self.public_actions(&actions),
        });
        let action_provenance = self.action_provenance(&actions);
        self.set_rule_provenance(rule_index, span, std::iter::empty(), action_provenance)?;
        Ok(())
    }

    fn reject_rule_metadata(&self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        if rule.new_page.is_some() {
            let span = rule
                .annotations
                .iter()
                .find(|annotation| annotation.name == "NewPage")
                .and_then(|annotation| annotation.span)
                .or(rule.span);
            return Err(self.unsupported(
                "rule new-page metadata is not representable in canonical WIR",
                span,
            ));
        }
        for annotation in &rule.annotations {
            match annotation.name.as_str() {
                "Event" | "Condition" | "Team" | "Slot" | "Hero" | "Disabled" | "Delimiter"
                | "SuppressWarnings" => {}
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "rule annotation '{}' is not representable in canonical WIR",
                            annotation.name
                        ),
                        annotation.span.or(rule.span),
                    ));
                }
            }
        }
        Ok(())
    }

    fn reject_subroutine_metadata(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<(), IntegrationError> {
        for annotation in annotations {
            match annotation.name.as_str() {
                "Name" | "SuppressWarnings" => {}
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "subroutine annotation '{}' is not representable in canonical WIR",
                            annotation.name
                        ),
                        annotation.span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn subroutine_rule_name(&self, generated_name: &str) -> String {
        if self.hir.preprocessing.rule_prefix_template.is_some() {
            generated_name.to_string()
        } else {
            format!("Subroutine {generated_name}")
        }
    }

    fn global_initializer_rule_name(&self) -> String {
        directive_value(self.hir, "globalvarInitRuleName")
            .map(str::to_string)
            .unwrap_or_else(|| {
                crate::lower::render_generated_rule_name(
                    "Initialize global variables",
                    &self.hir.preprocessing,
                )
            })
    }

    fn player_initializer_rule_name(&self) -> String {
        directive_value(self.hir, "playervarInitRuleName")
            .map(str::to_string)
            .unwrap_or_else(|| "Initialize player variables".to_string())
    }

    fn lower_event(
        &self,
        event: &hir::Event,
        annotations: &[hir::Annotation],
    ) -> Result<Event, IntegrationError> {
        if !event.args.is_empty() {
            return Err(self.unsupported(
                "event arguments are not representable in canonical WIR; use structural event filters",
                event.span,
            ));
        }
        let team = self.lower_event_team(annotations)?;
        let target = self.lower_event_target(annotations)?;
        let has_filters = !matches!(team, EventTeam::All) || !matches!(target, EventTarget::All);
        match event.name.as_str() {
            "global" => {
                if has_filters {
                    return Err(
                        self.unsupported("global events cannot have player filters", event.span)
                    );
                }
                Ok(Event::Global)
            }
            "eachPlayer" => {
                if has_filters {
                    Ok(Event::EachPlayerWithFilters { team, target })
                } else {
                    Ok(Event::EachPlayer)
                }
            }
            name => player_event_kind(name).map_or_else(
                || {
                    Err(self.unsupported(
                        format!("event '{name}' is not supported by canonical WIR"),
                        event.span,
                    ))
                },
                |kind| Ok(Event::Player { kind, team, target }),
            ),
        }
    }

    fn lower_event_team(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<EventTeam, IntegrationError> {
        let team_annotations = annotations
            .iter()
            .filter(|annotation| annotation.name == "Team")
            .collect::<Vec<_>>();
        if team_annotations.len() > 1 {
            return Err(self.unsupported(
                "an event cannot have multiple @Team filters",
                team_annotations[1].span.or(team_annotations[0].span),
            ));
        }
        let Some(annotation) = team_annotations.first() else {
            return Ok(EventTeam::All);
        };
        let argument = annotation
            .args
            .first()
            .ok_or_else(|| self.unsupported("@Team requires one filter value", annotation.span))?;
        if annotation.args.len() != 1 {
            return Err(
                self.unsupported("@Team requires exactly one filter value", annotation.span)
            );
        }
        let spelling = match argument.text.as_str() {
            "1" => "Team 1",
            "2" => "Team 2",
            value => value,
        };
        let (_, member) = self
            .compiler
            .catalog
            .resolve_enum_member("EventTeam", &Locale::new("en-US"), spelling)
            .ok_or_else(|| {
                self.unsupported(
                    format!("unknown EventTeam filter '{spelling}'"),
                    argument.span.or(annotation.span),
                )
            })?;
        match member.as_str() {
            "ALL" => Ok(EventTeam::All),
            "TEAM_1" => Ok(EventTeam::Team1),
            "TEAM_2" => Ok(EventTeam::Team2),
            _ => Err(self.unsupported(
                format!("catalog EventTeam member '{member}' is not supported by canonical WIR"),
                argument.span.or(annotation.span),
            )),
        }
    }

    fn lower_event_target(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<EventTarget, IntegrationError> {
        let mut filters = Vec::new();
        for name in ["Slot", "Hero"] {
            let matches = annotations
                .iter()
                .filter(|annotation| annotation.name == name)
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(self.unsupported(
                    format!("an event cannot have multiple @{name} filters"),
                    matches[1].span.or(matches[0].span),
                ));
            }
            filters.extend(matches);
        }
        if filters.len() > 1 {
            return Err(self.unsupported(
                "an event cannot combine @Slot and @Hero filters",
                filters[1].span.or(filters[0].span),
            ));
        }
        let Some(annotation) = filters.first() else {
            return Ok(EventTarget::All);
        };
        let argument = annotation.args.first().ok_or_else(|| {
            self.unsupported(
                format!("@{} requires one filter value", annotation.name),
                annotation.span,
            )
        })?;
        if annotation.args.len() != 1 {
            return Err(self.unsupported(
                format!("@{} requires exactly one filter value", annotation.name),
                annotation.span,
            ));
        }
        let spelling = if annotation.name == "Slot" {
            match argument.text.as_str() {
                value if value.parse::<u8>().is_ok() => {
                    format!("Slot {}", value.parse::<u8>().unwrap_or_default())
                }
                value => value.to_string(),
            }
        } else {
            argument.text.clone()
        };
        let domain = if annotation.name == "Slot" {
            "EventPlayer"
        } else {
            "Hero"
        };
        let locale = Locale::new("en-US");
        let catalog_spelling = match (domain, spelling.as_str()) {
            ("Hero", "mccree") => "CASSIDY",
            ("Hero", "hammond") => "WRECKING_BALL",
            ("Hero", "soldier") => "SOLDIER_76",
            ("Hero", "domina") => "JINYU",
            ("Hero", "dmon") => "D_MON",
            _ => spelling.as_str(),
        };
        let member = self
            .compiler
            .catalog
            .resolve_enum_member(domain, &locale, catalog_spelling)
            .map(|(_, member)| member)
            .or_else(|| {
                (domain == "Hero")
                    .then(|| {
                        self.compiler
                            .catalog
                            .enum_domain(domain)
                            .and_then(|domain| {
                                domain
                                    .members
                                    .iter()
                                    .find(|member| {
                                        member.member.eq_ignore_ascii_case(catalog_spelling)
                                            || member.spellings(&locale).iter().any(|candidate| {
                                                candidate.eq_ignore_ascii_case(catalog_spelling)
                                            })
                                            || member
                                                .member
                                                .chars()
                                                .filter(|c| c.is_ascii_alphanumeric())
                                                .collect::<String>()
                                                .eq_ignore_ascii_case(
                                                    &catalog_spelling
                                                        .chars()
                                                        .filter(|c| c.is_ascii_alphanumeric())
                                                        .collect::<String>(),
                                                )
                                    })
                                    .map(|member| member.member.clone())
                            })
                    })
                    .flatten()
            })
            .ok_or_else(|| {
                self.unsupported(
                    format!("unknown {domain} filter '{spelling}'"),
                    argument.span.or(annotation.span),
                )
            })?;
        if domain == "EventPlayer" {
            if member == "ALL" {
                Ok(EventTarget::All)
            } else if let Some(slot) = member.strip_prefix("SLOT_") {
                let slot = slot.parse::<u8>().map_err(|_| {
                    self.unsupported(
                        format!("catalog EventPlayer member '{member}' is not a slot"),
                        argument.span.or(annotation.span),
                    )
                })?;
                Ok(EventTarget::Slot(slot))
            } else {
                Err(self.unsupported(
                    format!(
                        "catalog EventPlayer member '{member}' is not supported by canonical WIR"
                    ),
                    argument.span.or(annotation.span),
                ))
            }
        } else {
            Ok(EventTarget::Hero(member))
        }
    }

    fn lower_actions(
        &mut self,
        statements: &[Stmt],
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        self.visible_labels.push(
            statements
                .iter()
                .filter_map(|statement| match statement {
                    Stmt::Label { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
        );
        let mut actions = Vec::new();
        let mut labels = HashMap::new();
        let mut gotos = Vec::new();
        let mut index = 0;
        while index < statements.len() {
            let statement = &statements[index];
            let optimization = self.optimization_state_at(statement.span());
            if optimization.enabled
                && optimization.for_size
                && optimization.for_size_aggressive
                && index + 1 == statements.len()
                && let Stmt::If {
                    branches,
                    r#else: None,
                    span,
                } = statement
                && branches.len() == 1
            {
                let branch = &branches[0];
                let is_not_condition = matches!(
                    &*branch.condition,
                    Expr::Unary { op, .. } if op == "not"
                );
                let is_comparison = matches!(
                    &*branch.condition,
                    Expr::Binary { op, .. }
                        if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=")
                );
                if (is_not_condition || is_comparison) && !branch.body.is_empty() {
                    let body = self.lower_actions(&branch.body, break_target)?;
                    if !body.is_empty() && (is_not_condition || branch.body.len() == 1) {
                        let condition = if is_not_condition {
                            let Expr::Unary { operand, .. } = &*branch.condition else {
                                unreachable!()
                            };
                            self.lower_value(operand)?
                        } else {
                            let condition = self.lower_value(&branch.condition)?;
                            self.push_call("not", vec![condition])
                        };
                        let distance = self.canonical_action_width(&body, *span)?;
                        let distance = self.push_number(distance as f64, &distance.to_string());
                        let skip = self.push_call_action("skipIf", &[condition, distance]);
                        self.mark_action_origins(std::slice::from_ref(&skip), *span);
                        actions.push(skip);
                        actions.extend(body);
                        index += 1;
                        continue;
                    }
                }
            }
            if let Some((condition, label, span)) = direct_conditional_goto(statement)
                && self
                    .visible_labels
                    .iter()
                    .any(|labels| labels.iter().any(|candidate| candidate == label))
            {
                let condition = self.lower_value(condition)?;
                let placeholder = self.push_number(0.0, "0");
                let skip = self.push_call_action("skipIf", &[condition, placeholder]);
                self.mark_action_origins(std::slice::from_ref(&skip), span);
                actions.push(skip);
                self.deferred_gotos.push((skip, label.to_string(), span, 1));
                index += 1;
                continue;
            }
            if let Some((condition, offset, span)) = direct_conditional_dynamic_goto(statement) {
                let condition = self.lower_value(condition)?;
                let offset = self.lower_value(offset)?;
                let skip = self.push_call_action("skipIf", &[condition, offset]);
                self.mark_action_origins(std::slice::from_ref(&skip), span);
                actions.push(skip);
                let goto = self.push_call_action("skip", &[offset]);
                self.mark_action_origins(std::slice::from_ref(&goto), span);
                actions.push(goto);
                actions.push(self.push_call_action("abort", &[]));
                index += 1;
                continue;
            }
            match statement {
                Stmt::Label { name, .. } => {
                    self.resolve_deferred_gotos(&actions, name, actions.len())?;
                    labels.insert(name.clone(), actions.len());
                }
                Stmt::Goto {
                    label,
                    offset,
                    rule_start,
                    span,
                } => {
                    if *rule_start {
                        let loop_action = self.push_call_action("loop", &[]);
                        self.mark_action_origins(std::slice::from_ref(&loop_action), *span);
                        actions.push(loop_action);
                        index += 1;
                        continue;
                    }
                    let placeholder = self.push_number(0.0, "0");
                    let action = self.push_call_action("skip", &[placeholder]);
                    self.mark_action_origins(std::slice::from_ref(&action), *span);
                    let position = actions.len();
                    actions.push(action);
                    gotos.push((
                        action,
                        position,
                        label.clone(),
                        offset.clone(),
                        span.map(Into::into),
                    ));
                }
                _ => actions.extend(self.lower_action(statement, break_target)?),
            }
            index += 1;
        }
        for (action, position, label, offset, span) in gotos {
            let distance = if let Some(offset) = offset {
                self.lower_value(&offset)?
            } else {
                let Some(label) = label else {
                    return Err(self.unsupported("goto is missing a label or offset", span));
                };
                let Some(&target) = labels.get(&label) else {
                    if self
                        .visible_labels
                        .iter()
                        .any(|labels| labels.contains(&label))
                    {
                        self.deferred_gotos.push((action, label, span, 0));
                        continue;
                    }
                    return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
                };
                if target < position {
                    return Err(self
                        .unsupported("backward goto is not representable in canonical WIR", span));
                }
                let width = self.canonical_action_width(&actions[position + 1..target], span)?;
                self.push_number(width as f64, &width.to_string())
            };
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("goto placeholder must be a call action")
            };
            args[0] = distance;
        }
        if self.visible_labels.len() == 1 && !self.deferred_gotos.is_empty() {
            let (_, label, span, _) = self.deferred_gotos.remove(0);
            return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
        }
        self.visible_labels.pop();
        Ok(actions)
    }

    /// `if condition: return` and `if condition: loop()` lower to a single
    /// conditional action; with optimization a constant condition removes the
    /// condition entirely.
    fn lower_terminal_if(
        &mut self,
        branch: &hir::types::IfBranch,
        span: Option<HirSpan>,
    ) -> Result<Option<Vec<ActionId>>, IntegrationError> {
        let [child] = branch.body.as_slice() else {
            return Ok(None);
        };
        let is_loop_call = matches!(
            child,
            Stmt::Expr { expr, .. }
                if matches!(expr.as_ref(), Expr::Call { name, args, .. } if name == "loop" && args.is_empty())
        );
        let (unconditional, conditional, on_true, on_false) = match child {
            Stmt::Return { .. } => (
                "abort",
                "abortIf",
                "__abortIfConditionIsTrue__",
                "__abortIfConditionIsFalse__",
            ),
            Stmt::Goto {
                rule_start: true, ..
            } => (
                "loop",
                "loopIf",
                "loopIfConditionIsTrue",
                "__loopIfConditionIsFalse__",
            ),
            _ if is_loop_call => (
                "loop",
                "loopIf",
                "loopIfConditionIsTrue",
                "__loopIfConditionIsFalse__",
            ),
            _ => return Ok(None),
        };
        let is_rule_condition =
            |expr: &Expr| matches!(expr, Expr::Call { name, .. } if name == "ruleCondition");
        let rule_condition = match branch.condition.as_ref() {
            condition if is_rule_condition(condition) => Some(on_true),
            Expr::Unary { op, operand, .. }
                if op == "not" && is_rule_condition(operand.as_ref()) =>
            {
                Some(on_false)
            }
            _ => None,
        };
        if let Some(name) = rule_condition {
            return Ok(Some(vec![self.push_call_action(name, &[])]));
        }
        let optimization = self.optimization_state_at(span.as_ref());
        if !optimization.enabled {
            return Ok(None);
        }
        let condition = self.lower_value(&branch.condition)?;
        let materialized = self.materialize_value(condition);
        let operators = OperatorOptimizer::new(self.compiler, optimization.strict);
        let action = match operators.constant_truth(&materialized) {
            Some(false) => return Ok(Some(Vec::new())),
            Some(true) => self.push_call_action(unconditional, &[]),
            None => self.push_call_action(conditional, &[condition]),
        };
        self.mark_action_origins(std::slice::from_ref(&action), span);
        Ok(Some(vec![action]))
    }

    fn resolve_deferred_gotos(
        &mut self,
        actions: &[ActionId],
        label: &str,
        target: usize,
    ) -> Result<(), IntegrationError> {
        let deferred = std::mem::take(&mut self.deferred_gotos);
        let mut remaining = Vec::new();
        for (action, deferred_label, span, argument) in deferred {
            if deferred_label != label {
                remaining.push((action, deferred_label, span, argument));
                continue;
            }
            let Some(position) = actions.iter().position(|candidate| *candidate == action) else {
                remaining.push((action, deferred_label, span, argument));
                continue;
            };
            if target < position {
                return Err(
                    self.unsupported("backward goto is not representable in canonical WIR", span)
                );
            }
            // A deferred goto may sit inside a structured action, so this
            // slice is not necessarily a standalone valid action sequence.
            // The flat lowering stream has one action id per native action,
            // including the structural markers that the jump must cross.
            let width = actions[position + 1..target].len();
            let distance = self.push_number(width as f64, &width.to_string());
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("deferred goto placeholder must be a call action")
            };
            args[argument] = distance;
        }
        self.deferred_gotos = remaining;
        Ok(())
    }

    fn lower_action(
        &mut self,
        stmt: &Stmt,
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let result = match stmt {
            Stmt::Pass { .. } => Ok(Vec::new()),
            Stmt::Assign {
                target,
                value,
                span,
            } => self.lower_assign(target, value, *span).map(|action| vec![action]),
            Stmt::If {
                branches,
                r#else,
                span,
            } => {
                if let ([branch], None) = (branches.as_slice(), r#else)
                    && let Some(actions) = self.lower_terminal_if(branch, *span)?
                {
                    return Ok(actions);
                }
                let branches = branches
                    .iter()
                    .map(|branch| {
                        Ok((
                            self.lower_value(&branch.condition)?,
                            self.lower_actions(&branch.body, break_target)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?;
                let else_body = r#else
                    .as_ref()
                    .map(|body| self.lower_actions(body, break_target))
                    .transpose()?;
                Ok(self.push_if_actions(branches, else_body))
            }
            Stmt::For {
                variable,
                iterable,
                body,
                span: _,
            } => {
                let (start, stop, step) = self.lower_range(iterable)?;
                let body = self.lower_loop_body(body)?;
                match variable.as_ref() {
                    Expr::GlobalVar {
                        name,
                        span: target_span,
                    } => {
                        let variable_id = *self.globals.get(name).ok_or_else(|| {
                            self.unsupported(
                                format!("unknown global variable '{name}'"),
                                *target_span,
                            )
                        })?;
                        Ok(self.push_for_global_actions(variable_id, start, stop, step, body))
                    }
                    Expr::PlayerVar {
                        player,
                        name,
                        span: target_span,
                        ..
                    } => {
                        let variable_id = *self.players.get(name).ok_or_else(|| {
                            self.unsupported(
                                format!("unknown player variable '{name}'"),
                                *target_span,
                            )
                        })?;
                        let player = self.lower_value(player)?;
                        Ok(self.push_for_player_actions(
                            player, variable_id, start, stop, step, body,
                        ))
                    }
                    _ => Err(self.unsupported(
                        "range loops require a global- or player-variable binder in canonical WIR",
                        variable.span().copied(),
                    )),
                }
            }
            Stmt::While {
                condition,
                body,
                span: _,
            } => {
                let condition = self.lower_value(condition)?;
                let body = self.lower_loop_body(body)?;
                Ok(self.push_while_actions(condition, body))
            }
            Stmt::DoWhile {
                condition,
                body,
                span: _,
            } => {
                let body = self.lower_do_while_body(body)?;
                let condition = self.lower_value(condition)?;
                let loop_if = self.push_call_action("loopIf", &[condition]);
                // OverPy's pinned lowering expands do/while into its body
                // followed by the canonical Loop If action.
                let mut actions = body;
                actions.push(loop_if);
                Ok(actions)
            }
            Stmt::Switch {
                value,
                arms,
                span,
            } => self.lower_switch(value, arms, *span, break_target, None, false),
            Stmt::Delete { target, span } => self.lower_delete(target, *span).map(|action| vec![action]),
            Stmt::Continue { span } => Err(self.unsupported(
                "continue statements are only lowered while constructing a loop body",
                *span,
            )),
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => {
                if *rule_start {
                    Ok(vec![self.push_call_action("loop", &[])])
                } else if label.is_none() {
                    let offset = offset.as_ref().ok_or_else(|| {
                        self.unsupported("goto is missing a label or offset", *span)
                    })?;
                    let offset = self.lower_value(offset)?;
                    Ok(vec![self.push_call_action("skip", &[offset])])
                } else {
                    Err(self.unsupported(
                        "goto statements are not representable in canonical WIR",
                        *span,
                    ))
                }
            }
            Stmt::Label { span, .. } => Err(self.unsupported(
                "labels are not representable in canonical WIR",
                *span,
            )),
            Stmt::Break { span } => match break_target {
                Some(BreakTarget::Loop) => Ok(vec![self.push_call_action("break", &[])]),
                Some(BreakTarget::DoWhile) => Err(self.unsupported(
                    "break inside a do-while must be a direct statement or a single conditional break",
                    *span,
                )),
                Some(BreakTarget::Switch) => Ok(vec![self.push_action(Action::Else)]),
                None => Err(self.unsupported(
                    "break has no enclosing canonical loop or switch",
                    *span,
                )),
            },
            Stmt::Return { span: _ } => Ok(vec![self.push_call_action("abort", &[])]),
            Stmt::Expr { expr, span } => match expr.as_ref() {
                Expr::Call {
                    name,
                    args,
                    debug_source,
                    ..
                } => {
                    if name == "disableInspector" && args.is_empty() {
                        Ok(vec![self.push_call_action("disableInspector", &[])])
                    } else if name == "pass" && args.is_empty() {
                        Ok(Vec::new())
                    } else if name == "debug" && args.len() == 1 {
                        Ok(vec![self.lower_debug(&args[0], *span, debug_source.as_deref())?])
                    } else if name == "print" && args.len() == 1 {
                        Ok(vec![self.lower_print(&args[0], *span)?])
                    } else if name == "createCasedProgressBarIwt" {
                        self.lower_cased_progress_bar(args, *span)
                    } else {
                        self.lower_action_call(name, args, *span).map(|action| vec![action])
                    }
                }
                Expr::ReceiverCall {
                    receiver,
                    name,
                    args,
                    span: call_span,
                } => self
                    .lower_receiver_action_call(receiver, name, args, *call_span)
                    .map(|action| vec![action]),
                _ => Err(self.unsupported(
                    "only action calls are currently representable as expression statements in canonical WIR",
                    *span,
                )),
            },
            Stmt::CallSubroutine { name, span } => {
                let subroutine = *self.subroutines.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown subroutine '{name}'"), *span)
                })?;
                Ok(vec![self.push_action(Action::CallSubroutine {
                    subroutine: self.subroutine_names[subroutine].clone(),
                })])
            }
        };
        if let Ok(actions) = &result {
            self.mark_action_origins(actions, stmt.span().copied());
            self.mark_statement_argument_origins(stmt, actions);
        }
        result
    }

    fn mark_statement_argument_origins(&mut self, statement: &Stmt, actions: &[ActionId]) {
        match statement {
            Stmt::Assign { target, value, .. } => {
                let (target_span, index_span) = match &**target {
                    Expr::Index { array, index, .. } => {
                        (array.span().copied(), index.span().copied())
                    }
                    _ => (target.span().copied(), None),
                };
                let value_span = value.span().copied();
                let modified_value_span = match &**value {
                    Expr::Binary { right, .. } => right.span().copied(),
                    _ => value_span,
                };
                for action in actions {
                    let spans = match self.actions.get(*action) {
                        Some(Action::SetGlobalVariable { .. }) => vec![value_span],
                        Some(Action::ModifyGlobalVariable { .. }) => vec![modified_value_span],
                        Some(Action::SetPlayerVariable { .. }) => {
                            let player_span = match &**target {
                                Expr::PlayerVar { player, .. } => player.span().copied(),
                                _ => None,
                            };
                            vec![player_span, value_span]
                        }
                        Some(Action::ModifyPlayerVariable { .. }) => {
                            let player_span = match &**target {
                                Expr::PlayerVar { player, .. } => player.span().copied(),
                                _ => None,
                            };
                            vec![player_span, modified_value_span]
                        }
                        Some(Action::Call { name, .. })
                            if name == "setGlobalVariableAtIndex"
                                || name == "setPlayerVariableAtIndex" =>
                        {
                            vec![target_span, index_span, value_span]
                        }
                        Some(Action::Call { name, .. })
                            if name == "modifyGlobalVariableAtIndex"
                                || name == "modifyPlayerVariableAtIndex" =>
                        {
                            vec![target_span, index_span, None, modified_value_span]
                        }
                        _ => continue,
                    };
                    self.mark_action_argument_origins(*action, spans);
                }
            }
            Stmt::If { branches, .. } => {
                let mut depth = 0usize;
                let mut branch = 0usize;
                for action in actions {
                    match self.actions.get(*action) {
                        Some(Action::If { .. }) => {
                            if depth == 0 {
                                self.mark_action_argument_origins(
                                    *action,
                                    [branches
                                        .first()
                                        .and_then(|branch| branch.condition.span().copied())],
                                );
                            }
                            depth += 1;
                        }
                        Some(Action::ElseIf { .. }) if depth == 1 => {
                            branch += 1;
                            self.mark_action_argument_origins(
                                *action,
                                [branches
                                    .get(branch)
                                    .and_then(|branch| branch.condition.span().copied())],
                            );
                        }
                        Some(Action::End) => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
            }
            Stmt::While { condition, .. } => {
                if let Some(action) = actions.first() {
                    if matches!(self.actions.get(*action), Some(Action::While { .. })) {
                        self.mark_action_argument_origins(*action, [condition.span().copied()]);
                    }
                }
            }
            Stmt::For {
                variable, iterable, ..
            } => {
                let range_spans = match &**iterable {
                    Expr::Call { args, .. } => match args.as_slice() {
                        [stop] => vec![None, stop.span().copied(), None],
                        [start, stop] => {
                            vec![start.span().copied(), stop.span().copied(), None]
                        }
                        [start, stop, step] => vec![
                            start.span().copied(),
                            stop.span().copied(),
                            step.span().copied(),
                        ],
                        _ => return,
                    },
                    _ => return,
                };
                let spans = match &**variable {
                    Expr::PlayerVar { player, .. } => std::iter::once(player.span().copied())
                        .chain(range_spans)
                        .collect::<Vec<_>>(),
                    _ => range_spans,
                };
                if let Some(action) = actions.first() {
                    if matches!(
                        self.actions.get(*action),
                        Some(Action::ForGlobalVariable { .. })
                            | Some(Action::ForPlayerVariable { .. })
                    ) {
                        self.mark_action_argument_origins(*action, spans);
                    }
                }
            }
            Stmt::DoWhile { condition, .. } => {
                if let Some(action) = actions.last() {
                    if matches!(
                        self.actions.get(*action),
                        Some(Action::Call { name, .. }) if name == "loopIf"
                    ) {
                        self.mark_action_argument_origins(*action, [condition.span().copied()]);
                    }
                }
            }
            Stmt::Delete { target, .. } => {
                let mut indices = Vec::new();
                let _ = indexed_target_parts(target, &mut indices);
                indices.reverse();
                for action in actions {
                    let spans = match self.actions.get(*action) {
                        Some(Action::SetGlobalVariable { .. })
                        | Some(Action::ModifyGlobalVariable { .. }) => {
                            vec![target.span().copied()]
                        }
                        Some(Action::SetPlayerVariable { .. })
                        | Some(Action::ModifyPlayerVariable { .. }) => {
                            vec![None, target.span().copied()]
                        }
                        Some(Action::Call { name, .. })
                            if name == "setGlobalVariableAtIndex"
                                || name == "setPlayerVariableAtIndex" =>
                        {
                            vec![
                                target.span().copied(),
                                indices.first().and_then(|index| index.span().copied()),
                                target.span().copied(),
                            ]
                        }
                        Some(Action::Call { name, .. })
                            if name == "modifyGlobalVariableAtIndex"
                                || name == "modifyPlayerVariableAtIndex" =>
                        {
                            vec![
                                target.span().copied(),
                                indices.first().and_then(|index| index.span().copied()),
                                None,
                                indices.last().and_then(|index| index.span().copied()),
                            ]
                        }
                        _ => continue,
                    };
                    self.mark_action_argument_origins(*action, spans);
                }
            }
            _ => {}
        }
    }

    fn lower_loop_body(&mut self, statements: &[Stmt]) -> Result<Vec<ActionId>, IntegrationError> {
        self.lower_loop_sequence(statements, &[], 0)
    }

    fn lower_loop_sequence(
        &mut self,
        statements: &[Stmt],
        after: &[ActionId],
        structural_after: usize,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        self.lower_loop_sequence_with_break_target(
            statements,
            after,
            structural_after,
            BreakTarget::Loop,
        )
    }

    fn lower_loop_sequence_with_break_target(
        &mut self,
        statements: &[Stmt],
        after: &[ActionId],
        structural_after: usize,
        break_target: BreakTarget,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        let mut index = 0;
        while index < statements.len() {
            let statement = &statements[index];
            let tail = &statements[index + 1..];
            if let Some(conditions) = pure_continue_conditions(statement) {
                let tail = self.lower_loop_sequence_with_break_target(
                    tail,
                    after,
                    structural_after,
                    break_target,
                )?;
                let distance = self.canonical_action_width(&tail, statement.span().copied())?
                    + structural_after
                    + self.canonical_action_width(after, statement.span().copied())?;
                if distance > 0 {
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some((first, rest)) = conditions.split_first() {
                        let mut condition = self.lower_value(first)?;
                        for expression in rest {
                            let right = self.lower_value(expression)?;
                            condition = self.push_call("and", vec![condition, right]);
                        }
                        args.push(condition);
                    }
                    let distance = self.push_number(distance as f64, &distance.to_string());
                    args.push(distance);
                    let skip = self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    );
                    self.mark_action_origins(
                        std::slice::from_ref(&skip),
                        statement.span().copied(),
                    );
                    actions.push(skip);
                }
                actions.extend(tail);
                return Ok(actions);
            }
            if contains_loop_continue(statement) {
                let tail = self.lower_loop_sequence_with_break_target(
                    tail,
                    after,
                    structural_after,
                    break_target,
                )?;
                let mut continuation_after = tail.clone();
                continuation_after.extend_from_slice(after);
                let lowered = if let Stmt::Switch { value, arms, span } = statement {
                    self.lower_switch(
                        value,
                        arms,
                        *span,
                        Some(break_target),
                        Some((&continuation_after, structural_after)),
                        false,
                    )?
                } else {
                    self.lower_if_with_loop_continue(
                        statement,
                        &continuation_after,
                        structural_after,
                        break_target,
                    )?
                };
                self.mark_action_origins(&lowered, statement.span().copied());
                actions.extend(lowered);
                actions.extend(tail);
                return Ok(actions);
            }
            if let Some((conditions, label)) = pure_goto_conditions(statement) {
                if let Some(target) = statements[index + 1..]
                    .iter()
                    .position(
                        |candidate| matches!(candidate, Stmt::Label { name, .. } if name == label),
                    )
                    .map(|offset| index + 1 + offset)
                {
                    let middle = self.lower_loop_sequence_with_break_target(
                        &statements[index + 1..target],
                        after,
                        structural_after,
                        break_target,
                    )?;
                    let suffix = self.lower_loop_sequence_with_break_target(
                        &statements[target + 1..],
                        after,
                        structural_after,
                        break_target,
                    )?;
                    let distance =
                        self.canonical_action_width(&middle, statement.span().copied())?;
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some((first, rest)) = conditions.split_first() {
                        let mut condition = self.lower_value(first)?;
                        for expression in rest {
                            let right = self.lower_value(expression)?;
                            condition = self.push_call("and", vec![condition, right]);
                        }
                        args.push(condition);
                    }
                    args.push(self.push_number(distance as f64, &distance.to_string()));
                    let skip = self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    );
                    self.mark_action_origins(
                        std::slice::from_ref(&skip),
                        statement.span().copied(),
                    );
                    actions.push(skip);
                    actions.extend(middle);
                    actions.extend(suffix);
                    return Ok(actions);
                }
            }
            if let Some((conditions, label)) = pure_goto_conditions(statement) {
                let local_label = statements.iter().any(
                    |candidate| matches!(candidate, Stmt::Label { name, .. } if name == label),
                );
                let outer_label = self
                    .visible_labels
                    .last()
                    .is_some_and(|labels| labels.contains(label));
                if !local_label && outer_label {
                    let _span = statement.span().copied();
                    let mut condition = None;
                    for expression in conditions {
                        let value = self.lower_value(expression)?;
                        condition = Some(match condition {
                            Some(left) => self.push_call("and", vec![left, value]),
                            None => value,
                        });
                    }
                    let break_action = self.push_call_action("break", &[]);
                    self.mark_action_origins(
                        std::slice::from_ref(&break_action),
                        statement.span().copied(),
                    );
                    if let Some(condition) = condition {
                        let lowered =
                            self.push_if_actions(vec![(condition, vec![break_action])], None);
                        self.mark_action_origins(&lowered, statement.span().copied());
                        actions.extend(lowered);
                    } else {
                        actions.push(break_action);
                    }
                    index += 1;
                    continue;
                }
            }
            if matches!(statement, Stmt::Label { .. }) {
                index += 1;
                continue;
            }
            actions.extend(self.lower_action(statement, Some(break_target))?);
            index += 1;
        }
        Ok(actions)
    }

    fn lower_if_with_loop_continue(
        &mut self,
        statement: &Stmt,
        after: &[ActionId],
        structural_after: usize,
        break_target: BreakTarget,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let Stmt::If {
            branches,
            r#else,
            span: _,
        } = statement
        else {
            unreachable!("continue-containing loop statement must be an if")
        };
        let mut lowered_branches = Vec::with_capacity(branches.len());
        let mut suffix = after.to_vec();
        let mut suffix_structural = structural_after + 1;
        let mut lowered_else = None;
        if let Some(body) = r#else {
            let body = self.lower_loop_sequence_with_break_target(
                body,
                after,
                suffix_structural,
                break_target,
            )?;
            suffix.splice(0..0, body.iter().copied());
            suffix_structural += 1;
            lowered_else = Some(body);
        }
        for index in (0..branches.len()).rev() {
            let body = self.lower_loop_sequence_with_break_target(
                &branches[index].body,
                &suffix,
                suffix_structural,
                break_target,
            )?;
            suffix_structural += 1;
            suffix.splice(0..0, body.iter().copied());
            lowered_branches.push(body);
        }
        lowered_branches.reverse();
        let mut branch_actions = Vec::with_capacity(branches.len());
        for (branch, body) in branches.iter().zip(lowered_branches) {
            branch_actions.push((self.lower_value(&branch.condition)?, body));
        }
        Ok(self.push_if_actions(branch_actions, lowered_else))
    }

    fn lower_do_while_body(
        &mut self,
        statements: &[Stmt],
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        for (index, statement) in statements.iter().enumerate() {
            if let Some(conditions) = pure_continue_conditions(statement) {
                let tail = self.lower_do_while_body(&statements[index + 1..])?;
                let mut condition = None;
                for expression in conditions {
                    let value = self.lower_value(expression)?;
                    condition = Some(match condition {
                        Some(left) => self.push_call("and", vec![left, value]),
                        None => value,
                    });
                }
                let action = if let Some(condition) = condition {
                    self.push_call_action("loopIf", &[condition])
                } else {
                    self.push_call_action("loop", &[])
                };
                self.mark_action_origins(std::slice::from_ref(&action), statement.span().copied());
                actions.push(action);
                actions.extend(tail);
                return Ok(actions);
            }
            if contains_loop_continue(statement) {
                if let Stmt::Switch { value, arms, span } = statement {
                    let lowered = self.lower_switch(
                        value,
                        arms,
                        *span,
                        Some(BreakTarget::DoWhile),
                        None,
                        true,
                    )?;
                    self.mark_action_origins(&lowered, statement.span().copied());
                    actions.extend(lowered);
                    continue;
                }
                let Stmt::If {
                    branches, r#else, ..
                } = statement
                else {
                    unreachable!("continue-containing do-while statement must be an if")
                };
                let branches = branches
                    .iter()
                    .map(|branch| {
                        Ok((
                            self.lower_value(&branch.condition)?,
                            self.lower_do_while_body(&branch.body)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?;
                let else_body = r#else
                    .as_ref()
                    .map(|body| self.lower_do_while_body(body))
                    .transpose()?;
                let lowered = self.push_if_actions(branches, else_body);
                self.mark_action_origins(&lowered, statement.span().copied());
                actions.extend(lowered);
                continue;
            }
            let direct_break = matches!(statement, Stmt::Break { .. });
            let conditional_break = match statement {
                Stmt::If {
                    branches,
                    r#else: None,
                    ..
                } if branches.len() == 1 => {
                    matches!(branches[0].body.as_slice(), [Stmt::Break { .. }])
                }
                _ => false,
            };

            if direct_break || conditional_break {
                let tail = self.lower_do_while_body(&statements[index + 1..])?;
                let distance = self.canonical_action_width(&tail, statement.span().copied())? + 1;
                let (name, args, _span) = if let Stmt::Break { span } = statement {
                    ("skip", Vec::new(), *span)
                } else if let Stmt::If { branches, span, .. } = statement {
                    (
                        "skipIf",
                        vec![self.lower_value(&branches[0].condition)?],
                        *span,
                    )
                } else {
                    unreachable!("break shape was checked above")
                };
                let distance = self.push_number(distance as f64, &distance.to_string());
                let mut args = args;
                args.push(distance);
                let skip = self.push_call_action(name, &args);
                self.mark_action_origins(std::slice::from_ref(&skip), statement.span().copied());
                actions.push(skip);
                actions.extend(tail);
                return Ok(actions);
            }

            actions.extend(self.lower_action(statement, Some(BreakTarget::DoWhile))?);
        }
        Ok(actions)
    }

    fn lower_range(
        &mut self,
        iterable: &Expr,
    ) -> Result<(ValueId, ValueId, ValueId), IntegrationError> {
        let Expr::Call { name, args, .. } = iterable else {
            return Err(self.unsupported(
                "range loop iterable must be a range(...) call",
                iterable.span().copied(),
            ));
        };
        if name != "range" || !(1..=3).contains(&args.len()) {
            return Err(self.unsupported(
                "range loop requires one to three arguments",
                iterable.span().copied(),
            ));
        }
        let span = iterable.span().copied();
        let number = |this: &mut Self, value: f64| -> Result<ValueId, IntegrationError> {
            let _ = span;
            Ok(this.push_number(value, &value.to_string()))
        };
        match args.as_slice() {
            [stop] => Ok((
                number(self, 0.0)?,
                self.lower_value(stop)?,
                number(self, 1.0)?,
            )),
            [start, stop] => Ok((
                self.lower_value(start)?,
                self.lower_value(stop)?,
                number(self, 1.0)?,
            )),
            [start, stop, step] => Ok((
                self.lower_value(start)?,
                self.lower_value(stop)?,
                self.lower_value(step)?,
            )),
            _ => unreachable!("range arity checked above"),
        }
    }

    fn lower_switch(
        &mut self,
        value: &Expr,
        arms: &[SwitchArm],
        span: Option<HirSpan>,
        break_target: Option<BreakTarget>,
        loop_continue: Option<(&[ActionId], usize)>,
        do_while_continue: bool,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        if break_target.is_none()
            && arms.iter().all(|arm| match arm {
                SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                    switch_body_is_noop(body)
                }
            })
        {
            return Ok(Vec::new());
        }
        let selector = self.lower_value(value)?;
        let mut case_values = Vec::new();
        let mut lowered_arms = Vec::with_capacity(arms.len());
        let mut has_default = false;
        let mut legacy_case_offsets = Vec::new();
        let mut legacy_offset = 0;
        let mut legacy_default_offset = None;

        let mut reverse_bodies = (loop_continue.is_some() && !do_while_continue).then(|| {
            (0..arms.len())
                .map(|_| None)
                .collect::<Vec<Option<LoweredSwitchBody>>>()
        });
        if let Some((outer_after, structural_after)) = loop_continue {
            let mut future = Vec::new();
            for index in (0..arms.len()).rev() {
                let body = match &arms[index] {
                    SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => body,
                };
                let mut after = future.clone();
                after.extend_from_slice(outer_after);
                let lowered = self.lower_switch_body(
                    body,
                    Some((&after, structural_after)),
                    do_while_continue,
                )?;
                let mut next_future = lowered.0.clone();
                next_future.extend_from_slice(&future);
                future = next_future;
                reverse_bodies.as_mut().unwrap()[index] = Some(lowered);
            }
        }
        for (index, arm) in arms.iter().enumerate() {
            let (value, (body, break_at)) = match arm {
                SwitchArm::Case { value, body, .. } => {
                    case_values.push(self.lower_value(value)?);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (Some(value), lowered)
                }
                SwitchArm::Default { body, span } => {
                    if has_default {
                        return Err(
                            self.unsupported("a switch may contain at most one default arm", *span)
                        );
                    }
                    has_default = true;
                    legacy_default_offset = Some(legacy_offset);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (None, lowered)
                }
            };
            if value.is_some() {
                legacy_case_offsets.push(legacy_offset);
            }
            legacy_offset +=
                self.canonical_action_width(&body, span)? + usize::from(break_at.is_some());
            lowered_arms.push((value.map(Box::as_ref), body, break_at));
        }

        let break_arms: Vec<_> = lowered_arms
            .iter()
            .enumerate()
            .filter_map(|(index, (_, _, break_at))| break_at.map(|break_at| (index, break_at)))
            .collect();
        let first_break = break_arms.first().copied();
        let has_later_reachable_actions =
            first_break.is_some_and(|(break_index, (break_at, _))| {
                lowered_arms[break_index].1.len() > break_at
                    || lowered_arms
                        .iter()
                        .skip(break_index + 1)
                        .any(|(_, body, _)| !body.is_empty())
            });
        let use_shared_exit = break_arms.len() > 1 && has_later_reachable_actions;

        let case_values = self.lower_array(case_values, span)?;
        let _ = span;
        if !use_shared_exit {
            let default_offset = legacy_default_offset.unwrap_or(legacy_offset);
            let offset_values = std::iter::once(default_offset)
                .chain(legacy_case_offsets)
                .map(|value| self.push_number(value as f64, &value.to_string()))
                .collect();
            let offsets = self.lower_array(offset_values, span)?;
            let skip = self.lower_switch_selector(selector, case_values, offsets, span)?;
            let true_value = self.push_value(Value::Bool(true));
            let mut branch_body = vec![skip];
            let else_body = if let Some((break_index, (break_at, _))) = first_break {
                for (index, (_, body, _)) in lowered_arms.iter().enumerate() {
                    if index < break_index {
                        branch_body.extend(body.iter().copied());
                    } else if index == break_index {
                        branch_body.extend(body[..break_at].iter().copied());
                    }
                }
                let mut tail = Vec::new();
                tail.extend(lowered_arms[break_index].1[break_at..].iter().copied());
                for (_, body, _) in lowered_arms.iter().skip(break_index + 1) {
                    tail.extend(body.iter().copied());
                }
                Some(tail)
            } else {
                for (_, body, _) in &lowered_arms {
                    branch_body.extend(body.iter().copied());
                }
                None
            };
            let result = self.push_if_actions(vec![(true_value, branch_body)], else_body);
            return Ok(result);
        }

        let offsets = self.push_value(Value::Array(Vec::new()));
        let skip = self.lower_switch_selector(selector, case_values, offsets, span)?;
        let mut arm_offsets = vec![None; lowered_arms.len()];
        let (switch, switch_end) =
            self.lower_switch_level(&lowered_arms, 0, Some(skip), 0, &mut arm_offsets, span)?;

        let default_offset = lowered_arms
            .iter()
            .enumerate()
            .find_map(|(index, (value, _, _))| value.is_none().then(|| arm_offsets[index].unwrap()))
            .unwrap_or(switch_end);
        let offset_values = std::iter::once(default_offset)
            .chain(
                lowered_arms
                    .iter()
                    .enumerate()
                    .filter(|(_, (value, _, _))| value.is_some())
                    .map(|(index, _)| arm_offsets[index].unwrap()),
            )
            .map(|value| self.push_number(value as f64, &value.to_string()))
            .collect();
        let offset_values = self.lower_array(offset_values, span)?;
        let offset_value = self.value(offset_values).clone();
        let Some(node) = self.values.get_mut(offsets) else {
            unreachable!("switch offset placeholder must exist")
        };
        *node = offset_value;

        let Some(Action::Call { args, .. }) = self.actions.get_mut(skip) else {
            unreachable!("switch selector must be a call action")
        };
        let Some(selector_id) = args.first().copied() else {
            unreachable!("switch selector condition must be a value call")
        };
        let Some(Value::Call { args, .. }) = self.values.get_mut(selector_id) else {
            unreachable!("switch selector condition must be a value call")
        };
        args[0] = offsets;

        Ok(switch)
    }

    fn lower_switch_selector(
        &mut self,
        selector: ValueId,
        case_values: ValueId,
        offsets: ValueId,
        _span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let one = self.push_number(1.0, "1");
        let index = self.push_call("indexOfArrayValue", vec![case_values, selector]);
        let case_offset = self.push_call("add", vec![one, index]);
        let skip_condition = self.push_call("valueInArray", vec![offsets, case_offset]);
        Ok(self.push_call_action("skip", &[skip_condition]))
    }

    fn lower_switch_level(
        &mut self,
        arms: &[LoweredSwitchArm<'_>],
        start: usize,
        selector_skip: Option<ActionId>,
        level_offset: usize,
        arm_offsets: &mut [Option<usize>],
        span: Option<HirSpan>,
    ) -> Result<(Vec<ActionId>, usize), IntegrationError> {
        let break_index = (start..arms.len())
            .find(|index| arms[*index].2.is_some())
            .expect("switch level must contain a break");
        let mut branch_body = Vec::new();
        if let Some(selector_skip) = selector_skip {
            branch_body.push(selector_skip);
        }
        let mut branch_offset = 0;
        for index in start..=break_index {
            arm_offsets[index] = Some(if selector_skip.is_some() {
                level_offset + branch_offset
            } else if index == start {
                level_offset
            } else {
                level_offset + 1 + branch_offset
            });
            let (_, body, break_at) = &arms[index];
            let body = if index == break_index {
                &body[..break_at.as_ref().unwrap().0]
            } else {
                body.as_slice()
            };
            branch_offset += self.canonical_action_width(body, span)?;
            branch_body.extend(body.iter().copied());
        }
        let branch_width = self.canonical_action_width(&branch_body, span)?;
        let (_, break_body, Some((break_at, _))) = &arms[break_index] else {
            unreachable!("break index must point to a switch break")
        };
        let mut else_body = break_body[*break_at..].to_vec();
        let tail_width = self.canonical_action_width(&else_body, span)?;
        let else_content_start = if selector_skip.is_some() {
            level_offset + branch_width + tail_width
        } else {
            level_offset + branch_width + tail_width + 2
        };
        let has_next_break = (break_index + 1..arms.len()).any(|index| arms[index].2.is_some());
        let end_offset = if has_next_break {
            let (child, child_end) = self.lower_switch_level(
                arms,
                break_index + 1,
                None,
                else_content_start,
                arm_offsets,
                span,
            )?;
            else_body.extend(child);
            child_end
        } else {
            let mut offset = else_content_start;
            for index in break_index + 1..arms.len() {
                arm_offsets[index] = Some(offset);
                let (_, body, _) = &arms[index];
                offset += self.canonical_action_width(body, span)?;
                else_body.extend(body.iter().copied());
            }
            offset
        };
        let true_value = self.push_value(Value::Bool(true));
        let switch = self.push_if_actions(vec![(true_value, branch_body)], Some(else_body));
        Ok((switch, end_offset))
    }

    fn lower_switch_body(
        &mut self,
        statements: &[Stmt],
        loop_continue: Option<(&[ActionId], usize)>,
        do_while_continue: bool,
    ) -> Result<LoweredSwitchBody, IntegrationError> {
        let mut actions = Vec::new();
        let break_index = statements
            .iter()
            .position(|statement| matches!(statement, Stmt::Break { .. }));
        let body_end = break_index.unwrap_or(statements.len());
        if do_while_continue {
            actions.extend(self.lower_do_while_body(&statements[..body_end])?);
        } else if let Some((after, structural_after)) = loop_continue {
            actions.extend(self.lower_loop_sequence_with_break_target(
                &statements[..body_end],
                after,
                structural_after + 1,
                BreakTarget::Switch,
            )?);
        } else {
            for statement in &statements[..body_end] {
                actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
        }
        let mut break_at = None;
        if let Some(index) = break_index {
            let Stmt::Break { span } = &statements[index] else {
                unreachable!("switch break index must point to a break")
            };
            if statements[index + 1..]
                .iter()
                .any(|statement| matches!(statement, Stmt::Break { .. }))
            {
                return Err(self.unsupported(
                    "multiple switch breaks in one arm require canonical switch targets",
                    *span,
                ));
            }
            break_at = Some((
                actions.len(),
                span.ok_or_else(|| {
                    self.unsupported("switch break is missing source provenance", None)
                })?,
            ));
            for statement in statements[index + 1..].iter() {
                actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
        }
        Ok((actions, break_at))
    }

    fn canonical_action_width(
        &self,
        actions: &[ActionId],
        fallback_span: Option<HirSpan>,
    ) -> Result<usize, IntegrationError> {
        let public_actions = self.public_actions(actions);
        let mut program = self.program.clone();
        program.settings = None;
        program.rules.push(workshop_rs::Rule {
            name: "action layout".to_string(),
            disabled: false,
            event: workshop_rs::Event::Global,
            conditions: Vec::new(),
            actions: public_actions.clone(),
        });
        workshop_rs::emitter::action_width(
            &program,
            self.compiler.catalog,
            &Locale::new("en-US"),
            &public_actions,
        )
        .map(|layout| layout.width)
        .map_err(|error| {
            let span = fallback_span;
            IntegrationError::new("workshop-action-layout", error.to_string(), span)
        })
    }

    fn lower_array(
        &mut self,
        elements: Vec<ValueId>,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let name = if elements.is_empty() {
            "emptyArray"
        } else {
            "array"
        };
        let elements = self.normalize_contextual_arguments(name, elements);
        let _ = span;
        Ok(self.push_value(Value::Call {
            name: name.to_string(),
            args: self.value_args(&elements),
        }))
    }

    fn lower_translation_helper(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<ValueId, IntegrationError> {
        let translated_white = translations
            .languages
            .iter()
            .map(|language| {
                let locale = match language.as_str() {
                    "de" => "de-DE",
                    "en" => "en-US",
                    "es" => "es-MX",
                    "es_es" => "es-ES",
                    "es_mx" => "es-MX",
                    "fr" => "fr-FR",
                    "it" => "it-IT",
                    "ja" => "ja-JP",
                    "ko" => "ko-KR",
                    "pl" => "pl-PL",
                    "pt" => "pt-BR",
                    "ru" => "ru-RU",
                    "th" => "th-TH",
                    "tr" => "tr-TR",
                    "zh" | "zh_cn" => "zh-CN",
                    "zh_tw" => "zh-TW",
                    _ => {
                        return Err(IntegrationError::new(
                            "translations-invalid",
                            format!("unsupported translation language '{language}'"),
                            translations.span,
                        ));
                    }
                };
                self.compiler
                    .catalog
                    .localized_enum_spelling(
                        "Color",
                        &workshop_rs::catalog::Locale::new(locale),
                        "WHITE",
                    )
                    .ok_or_else(|| {
                        IntegrationError::new(
                            "translations-invalid",
                            format!("unsupported translation locale '{locale}'"),
                            translations.span,
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("0");
        let text = self.push_value(Value::String(format!("\u{ec48}0{translated_white}")));
        let custom_string = self.push_call("customString", vec![text]);
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        Ok(self.push_call("stringSplit", vec![custom_string, separator]))
    }

    fn lower_translation(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Err(IntegrationError::new(
                "translations-invalid",
                format!("translation function '{name}' requires #!translations"),
                span,
            ));
        };
        let (context, target) = match args {
            [target] => (None, target),
            [Expr::String { value: context, .. }, target] => (Some(context.as_str()), target),
            _ => {
                return Err(IntegrationError::new(
                    "translations-invalid",
                    format!("translation function '{name}' expects one or two arguments"),
                    span,
                ));
            }
        };
        let (literal, format_args) = match target {
            Expr::String { value, .. } => (value.clone(), Vec::new()),
            Expr::Format { text, args, .. } => {
                let (text, args) = self.fold_format_constants(text, args);
                (text, args)
            }
            _ => {
                let target = self.lower_value(target)?;
                if name == "___" {
                    return Ok(target);
                }
                return Ok(self.select_translation(target));
            }
        };
        if format_args.len() > 16 {
            return Err(IntegrationError::new(
                "translations-invalid",
                "translated format strings support at most sixteen dynamic arguments",
                span,
            ));
        }
        let literal = literal.as_str();
        let msgid = literal.trim();
        if literal.contains('\u{ec48}') {
            return Err(IntegrationError::new(
                "translations-invalid",
                "translation strings must not contain the reserved translation separator",
                span,
            ));
        }
        if !self
            .translation_uses
            .iter()
            .any(|(existing_msgid, existing)| {
                existing_msgid == msgid && existing.as_deref() == context
            })
        {
            self.translation_uses
                .push((msgid.to_string(), context.map(str::to_string)));
        }
        let use_tl_err = !self.translation_player_options().2;
        let mut localized = translations
            .languages
            .iter()
            .map(|language| {
                translations
                    .entries
                    .iter()
                    .find(|entry| entry.msgid == msgid && entry.context.as_deref() == context)
                    .and_then(|entry| entry.translations.get(language))
                    .filter(|value| !value.is_empty())
                    .cloned()
                    .unwrap_or_else(|| literal.to_string())
            })
            .collect::<Vec<_>>();
        let tl_err_prefix = if use_tl_err {
            "\u{ff34}\u{ff2c}\u{ff25}\u{ff52}\u{ff52}\u{ec48}"
        } else {
            ""
        };
        let raw_string = format!("{tl_err_prefix}{}", localized.join("\u{ec48}"));
        let replacement_mode = raw_string.chars().count() > 128 || format_args.len() > 3;
        if replacement_mode {
            for (index, replacement) in format_args.iter().enumerate() {
                let _ = replacement;
                let marker = format_number_marker(index);
                for value in &mut localized {
                    *value = value.replace(&format!("{{{index}}}"), &marker);
                }
            }
            let encoded_segments = localized.iter().enumerate().map(|(index, value)| {
                if index == 0 {
                    format!("{tl_err_prefix}{value}")
                } else {
                    value.clone()
                }
            });
            for (index, segment) in encoded_segments.enumerate() {
                if segment.len() > 511 {
                    return Err(IntegrationError::new(
                        "translations-invalid",
                        format!(
                            "translated string for language '{}' is too long, maximum length is 511 bytes",
                            translations.languages[index]
                        ),
                        span,
                    ));
                }
            }
        }
        let encoded = format!("{tl_err_prefix}{}", localized.join("\u{ec48}"));
        let text = self.push_value(Value::String(encoded));
        let custom = if replacement_mode {
            let mut value = self.push_call("customString", vec![text]);
            for (index, arg) in format_args.iter().enumerate() {
                let marker = self.push_number(
                    format_number_marker_value(index),
                    &format_number_marker(index),
                );
                let marker = self.push_call("updateEveryFrame", vec![marker]);
                let replacement = self.lower_value(arg)?;
                value = self.push_call("stringReplace", vec![value, marker, replacement]);
            }
            value
        } else {
            let mut custom_args = vec![text];
            custom_args.extend(
                format_args
                    .iter()
                    .map(|arg| self.lower_value(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            self.push_call("customString", custom_args)
        };
        let helper_id = *self.globals.get(TRANSLATION_HELPER_NAME).ok_or_else(|| {
            IntegrationError::new(
                "translations-invalid",
                "translation helper variable was not allocated",
                span,
            )
        })?;
        let helper = self.push_value(Value::GlobalVariable(self.global_names[helper_id].clone()));
        let translated = self.push_call("stringSplit", vec![custom, helper]);
        if name == "___" {
            return Ok(translated);
        }
        if name == "_"
            && self
                .hir
                .preprocessing
                .directives
                .iter()
                .any(|directive| directive.name == "translateWithPlayerVar")
        {
            let variable = *self
                .players
                .get("__languageIndex__")
                .expect("translation player variable is allocated");
            let player = self.push_call("localPlayer", Vec::new());
            let index = self.push_value(Value::PlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
            });
            return Ok(self.push_call("valueInArray", vec![translated, index]));
        }
        Ok(self.select_translation(translated))
    }

    fn select_translation(&mut self, values: ValueId) -> ValueId {
        let helper_id = *self
            .globals
            .get(TRANSLATION_HELPER_NAME)
            .expect("translation helper variable is allocated");
        let helper = self.push_value(Value::GlobalVariable(self.global_names[helper_id].clone()));
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let empty_array = self.push_call("emptyArray", Vec::new());
        let color = self.push_call("stringSplit", vec![color, empty_array]);
        let index = self.push_call("indexOfArrayValue", vec![helper, color]);
        let index = self.push_call("absoluteValue", vec![index]);
        self.push_call("valueInArray", vec![values, index])
    }

    fn translation_language_index(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<ValueId, IntegrationError> {
        let helper = self.lower_translation_helper(translations)?;
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let empty_array = self.push_call("emptyArray", Vec::new());
        let color = self.push_call("stringSplit", vec![color, empty_array]);
        Ok(self.push_call("indexOfArrayValue", vec![helper, color]))
    }

    pub(super) fn translation_files(&self) -> Vec<(String, String)> {
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Vec::new();
        };
        let keep_unused = self
            .hir
            .preprocessing
            .directives
            .iter()
            .any(|directive| directive.name == "keepUnusedTranslations");
        translations
            .languages
            .iter()
            .skip(1)
            .map(|language| {
                let mut keys = self.translation_uses.clone();
                if keep_unused {
                    keys.extend(
                        translations
                            .entries
                            .iter()
                            .map(|entry| (entry.msgid.clone(), entry.context.clone())),
                    );
                }
                keys.sort();
                keys.dedup();
                let mut output = String::from(
                    "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n",
                );
                output.push_str(&format!("\"Language: {language}\\n\"\n\n"));
                for (msgid, context) in keys {
                    if let Some(ref context) = context {
                        output.push_str(&format!(
                            "msgctxt {}\n",
                            serde_json::to_string(&context).unwrap()
                        ));
                    }
                    let translated = translations
                        .entries
                        .iter()
                        .find(|entry| {
                            entry.msgid == msgid && entry.context.as_deref() == context.as_deref()
                        })
                        .and_then(|entry| entry.translations.get(language))
                        .cloned()
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "msgid {}\n",
                        serde_json::to_string(&msgid).unwrap()
                    ));
                    output.push_str(&format!(
                        "msgstr {}\n\n",
                        serde_json::to_string(&translated).unwrap()
                    ));
                }
                (language.clone(), output)
            })
            .collect()
    }

    fn lower_debug(
        &mut self,
        expr: &Expr,
        _span: Option<HirSpan>,
        debug_source: Option<&str>,
    ) -> Result<ActionId, IntegrationError> {
        let argument_span = expr.span().copied();
        let value = self.lower_text_value(expr)?;
        let array_text = if self.debug_value_is_array(value) {
            self.lower_debug_array_text(value, 6)
        } else {
            value
        };
        let debug_label_text = debug_source
            .map(str::to_string)
            .unwrap_or_else(|| debug_expr_text(expr));
        let debug_label = canonical_debug_text(&debug_label_text);
        let debug_prefix = format!("{debug_label}\u{2028}= {{0}}");
        let inline_padding = 128 - debug_prefix.chars().count() - "{1}".chars().count();
        let padding_text = self.push_value(Value::String(" ".repeat(170 - inline_padding)));
        let padding = self.push_call("customString", vec![padding_text]);
        let debug_label = self.push_value(Value::String(format!(
            "{debug_prefix}{}{{1}}",
            " ".repeat(inline_padding)
        )));
        let text = self.push_call("customString", vec![debug_label, array_text, padding]);
        let all_players = self.lower_all_players();
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let null_value_4 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_number(-9999.0, "-9999");
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_SORT_ORDER_STRING_AND_COLOR".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        let args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
                all_players,
                null_value,
                text,
                null_value_2,
                hud_position,
                sort_order,
                null_value_3,
                color,
                null_value_4,
                reevaluation,
                visibility,
            ],
        );
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                None,
                None,
                argument_span,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ))
    }

    fn lower_print(
        &mut self,
        expr: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let argument_span = expr.span().copied();
        let empty_string = matches!(expr, Expr::String { value, .. } if value.is_empty());
        let value = self.lower_value(expr)?;
        let value = if empty_string {
            self.push_value(Value::Null)
        } else {
            value
        };
        let padding_text = self.push_value(Value::String(" ".repeat(45)));
        let padding = self.push_call("customString", vec![padding_text]);
        let body_text = self.push_value(Value::String(format!("{}{{0}}", " ".repeat(125))));
        let body = self.push_call("customString", vec![body_text, padding]);
        let all_players = self.lower_all_players();
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_number(-9999.0, "-9999");
        let color = if empty_string {
            self.push_value(Value::Null)
        } else {
            self.push_value(Value::Enum {
                value_type: "Color".to_string(),
                value: "ORANGE".to_string(),
            })
        };
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_AND_STRING".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        let mut args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
                all_players,
                value,
                body,
                null_value,
                hud_position,
                sort_order,
                color,
                null_value_2,
                null_value_3,
                reevaluation,
                visibility,
            ],
        );
        self.apply_replacements("createHudText", &mut args, span);
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                None,
                argument_span,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ))
    }

    fn lower_debug_array_text(&mut self, value: ValueId, max_length: usize) -> ValueId {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let current_count = call!("countOf", call!("currentArrayElement"));
        let is_single = call!(
            "==",
            call!("countOf", call!("currentArrayElement")),
            self.push_number(1.0, "1")
        );
        let is_empty = call!("==", call!("currentArrayElement"), call!("emptyArray"));
        let not_null = call!(
            "!=",
            call!("currentArrayElement"),
            self.push_value(Value::Null)
        );
        let has_empty_array = call!("and", is_empty, not_null);
        let brackets = call!("or", is_single, has_empty_array);
        let first_element = call!(
            "customString",
            self.push_value(Value::String("[{0}]".to_string())),
            call!("currentArrayElement"),
        );
        let many_elements = call!(
            "customString",
            self.push_value(Value::String("[{0}, …+{1}]".to_string())),
            call!("currentArrayElement"),
            call!(
                "subtract",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(1.0, "1"),
            ),
        );
        let element_text = call!(
            "ifThenElse",
            brackets,
            first_element,
            call!(
                "ifThenElse",
                current_count,
                many_elements,
                call!("currentArrayElement"),
            ),
        );
        let mapped_elements = call!("mappedArray", value, element_text,);
        let mapped_input = call!("array", mapped_elements);
        let current_array = call!("currentArrayElement");
        let actual_array = call!(
            "or",
            call!("countOf", current_array),
            call!(
                "and",
                call!("==", call!("currentArrayElement"), call!("emptyArray")),
                call!(
                    "!=",
                    call!("currentArrayElement"),
                    self.push_value(Value::Null)
                ),
            ),
        );
        let empty_length = call!(
            "ifThenElse",
            call!(
                "and",
                call!("not", call!("countOf", call!("currentArrayElement"))),
                call!("!=", call!("currentArrayElement"), call!("emptyArray"),),
            ),
            self.push_number(3.0, "3"),
            call!(
                "multiply",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(3.0, "3"),
            ),
        );
        let x = call!(
            "appendToArray",
            call!("appendToArray", actual_array, empty_length),
            current_array,
        );
        let x_input = call!("mappedArray", mapped_input, x);
        let x_length = |this: &mut Self| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index = this.push_number(1.0, "1");
            this.push_call("valueInArray", vec![current, index])
        };
        let x_value = |this: &mut Self, index: f64| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index_value = this.push_number(index, &index.to_string());
            this.push_call("valueInArray", vec![current, index_value])
        };
        let first = call!("firstOf", call!("currentArrayElement"));
        let array_head = if max_length == 6 {
            let array_tail = call!(
                "customString",
                self.push_value(Value::String("{0}, {1}, {2}".to_string())),
                x_value(self, 4.0),
                x_value(self, 5.0),
                call!(
                    "customString",
                    self.push_value(Value::String("{0}, {1}, …\u{0001}".to_string())),
                    x_value(self, 6.0),
                    x_value(self, 7.0),
                ),
            );
            call!(
                "customString",
                self.push_value(Value::String("{0}, {1}, {2}".to_string())),
                x_value(self, 2.0),
                x_value(self, 3.0),
                array_tail,
            )
        } else if max_length <= 3 {
            let display = format!(
                "{}…\u{0001}",
                (0..max_length)
                    .map(|index| format!("{{{index}}}, "))
                    .collect::<String>()
            );
            let mut args = vec![self.push_value(Value::String(display))];
            for index in 0..max_length {
                args.push(x_value(self, (index + 2) as f64));
            }
            self.push_call("customString", args)
        } else {
            let mut array_head = self.push_value(Value::String("…\u{0001}".to_string()));
            for index in (0..max_length).rev() {
                array_head = call!(
                    "customString",
                    self.push_value(Value::String("{0}, {1}".to_string())),
                    x_value(self, (index + 2) as f64),
                    array_head,
                );
            }
            array_head
        };
        let placeholder_text = format!(
            "{}\u{2026}\u{0001}",
            (0..max_length).map(|_| "0, ").collect::<String>()
        );
        let placeholder = call!(
            "customString",
            self.push_value(Value::String(placeholder_text.clone())),
        );
        let length_for_slice = x_length(self);
        let end_length_for_slice = x_length(self);
        let start = self.push_number(
            (placeholder_text.chars().count() as isize - 4 - 3 * max_length as isize) as f64,
            "",
        );
        let end = self.push_number((max_length * 3 + 4) as f64, "");
        let slice = call!(
            "stringSlice",
            placeholder,
            call!("add", start, length_for_slice),
            call!("subtract", end, end_length_for_slice,),
        );
        let replaced = call!("stringReplace", array_head, slice, call!("emptyArray"),);
        let length_for_compare = x_length(self);
        let length_for_divide = x_length(self);
        let plus = call!(
            "ifThenElse",
            call!(
                ">",
                length_for_compare,
                self.push_number((max_length * 3) as f64, ""),
            ),
            call!(
                "customString",
                self.push_value(Value::String("+{0}".to_string())),
                call!(
                    "subtract",
                    call!("divide", length_for_divide, self.push_number(3.0, "3")),
                    self.push_number(max_length as f64, ""),
                ),
            ),
            call!("emptyArray"),
        );
        let formatted_array = call!(
            "customString",
            self.push_value(Value::String("[{0}{1}]".to_string())),
            replaced,
            plus,
        );
        let current_for_split = call!("currentArrayElement");
        let rendered = call!(
            "ifThenElse",
            first,
            formatted_array,
            call!(
                "stringSplit",
                call!(
                    "valueInArray",
                    current_for_split,
                    self.push_number(2.0, "2")
                ),
                call!("emptyArray"),
            ),
        );
        call!("mappedArray", x_input, rendered)
    }

    fn lower_text_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let value = self.lower_value(expr)?;
        let Value::Call { name, args } = self.value(value) else {
            return Ok(value);
        };
        if name == "customString" && args.len() == 1 {
            Ok(args[0])
        } else {
            Ok(value)
        }
    }

    fn debug_value_is_array(&self, value: ValueId) -> bool {
        match self.value(value) {
            Value::GlobalVariable(_) | Value::Array(_) => true,
            Value::Call { name, .. } if matches!(name.as_str(), "array" | "emptyArray") => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Array")
                }),
            _ => false,
        }
    }

    fn value_is_known_player(&self, value: ValueId) -> bool {
        match self.value(value) {
            Value::EventPlayer => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Player")
                }),
            _ => false,
        }
    }

    fn push_value(&mut self, value: Value) -> ValueId {
        let id = self.values.len();
        self.values.push(value);
        #[cfg(test)]
        crate::resource_metrics::record_lowering_values(self.values.len());
        id
    }

    fn push_call(&mut self, name: &str, args: Vec<ValueId>) -> ValueId {
        let args = self.normalize_contextual_arguments(name, args);
        self.push_value(Value::Call {
            name: name.to_string(),
            args,
        })
    }

    fn normalize_contextual_values(&mut self, call_id: &str, values: Vec<ValueId>) -> Vec<ValueId> {
        self.normalize_contextual_arguments(call_id, values)
    }

    fn contextual_coercions(&self, call_id: &str, arg_index: usize) -> Option<ParamCoercions> {
        [Kind::Action, Kind::Value].into_iter().find_map(|kind| {
            self.compiler
                .catalog
                .entry(kind, call_id)
                .and_then(|entry| entry.param_coercions(arg_index))
                .copied()
        })
    }

    fn normalize_value_with_coercions(
        &mut self,
        coercions: ParamCoercions,
        value_id: ValueId,
    ) -> ValueId {
        let Some(node) = self.values.get(value_id) else {
            return value_id;
        };
        let replacement = match node {
            Value::Bool(false) if coercions.false_as_number => Some(Value::Number(0.0)),
            Value::Bool(true) if coercions.true_as_number => Some(Value::Number(1.0)),
            Value::Number(value) if coercions.zero_as_null && *value == 0.0 => Some(Value::Null),
            Value::Vector { x, y, z }
                if coercions.null_vector_as_null
                    && self.value_is_number(*x, 0.0)
                    && self.value_is_number(*y, 0.0)
                    && self.value_is_number(*z, 0.0) =>
            {
                Some(Value::Null)
            }
            Value::Call { name, args }
                if coercions.null_vector_as_null
                    && name == "vector"
                    && args.len() == 3
                    && args.iter().all(|value| self.value_is_number(*value, 0.0)) =>
            {
                Some(Value::Null)
            }
            Value::Call { name, args }
                if coercions.empty_array_as_string && name == "emptyArray" && args.is_empty() =>
            {
                Some(Value::String(String::new()))
            }
            Value::Call { name, args }
                if coercions.empty_array_as_string
                    && name == "customString"
                    && args.len() == 1
                    && self.value_is_empty_string(args[0]) =>
            {
                Some(Value::String(String::new()))
            }
            Value::Array(elements) if coercions.empty_array_as_string && elements.is_empty() => {
                Some(Value::String(String::new()))
            }
            _ => None,
        };
        let Some(value) = replacement else {
            return value_id;
        };
        self.push_value(value)
    }

    fn normalize_contextual_argument(
        &mut self,
        call_id: &str,
        arg_index: usize,
        value_id: ValueId,
    ) -> ValueId {
        let domain = [Kind::Action, Kind::Value].into_iter().find_map(|kind| {
            self.compiler
                .catalog
                .entry(kind, call_id)
                .and_then(|entry| entry.param_domain(arg_index))
        });
        if domain == Some("BarrierLos") {
            let value = match self.value(value_id) {
                Value::Bool(true) => Some("PASS_THROUGH_BARRIERS"),
                Value::Bool(false) => Some("BLOCKED_BY_ALL_BARRIERS"),
                _ => None,
            };
            if let Some(value) = value {
                return self.push_value(Value::Enum {
                    value_type: "BarrierLos".to_string(),
                    value: value.to_string(),
                });
            }
        }
        let Some(coercions) = self.contextual_coercions(call_id, arg_index) else {
            return value_id;
        };
        self.normalize_value_with_coercions(coercions, value_id)
    }

    #[allow(unreachable_patterns)]
    fn normalize_modify_value(&mut self, op: ModifyOp, value_id: ValueId) -> ValueId {
        let coercions = match op {
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Modulo
            | ModifyOp::Min
            | ModifyOp::Max
            | ModifyOp::RemoveFromArrayByIndex => ParamCoercions {
                false_as_number: true,
                true_as_number: true,
                ..Default::default()
            },
            ModifyOp::AppendToArray | ModifyOp::RemoveFromArrayByValue => ParamCoercions {
                zero_as_null: true,
                ..Default::default()
            },
            ModifyOp::Multiply | ModifyOp::Divide | ModifyOp::RaiseToPower => {
                return value_id;
            }
            _ => return value_id,
        };
        self.normalize_value_with_coercions(coercions, value_id)
    }

    fn modify_op_from_value(&self, value_id: ValueId) -> Option<ModifyOp> {
        let Value::Call { name, args } = self.values.get(value_id)? else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        match name.as_str() {
            "add" => Some(ModifyOp::Add),
            "subtract" => Some(ModifyOp::Subtract),
            "multiply" => Some(ModifyOp::Multiply),
            "divide" => Some(ModifyOp::Divide),
            "modulo" => Some(ModifyOp::Modulo),
            "min" => Some(ModifyOp::Min),
            "max" => Some(ModifyOp::Max),
            "raiseToPower" => Some(ModifyOp::RaiseToPower),
            "appendToArray" => Some(ModifyOp::AppendToArray),
            "removeFromArray" | "removeFromArrayByValue" => Some(ModifyOp::RemoveFromArrayByValue),
            "removeFromArrayByIndex" => Some(ModifyOp::RemoveFromArrayByIndex),
            _ => None,
        }
    }

    fn normalize_contextual_arguments(
        &mut self,
        call_id: &str,
        mut args: Vec<ValueId>,
    ) -> Vec<ValueId> {
        let mut index = 0;
        while index < args.len() {
            args[index] = self.normalize_contextual_argument(call_id, index, args[index]);
            if matches!(
                call_id,
                "modifyGlobalVariableAtIndex" | "modifyPlayerVariableAtIndex"
            ) && index == 3
            {
                if let Some(op) = args
                    .get(2)
                    .and_then(|value_id| self.modify_op_from_value(*value_id))
                {
                    args[index] = self.normalize_modify_value(op, args[index]);
                }
            }
            index += 1;
        }
        args
    }

    fn lower_custom_string(
        &mut self,
        value: String,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let _ = span;
        let text = self.push_value(Value::String(value));
        Ok(self.push_call("customString", vec![text]))
    }

    fn fold_format_constants<'b>(
        &self,
        text: &str,
        args: &'b [hir::Expr],
    ) -> (String, Vec<&'b hir::Expr>) {
        let values = args
            .iter()
            .map(|arg| {
                let mut stack = Vec::new();
                crate::compile_time::evaluate(arg, &self.constants, &HashMap::new(), &mut stack)
                    .and_then(compile_time_value_text)
            })
            .collect::<Vec<_>>();
        let dynamic_indexes = values
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.is_none().then_some(index))
            .collect::<Vec<_>>();
        let dynamic_args = dynamic_indexes
            .iter()
            .map(|index| &args[*index])
            .collect::<Vec<_>>();
        let dynamic_position = dynamic_indexes
            .iter()
            .enumerate()
            .map(|(position, index)| (*index, position))
            .collect::<HashMap<_, _>>();

        let canonical = canonical_format_text(text);
        let mut output = String::with_capacity(canonical.len());
        let mut cursor = 0;
        while cursor < canonical.len() {
            let Some(open_rel) = canonical[cursor..].find('{') else {
                output.push_str(&canonical[cursor..]);
                break;
            };
            let open = cursor + open_rel;
            output.push_str(&canonical[cursor..open]);
            let Some(close_rel) = canonical[open + 1..].find('}') else {
                output.push_str(&canonical[open..]);
                break;
            };
            let close = open + 1 + close_rel;
            let marker = &canonical[open + 1..close];
            let Ok(index) = marker.parse::<usize>() else {
                output.push_str(&canonical[open..=close]);
                cursor = close + 1;
                continue;
            };
            if let Some(Some(value)) = values.get(index) {
                output.push_str(value);
            } else if let Some(position) = dynamic_position.get(&index) {
                output.push('{');
                output.push_str(&position.to_string());
                output.push('}');
            } else {
                output.push_str(&canonical[open..=close]);
            }
            cursor = close + 1;
        }
        (output, dynamic_args)
    }

    fn push_number(&mut self, value: f64, text: &str) -> ValueId {
        let _ = text;
        self.push_value(Value::Number(value))
    }

    fn canonical_vector_member(&self, x: ValueId, y: ValueId, z: ValueId) -> Option<&'static str> {
        let number = |id| match self.values.get(id)? {
            Value::Number(value) => Some(*value),
            _ => None,
        };
        match (number(x), number(y), number(z)) {
            (Some(1.0), Some(0.0), Some(0.0)) => Some("LEFT"),
            (Some(-1.0), Some(0.0), Some(0.0)) => Some("RIGHT"),
            (Some(0.0), Some(1.0), Some(0.0)) => Some("UP"),
            (Some(0.0), Some(-1.0), Some(0.0)) => Some("DOWN"),
            (Some(0.0), Some(0.0), Some(1.0)) => Some("FORWARD"),
            (Some(0.0), Some(0.0), Some(-1.0)) => Some("BACKWARD"),
            _ => None,
        }
    }

    fn fold_numeric_binary(&self, op: &str, left: ValueId, right: ValueId) -> Option<f64> {
        let number = |id| match self.values.get(id)? {
            Value::Number(value) => Some(*value),
            _ => None,
        };
        let left = number(left)?;
        let right = number(right)?;
        let value = match op {
            "+" => left + right,
            "-" => left - right,
            "*" => left * right,
            "/" if right != 0.0 => left / right,
            "%" if right != 0.0 => left % right,
            "**" => left.powf(right),
            _ => return None,
        };
        value.is_finite().then_some(value)
    }

    fn value_is_number(&self, id: ValueId, expected: f64) -> bool {
        matches!(self.values.get(id), Some(Value::Number(value)) if *value == expected)
    }

    fn value_is_empty_string(&self, id: ValueId) -> bool {
        matches!(self.values.get(id), Some(Value::String(value)) if value.is_empty())
    }

    fn lower_condition(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        self.lower_value(expr)
    }

    fn lower_delete(
        &mut self,
        target: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let mut indices = Vec::new();
        let Some(root) = indexed_target_parts(target, &mut indices) else {
            return Err(self.unsupported(
                "delete statements require an indexed global or player variable",
                span,
            ));
        };
        if indices.len() > 4 {
            return Err(self.unsupported("Cannot delete index of 4d array", span));
        }
        indices.reverse();
        if indices.len() >= 3
            && (expr_contains_random(root)
                || indices[..indices.len() - 1]
                    .iter()
                    .any(|index| expr_contains_random(index)))
        {
            return Err(self.unsupported(
                "Cannot delete from nested array with a random outer or middle index",
                span,
            ));
        }
        let (root_value, action_name) = match root {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let root_value =
                    self.push_value(Value::GlobalVariable(self.global_names[variable].clone()));
                (root_value, "modifyGlobalVariableAtIndex")
            }
            Expr::PlayerVar {
                player,
                name,
                span: target_span,
                ..
            } => {
                let variable = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                })?;
                let player = self.lower_value(player)?;
                let value = self.push_value(Value::PlayerVariable {
                    player,
                    variable: self.player_names[variable].clone(),
                });
                (value, "modifyPlayerVariableAtIndex")
            }
            _ => {
                return Err(self.unsupported(
                    "delete statements are only representable for global or player variables",
                    target.span().copied(),
                ));
            }
        };
        let index = self.lower_value(indices[0])?;
        if indices.len() == 1 {
            let op = ModifyOp::RemoveFromArrayByIndex;
            let index = self.normalize_modify_value(op, index);
            return Ok(if action_name == "modifyGlobalVariableAtIndex" {
                let variable = match self.values.get(root_value) {
                    Some(Value::GlobalVariable(variable)) => variable.clone(),
                    _ => unreachable!("global delete root must be a global variable value"),
                };
                self.push_action(Action::ModifyGlobalVariable {
                    variable,
                    op,
                    value: index,
                })
            } else {
                let (player, variable) = match self.values.get(root_value) {
                    Some(Value::PlayerVariable { player, variable }) => (*player, variable.clone()),
                    _ => unreachable!("player delete root must be a player variable value"),
                };
                self.push_action(Action::ModifyPlayerVariable {
                    player,
                    variable,
                    op,
                    value: index,
                })
            });
        }

        let op = self.push_call("removeFromArrayByIndex", Vec::new());
        if indices.len() == 2 {
            let inner_index = self.lower_value(indices[1])?;
            let args = self.normalize_contextual_arguments(
                action_name,
                vec![root_value, index, op, inner_index],
            );
            return Ok(self.push_call_action(action_name, &args));
        }

        let outer_array = self.lower_indexed_read(root_value, indices[0], index)?;
        if indices.len() == 4 {
            let replacement = self.rebuild_deleted_array(outer_array, &indices[1..], span)?;
            let action_name = if action_name == "modifyGlobalVariableAtIndex" {
                "setGlobalVariableAtIndex"
            } else {
                "setPlayerVariableAtIndex"
            };
            let args = self
                .normalize_contextual_arguments(action_name, vec![root_value, index, replacement]);
            return Ok(self.push_call_action(action_name, &args));
        }
        let inner_index = self.lower_value(indices[1])?;
        let row = self.lower_indexed_read(outer_array, indices[1], inner_index)?;
        let leaf_index = self.lower_value(indices[2])?;
        let current_index = self.push_call("currentArrayIndex", Vec::new());
        let condition = self.push_call("!=", vec![current_index, leaf_index]);
        let filtered = self.push_call("filteredArray", vec![row, condition]);
        let replacement = if let Some(number) = literal_number(indices[1]) {
            let middle = self.lower_array(vec![filtered], span)?;
            let maximum = self.push_number(999_999_999_999.0, "999999999999");
            let suffix_start = self.push_number(number + 1.0, &(number + 1.0).to_string());
            let suffix = self.push_call("slice", vec![outer_array, suffix_start, maximum]);
            if number == 0.0 {
                self.push_call("appendToArray", vec![middle, suffix])
            } else {
                let zero = self.push_number(0.0, "0");
                let prefix = self.push_call("slice", vec![outer_array, zero, inner_index]);
                let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
                self.push_call("appendToArray", vec![with_replacement, suffix])
            }
        } else {
            self.replace_array_element(outer_array, inner_index, filtered, span)?
        };
        let action_name = if action_name == "modifyGlobalVariableAtIndex" {
            "setGlobalVariableAtIndex"
        } else {
            "setPlayerVariableAtIndex"
        };
        let args =
            self.normalize_contextual_arguments(action_name, vec![root_value, index, replacement]);
        Ok(self.push_call_action(action_name, &args))
    }

    fn lower_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let mut indices = Vec::new();
        if let Some(root) = indexed_target_parts(target, &mut indices) {
            if indices.len() > 3 {
                return Err(self.unsupported("Cannot assign to 4d array", target.span().copied()));
            }
            if indices.len() > 1 {
                indices.reverse();
                return self.lower_nested_indexed_assign(root, &indices, target, value, span);
            }
        }
        match target {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                if let Expr::Binary {
                    op, left, right, ..
                } = value
                {
                    if let Expr::GlobalVar {
                        name: left_name, ..
                    } = left.as_ref()
                    {
                        if left_name == name {
                            if let Some(modify_op) = modify_op_from_str(op) {
                                let right = self.lower_value(right)?;
                                let val = self.normalize_modify_value(modify_op, right);
                                return Ok(self.push_action(Action::ModifyGlobalVariable {
                                    variable: self.global_names[variable].clone(),
                                    op: modify_op,
                                    value: val,
                                }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value: val,
                }))
            }
            Expr::PlayerVar {
                player,
                name,
                span: target_span,
                ..
            } => {
                let variable = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                })?;
                let player_val = self.lower_value(player)?;
                if let Expr::Binary {
                    op, left, right, ..
                } = value
                {
                    if let Expr::PlayerVar {
                        player: left_player,
                        name: left_name,
                        ..
                    } = left.as_ref()
                    {
                        if left_name == name && left_player.as_ref() == player.as_ref() {
                            if let Some(modify_op) = modify_op_from_str(op) {
                                let right = self.lower_value(right)?;
                                let val = self.normalize_modify_value(modify_op, right);
                                    return Ok(self.push_action(Action::ModifyPlayerVariable {
                                        player: player_val,
                                        variable: self.player_names[variable].clone(),
                                        op: modify_op,
                                        value: val,
                                    }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetPlayerVariable {
                    player: player_val,
                    variable: self.player_names[variable].clone(),
                    value: val,
                }))
            }
            Expr::Index {
                array,
                index,
                span: target_span,
            } => match array.as_ref() {
                Expr::GlobalVar {
                    name,
                    span: arr_span,
                } => {
                    let variable = *self.globals.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown global variable '{name}'"), *arr_span)
                    })?;
                    let var_node = self.push_value(Value::GlobalVariable(
                        self.global_names[variable].clone(),
                    ));
                    let index_val = self.lower_value(index)?;
                    if let Expr::Binary {
                        op, left, right, ..
                    } = value
                    {
                        if let Expr::Index {
                            array: left_arr,
                            index: left_idx,
                            ..
                        } = left.as_ref()
                        {
                            if left_arr.as_ref() == array.as_ref()
                                && left_idx.as_ref() == index.as_ref()
                            {
                                if let Some(modify_op) = modify_op_from_str(op) {
                                    let op_id = modify_catalog_name_from_str(op)
                                        .expect("known modify operator has a catalog name");
                                    let op_node = self.push_call(op_id, Vec::new());
                                    let right = self.lower_value(right)?;
                                    let right_val = self.normalize_modify_value(modify_op, right);
                                    let args = self.normalize_contextual_arguments(
                                        "modifyGlobalVariableAtIndex",
                                        vec![var_node, index_val, op_node, right_val],
                                    );
                                    return Ok(self.push_call_action(
                                        "modifyGlobalVariableAtIndex",
                                        &args,
                                    ));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    let args = self.normalize_contextual_arguments(
                        "setGlobalVariableAtIndex",
                        vec![var_node, index_val, val],
                    );
                    Ok(self.push_call_action("setGlobalVariableAtIndex", &args))
                }
                Expr::PlayerVar {
                    player,
                    name,
                    span: arr_span,
                    ..
                } => {
                    let player_val = self.lower_value(player)?;
                    let variable = *self.players.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown player variable '{name}'"), *arr_span)
                    })?;
                    let var_node = self.push_value(Value::PlayerVariable {
                        player: player_val,
                        variable: self.player_names[variable].clone(),
                    });
                    let index_val = self.lower_value(index)?;
                    if let Expr::Binary {
                        op, left, right, ..
                    } = value
                    {
                        if let Expr::Index {
                            array: left_arr,
                            index: left_idx,
                            ..
                        } = left.as_ref()
                        {
                            if left_arr.as_ref() == array.as_ref()
                                && left_idx.as_ref() == index.as_ref()
                            {
                                if let Some(modify_op) = modify_op_from_str(op) {
                                    let op_id = modify_catalog_name_from_str(op)
                                        .expect("known modify operator has a catalog name");
                                    let op_node = self.push_call(op_id, Vec::new());
                                    let right = self.lower_value(right)?;
                                    let right_val = self.normalize_modify_value(modify_op, right);
                                    let args = self.normalize_contextual_arguments(
                                        "modifyPlayerVariableAtIndex",
                                        vec![var_node, index_val, op_node, right_val],
                                    );
                                    // The canonical signature takes the
                                    // player-variable value node (which
                                    // carries the player) as its first
                                    // argument.
                                    return Ok(self.push_call_action(
                                        "modifyPlayerVariableAtIndex",
                                        &args,
                                    ));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    let args = self.normalize_contextual_arguments(
                        "setPlayerVariableAtIndex",
                        vec![var_node, index_val, val],
                    );
                    // The canonical signature takes the player-variable
                    // value node (which carries the player) as its first
                    // argument.
                    Ok(self.push_call_action("setPlayerVariableAtIndex", &args))
                }
                _ => Err(self.unsupported(
                    "indexing assignment is only representable for global or player variables",
                    *target_span,
                )),
            },
            _ => Err(self.unsupported(
                "only global-variable, player-variable, or index assignment is currently representable in canonical WIR",
                span,
            )),
        }
    }

    fn lower_nested_indexed_assign(
        &mut self,
        root: &Expr,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let (action_name, root_value) = match root {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let root_value =
                    self.push_value(Value::GlobalVariable(self.global_names[variable].clone()));
                ("setGlobalVariableAtIndex", root_value)
            }
            Expr::PlayerVar {
                player,
                name,
                span: target_span,
                ..
            } => {
                let player_value = self.lower_value(player)?;
                let variable = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                })?;
                let root_value = self.push_value(Value::PlayerVariable {
                    player: player_value,
                    variable: self.player_names[variable].clone(),
                });
                ("setPlayerVariableAtIndex", root_value)
            }
            _ => {
                return Err(self.unsupported(
                    "indexing assignment is only representable for global or player variables",
                    target.span().copied(),
                ));
            }
        };

        let outer_index = self.lower_value(indices[0])?;
        let outer_array = self.lower_indexed_read(root_value, indices[0], outer_index)?;
        let replacement =
            self.rebuild_indexed_value(outer_array, &indices[1..], target, value, span)?;
        let args = self.normalize_contextual_arguments(
            action_name,
            vec![root_value, outer_index, replacement],
        );
        Ok(self.push_call_action(action_name, &args))
    }

    fn rebuild_indexed_value(
        &mut self,
        array: ValueId,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let index = indices
            .first()
            .copied()
            .expect("nested indexed assignment has an inner index");
        let index_value = self.lower_value(index)?;
        let replacement = if indices.len() == 1 {
            if let Expr::Binary {
                op, left, right, ..
            } = value
                && left.as_ref() == target
                && let Some(call_name) = modify_catalog_name_from_str(op)
            {
                let current = self.lower_indexed_read(array, index, index_value)?;
                let right = self.lower_value(right)?;
                self.push_call(call_name, vec![current, right])
            } else {
                self.lower_value(value)?
            }
        } else {
            let child = self.lower_indexed_read(array, index, index_value)?;
            self.rebuild_indexed_value(child, &indices[1..], target, value, span)?
        };
        self.replace_array_element(array, index_value, replacement, span)
    }

    fn lower_indexed_read(
        &mut self,
        array: ValueId,
        index: &Expr,
        index_value: ValueId,
    ) -> Result<ValueId, IntegrationError> {
        if matches!(index, Expr::Number { value, .. } if *value == 0.0) {
            Ok(self.push_call("firstOf", vec![array]))
        } else {
            Ok(self.push_call("valueInArray", vec![array, index_value]))
        }
    }

    fn replace_array_element(
        &mut self,
        array: ValueId,
        index: ValueId,
        replacement: ValueId,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let zero = self.push_number(0.0, "0");
        let one = self.push_number(1.0, "1");
        let end = self.push_call("add", vec![index, one]);
        let maximum = self.push_number(999_999_999_999.0, "999999999999");
        let prefix = self.push_call("slice", vec![array, zero, index]);
        let middle = self.lower_array(vec![replacement], span)?;
        let suffix = self.push_call("slice", vec![array, end, maximum]);
        let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
        Ok(self.push_call("appendToArray", vec![with_replacement, suffix]))
    }

    fn rebuild_deleted_array(
        &mut self,
        array: ValueId,
        indices: &[&Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let index = self.lower_value(indices[0])?;
        if indices.len() == 1 {
            let current_index = self.push_call("currentArrayIndex", Vec::new());
            let condition = self.push_call("!=", vec![current_index, index]);
            return Ok(self.push_call("filteredArray", vec![array, condition]));
        }
        let child = self.lower_indexed_read(array, indices[0], index)?;
        let replacement = self.rebuild_deleted_array(child, &indices[1..], span)?;
        self.replace_array_element_for_delete(array, indices[0], index, replacement, span)
    }

    fn replace_array_element_for_delete(
        &mut self,
        array: ValueId,
        index_expr: &Expr,
        index: ValueId,
        replacement: ValueId,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        if let Some(number) = literal_number(index_expr) {
            let middle = self.lower_array(vec![replacement], span)?;
            let maximum = self.push_number(999_999_999_999.0, "999999999999");
            let suffix_start = self.push_number(number + 1.0, &(number + 1.0).to_string());
            let suffix = self.push_call("slice", vec![array, suffix_start, maximum]);
            if number == 0.0 {
                return Ok(self.push_call("appendToArray", vec![middle, suffix]));
            }
            let zero = self.push_number(0.0, "0");
            let prefix = self.push_call("slice", vec![array, zero, index]);
            let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
            return Ok(self.push_call("appendToArray", vec![with_replacement, suffix]));
        }
        self.replace_array_element(array, index, replacement, span)
    }

    fn lower_cased_progress_bar(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let [
            Expr::Number {
                value: text_count, ..
            },
            visible_to,
            Expr::String { value: text, .. },
            position,
            scale,
            clipping,
            text_color,
            reevaluation,
            spectators,
        ] = args
        else {
            return Err(self.unsupported(
                "createCasedProgressBarIwt requires a literal text count and text",
                span,
            ));
        };
        let text_count_value = *text_count;
        if !text_count_value.is_finite()
            || text_count_value.fract() != 0.0
            || !(2.0..=6.0).contains(&text_count_value)
        {
            return Err(self.unsupported(
                "createCasedProgressBarIwt text count must be between 2 and 6",
                span,
            ));
        }
        let text_count = text_count_value as usize;
        if args.iter().any(expr_contains_random) {
            return Err(self.unsupported(
                "Cannot use random functions in createCasedProgressBarIwt",
                span,
            ));
        }
        let visible_to = self.lower_value(visible_to)?;
        let position = self.lower_value(position)?;
        let scale = self.lower_value(scale)?;
        let clipping = self.lower_value(clipping)?;
        let text_color = self.lower_value(text_color)?;
        let reevaluation = self.lower_value(reevaluation)?;
        let spectators = self.lower_value(spectators)?;
        let header_color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let texts = text
            .replace('\n', "  \n  ")
            .split('\n')
            .map(|line| {
                cased_line(line, text_count)
                    .into_iter()
                    .map(|line| format!("{line}\u{ad}"))
                    .collect::<Vec<_>>()
            })
            .reduce(|mut all, lines| {
                for (index, line) in lines.into_iter().enumerate() {
                    if index < all.len() {
                        all[index].push('\n');
                        all[index].push_str(&line);
                    }
                }
                all
            })
            .unwrap_or_else(|| vec![String::new(); text_count]);
        let mut actions = Vec::with_capacity(text_count);
        for (index, text) in texts.into_iter().enumerate() {
            let value = self.push_number(index as f64, &index.to_string());
            let text = self.lower_custom_string(text, span)?;
            let values = self.normalize_contextual_arguments(
                "createProgressBarInWorldText",
                vec![
                    visible_to,
                    value,
                    text,
                    position,
                    scale,
                    clipping,
                    header_color,
                    text_color,
                    reevaluation,
                    spectators,
                ],
            );
            actions.push(self.push_call_action_with_spans(
                "createProgressBarInWorldText",
                &values,
                [None; 10],
            ));
        }
        Ok(actions)
    }

    fn lower_action_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        if args.is_empty() {
            if let Some(&subroutine) = self.subroutines.get(name) {
                return Ok(self.push_action(Action::CallSubroutine {
                    subroutine: self.subroutine_names[subroutine].clone(),
                }));
            }
        }
        if name == "chaseAtRate" {
            let spans = args
                .iter()
                .map(|expr| expr.span().copied())
                .collect::<Vec<_>>();
            let args = args
                .iter()
                .map(|expr| self.lower_value(expr))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(self.push_call_action_with_spans(name, &args, spans));
        }
        let function = self
            .compiler
            .manifest
            .resolve_function(name)
            .ok_or_else(|| self.unsupported(format!("unknown action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::Action) {
            return Err(self.unsupported(format!("'{name}' is not a generic OPY action"), span));
        }
        if function.id == "async" {
            let [subroutine, behavior] = args else {
                return Err(
                    self.unsupported("async requires a subroutine and an AsyncBehavior", span)
                );
            };
            let subroutine_name = match subroutine {
                Expr::Call { name, args, .. } if args.is_empty() => name,
                _ => {
                    return Err(self.unsupported(
                        "async requires a declared subroutine",
                        subroutine.span().copied(),
                    ));
                }
            };
            let subroutine_id = *self.subroutines.get(subroutine_name).ok_or_else(|| {
                self.unsupported(
                    format!("unknown subroutine '{subroutine_name}'"),
                    subroutine.span().copied(),
                )
            })?;
            let subroutine_span = subroutine.span().copied();
            let behavior_span = behavior.span().copied();
            let subroutine = self.push_value(Value::Subroutine(
                self.subroutine_names[subroutine_id].clone(),
            ));
            let behavior = self.lower_value(behavior)?;
            return Ok(self.push_call_action_with_spans(
                "startRule",
                &[subroutine, behavior],
                [subroutine_span, behavior_span],
            ));
        }
        if matches!(
            function.id.as_str(),
            "hudHeader" | "hudSubheader" | "hudSubtext"
        ) {
            let text_slot = match function.id.as_str() {
                "hudHeader" => 1,
                "hudSubheader" => 2,
                "hudSubtext" => 3,
                _ => unreachable!(),
            };
            return self.lower_hud_text(args, span, text_slot, &function.id);
        }
        if function.id == "createDummy" && args.len() == 4 {
            let spans = args
                .iter()
                .map(|expr| expr.span().copied())
                .chain(std::iter::once(None));
            let mut lowered = args
                .iter()
                .map(|expr| self.lower_value(expr))
                .collect::<Result<Vec<_>, _>>()?;
            let mut zero_vector = Vec::with_capacity(3);
            for value in [0.0, 0.0, 0.0] {
                zero_vector.push(self.push_number(value, "0"));
            }
            lowered.push(self.push_call("vector", zero_vector));
            let args = self.normalize_contextual_arguments("createDummyBot", lowered);
            return Ok(self.push_call_action_with_spans("createDummyBot", &args, spans));
        }
        let spans = args
            .iter()
            .map(|expr| expr.span().copied())
            .collect::<Vec<_>>();
        let args = args
            .iter()
            .map(|expr| self.lower_value(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let catalog_id = if matches!(function.id.as_str(), "stopChasingVariable" | "stopChasing") {
            match args.first().map(|value| self.value(*value)) {
                Some(Value::GlobalVariable(_)) => "stopChasingGlobalVariable",
                Some(Value::PlayerVariable { .. }) => "stopChasingPlayerVariable",
                _ => {
                    return Err(self.unsupported(
                        "stopChasingVariable requires a global or player variable",
                        span,
                    ));
                }
            }
        } else {
            function.catalog_id.as_deref().ok_or_else(|| {
                self.unsupported(
                    format!(
                        "action '{}' requires a special lowering not in #46",
                        function.id
                    ),
                    span,
                )
            })?
        };
        let mut args = self.normalize_catalog_argument_domains(catalog_id, args);
        self.apply_replacements(catalog_id, &mut args, span);
        self.optimize_wait_duration(catalog_id, &mut args, span);
        Ok(self.push_call_action_with_spans(catalog_id, &args, spans))
    }

    fn optimize_wait_duration(
        &mut self,
        catalog_id: &str,
        args: &mut [ValueId],
        span: Option<HirSpan>,
    ) {
        const DEFAULT_WAIT_SECONDS: f64 = 0.016;

        let optimization = self.optimization_state_at(span.as_ref());
        if catalog_id != "wait" || !optimization.enabled || !optimization.for_size {
            return;
        }
        let Some(duration) = args.first().copied() else {
            return;
        };
        match self.value(duration) {
            Value::Number(value) if *value <= DEFAULT_WAIT_SECONDS => {
                let value = self.push_value(Value::Bool(false));
                args[0] = self.normalize_contextual_argument(catalog_id, 0, value);
            }
            Value::Number(value) if *value == 1.0 => {
                let value = self.push_value(Value::Bool(true));
                args[0] = self.normalize_contextual_argument(catalog_id, 0, value);
            }
            _ => {}
        }
    }

    fn normalize_catalog_argument_domains(
        &mut self,
        catalog_id: &str,
        mut args: Vec<ValueId>,
    ) -> Vec<ValueId> {
        if self
            .compiler
            .catalog
            .entry(Kind::Action, catalog_id)
            .is_none()
        {
            return args;
        }
        let mut index = 0;
        while index < args.len() {
            args[index] = self.normalize_contextual_argument(catalog_id, index, args[index]);
            let Some(domain) = self
                .compiler
                .catalog
                .entry(Kind::Action, catalog_id)
                .and_then(|entry| entry.param_domain(index))
                .map(str::to_string)
            else {
                index += 1;
                continue;
            };
            let Some(Value::Enum { value_type, value }) = self.values.get(args[index]) else {
                index += 1;
                continue;
            };
            if value_type == "Team" && domain == "Color" {
                args[index] = self.push_value(Value::Enum {
                    value_type: domain,
                    value: value.clone(),
                });
            }
            index += 1;
        }
        args
    }

    fn lower_hud_text(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
        text_slot: usize,
        function_name: &str,
    ) -> Result<ActionId, IntegrationError> {
        let [
            visible_to,
            text,
            position,
            sort_order,
            color,
            reevaluation,
            spectators,
        ] = args
        else {
            return Err(self.unsupported(
                format!("{function_name} requires exactly seven bound arguments"),
                span,
            ));
        };
        let visible_to_span = visible_to.span().copied();
        let visible_to = self.lower_hud_visible_to(visible_to)?;
        let mut text_slots = [
            self.push_value(Value::Null),
            self.push_value(Value::Null),
            self.push_value(Value::Null),
        ];
        let text_value = self.lower_text_value(text)?;
        text_slots[text_slot - 1] = if matches!(
            self.value(text_value),
            Value::Call { name, .. } if name == "customString"
        ) {
            text_value
        } else {
            self.push_call("customString", vec![text_value])
        };
        let mut colors = [
            self.push_value(Value::Null),
            self.push_value(Value::Null),
            self.push_value(Value::Null),
        ];
        colors[text_slot - 1] = self.lower_value(color)?;
        let args = vec![
            visible_to,
            text_slots[0],
            text_slots[1],
            text_slots[2],
            self.lower_value(position)?,
            self.lower_value(sort_order)?,
            colors[0],
            colors[1],
            colors[2],
            self.lower_value(reevaluation)?,
            self.lower_value(spectators)?,
        ];
        let args = self.normalize_contextual_arguments("createHudText", args);
        let text_span = text.span().copied();
        let color_span = color.span().copied();
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                visible_to_span,
                (text_slot == 1).then_some(text_span).flatten(),
                (text_slot == 2).then_some(text_span).flatten(),
                (text_slot == 3).then_some(text_span).flatten(),
                position.span().copied(),
                sort_order.span().copied(),
                (text_slot == 1).then_some(color_span).flatten(),
                (text_slot == 2).then_some(color_span).flatten(),
                (text_slot == 3).then_some(color_span).flatten(),
                reevaluation.span().copied(),
                spectators.span().copied(),
            ],
        ))
    }

    fn lower_hud_visible_to(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        if let Expr::Call { name, args, .. } = expr {
            if name == "getAllPlayers" && args.is_empty() {
                return Ok(self.lower_all_players());
            }
        }
        self.lower_value(expr)
    }

    fn lower_all_players(&mut self) -> ValueId {
        let all_teams = self.push_value(Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        });
        self.push_call("allPlayers", vec![all_teams])
    }

    fn lower_receiver_action_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let function = self
            .compiler
            .manifest
            .resolve_member(name)
            .ok_or_else(|| self.unsupported(format!("unknown member action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::MemberAction) {
            return Err(self.unsupported(format!("'{name}' is not a member action"), span));
        }

        // `append` is an OPY mutation, represented by the canonical variable
        // modify actions rather than a catalog action call.
        if matches!(function.id.as_str(), "append" | "remove") {
            let [value] = args else {
                return Err(self.unsupported(
                    format!("{} requires exactly one argument", function.id),
                    span,
                ));
            };
            let op = if function.id == "append" {
                ModifyOp::AppendToArray
            } else {
                ModifyOp::RemoveFromArrayByValue
            };
            let value_span = value.span().copied();
            let value = self.lower_value(value)?;
            let value = self.normalize_modify_value(op, value);
            return match receiver {
                Expr::GlobalVar {
                    name,
                    span: target_span,
                } => {
                    let variable = *self.globals.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                    })?;
                    let action = self.push_action(Action::ModifyGlobalVariable {
                        variable: self.global_names[variable].clone(),
                        op,
                        value,
                    });
                    self.mark_action_argument_origins(action, [value_span]);
                    Ok(action)
                }
                Expr::PlayerVar {
                    player,
                    name,
                    span: target_span,
                    ..
                } => {
                    let variable = *self.players.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                    })?;
                    let player_span = player.span().copied();
                    let player = self.lower_value(player)?;
                    let action = self.push_action(Action::ModifyPlayerVariable {
                        player,
                        variable: self.player_names[variable].clone(),
                        op,
                        value,
                    });
                    self.mark_action_argument_origins(action, [player_span, value_span]);
                    Ok(action)
                }
                Expr::Index { array, index, .. } => {
                    let op_name = if function.id == "append" {
                        "appendToArray"
                    } else {
                        "removeFromArray"
                    };
                    let op_node = self.push_call(op_name, Vec::new());
                    let index_span = index.span().copied();
                    let target_span = array.span().copied();
                    let index = self.lower_value(index)?;
                    match array.as_ref() {
                        Expr::GlobalVar {
                            name,
                            span: array_span,
                        } => {
                            let variable = *self.globals.get(name).ok_or_else(|| {
                                self.unsupported(
                                    format!("unknown global variable '{name}'"),
                                    *array_span,
                                )
                            })?;
                            let variable = self.push_value(Value::GlobalVariable(
                                self.global_names[variable].clone(),
                            ));
                            let args = self.normalize_contextual_arguments(
                                "modifyGlobalVariableAtIndex",
                                vec![variable, index, op_node, value],
                            );
                            let action =
                                self.push_call_action("modifyGlobalVariableAtIndex", &args);
                            self.mark_action_argument_origins(
                                action,
                                [target_span, index_span, None, value_span],
                            );
                            return Ok(action);
                        }
                        Expr::PlayerVar {
                            player,
                            name,
                            span: array_span,
                            ..
                        } => {
                            let variable = *self.players.get(name).ok_or_else(|| {
                                self.unsupported(
                                    format!("unknown player variable '{name}'"),
                                    *array_span,
                                )
                            })?;
                            let player = self.lower_value(player)?;
                            let variable = self.push_value(Value::PlayerVariable {
                                player,
                                variable: self.player_names[variable].clone(),
                            });
                            let args = self.normalize_contextual_arguments(
                                "modifyPlayerVariableAtIndex",
                                vec![variable, index, op_node, value],
                            );
                            let action =
                                self.push_call_action("modifyPlayerVariableAtIndex", &args);
                            self.mark_action_argument_origins(
                                action,
                                [target_span, index_span, None, value_span],
                            );
                            return Ok(action);
                        }
                        _ => {
                            return Err(self.unsupported(
                                format!(
                                    "{} requires a global or player variable receiver",
                                    function.id
                                ),
                                receiver.span().copied().or(span),
                            ));
                        }
                    }
                }
                _ => Err(self.unsupported(
                    format!(
                        "{} requires a global or player variable receiver",
                        function.id
                    ),
                    receiver.span().copied().or(span),
                )),
            };
        }

        let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
            self.unsupported(
                format!(
                    "member action '{}' has no canonical catalog identity",
                    function.id
                ),
                span,
            )
        })?;
        let argument_spans = std::iter::once(receiver.span().copied())
            .chain(args.iter().map(|arg| arg.span().copied()))
            .collect::<Vec<_>>();
        let mut lowered = Vec::with_capacity(args.len() + 1);
        lowered.push(self.lower_value(receiver)?);
        lowered.extend(
            args.iter()
                .map(|arg| self.lower_value(arg))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut args = self.normalize_contextual_arguments(catalog_id, lowered);
        self.apply_replacements(catalog_id, &mut args, span);
        Ok(self.push_call_action_with_spans(catalog_id.clone(), &args, argument_spans))
    }

    fn lower_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let span = expr.span().copied();
        let optimization = self.optimization_state_at(span.as_ref());
        if optimization.enabled
            && !optimization.strict
            && (matches!(expr, Expr::Binary { .. } | Expr::Unary { .. })
                || matches!(expr, Expr::Call { name, .. } if matches!(name.as_str(), "len" | "countOf")))
        {
            let bindings = HashMap::new();
            let mut stack = Vec::new();
            if let Some(value) =
                crate::compile_time::evaluate(expr, &self.constants, &bindings, &mut stack)
            {
                match value {
                    crate::compile_time::Value::Number(value) if value.is_finite() => {
                        return Ok(self.push_number(value, &computed_number_text(value)));
                    }
                    crate::compile_time::Value::String(value) => {
                        return self.lower_custom_string(value, span);
                    }
                    crate::compile_time::Value::Bool(value) => {
                        return Ok(self.push_value(Value::Bool(value)));
                    }
                    crate::compile_time::Value::Array(_)
                    | crate::compile_time::Value::Object(_) => {}
                    crate::compile_time::Value::Number(_) => {}
                }
            }
        }
        let value = match expr {
            Expr::Number { value, .. } => Value::Number(*value),
            Expr::String { value, .. } => {
                return self.lower_custom_string(value.clone(), span);
            }
            Expr::Bool { value, .. } => Value::Bool(*value),
            Expr::Null { .. } => Value::Null,
            Expr::Local { name, .. } => {
                let binding = self.array_bindings.iter().rev().find(|binding| {
                    binding.element == *name || binding.index.as_deref() == Some(name)
                });
                match binding {
                    Some(binding) if binding.element == *name => {
                        return Ok(self.push_call("currentArrayElement", Vec::new()));
                    }
                    Some(_) => return Ok(self.push_call("currentArrayIndex", Vec::new())),
                    None => {
                        return Err(self.unsupported(
                            format!("local '{name}' is not inside a supported array callback"),
                            span,
                        ));
                    }
                }
            }
            Expr::Type { .. } => {
                return Err(self.unsupported(
                    "type expressions are only valid as createWorkshopSetting type arguments",
                    span,
                ));
            }
            Expr::GlobalVar { name, .. } => {
                let id = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), span)
                })?;
                Value::GlobalVariable(self.global_names[id].clone())
            }
            Expr::PlayerVar { player, name, .. } => {
                let player = self.lower_value(player)?;
                let id = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), span)
                })?;
                Value::PlayerVariable {
                    player,
                    variable: self.player_names[id].clone(),
                }
            }
            Expr::EventPlayer { .. } => Value::EventPlayer,
            Expr::HostPlayer { .. } => Value::Call {
                name: "hostPlayer".to_string(),
                args: Vec::new(),
            },
            Expr::Enum {
                value_type, value, ..
            } => {
                let value = match (value_type.as_str(), value.as_str()) {
                    ("Clipping", "NONE") => "DO_NOT_CLIP",
                    ("Clipping", "SURFACES") => "CLIP_AGAINST_SURFACES",
                    _ => value,
                };
                if self
                    .compiler
                    .catalog
                    .enum_spelling(value_type, &Locale::new("en-US"), value)
                    .is_none()
                {
                    return Err(self.unsupported(
                        format!("unknown catalog enum member '{value_type}.{value}'"),
                        span,
                    ));
                }
                Value::Enum {
                    value_type: value_type.clone(),
                    value: value.to_string(),
                }
            }
            Expr::Array { elements, .. } => {
                let elements = elements
                    .iter()
                    .map(|element| self.lower_value(element))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.lower_array(elements, span);
            }
            Expr::Vector { x, y, z, .. } => {
                let x = self.lower_value(x)?;
                let y = self.lower_value(y)?;
                let z = self.lower_value(z)?;
                if let Some(member) = self.canonical_vector_member(x, y, z) {
                    Value::Enum {
                        value_type: "Vector".to_string(),
                        value: member.to_string(),
                    }
                } else {
                    Value::Call {
                        name: "vector".to_string(),
                        args: self.value_args(&[x, y, z]),
                    }
                }
            }
            Expr::Constant { name, .. } => {
                let const_expr = *self
                    .constants
                    .get(name)
                    .ok_or_else(|| self.unsupported(format!("unknown constant '{name}'"), span))?;
                return self.lower_value(const_expr);
            }
            Expr::Index { array, index, .. } => {
                if let Expr::Dict { entries, .. } = array.as_ref()
                    && is_literal_key(index)
                    && entries.iter().all(|entry| is_literal_key(&entry.key))
                {
                    if let Some(value) = entries
                        .iter()
                        .find(|entry| literal_key_matches(&entry.key, index))
                        .map(|entry| &entry.value)
                    {
                        return self.lower_value(value);
                    }
                    return Ok(self.push_value(Value::Null));
                }
                // The pinned OverPy oracle lowers a literal zero-index read
                // (`arr[0]`, `arr[0.0]`) to `firstOf(arr)`; non-zero indexes
                // and indexed writes keep the indexed forms.
                if matches!(index.as_ref(), Expr::Number { value, .. } if *value == 0.0) {
                    let array = self.lower_value(array)?;
                    Value::Call {
                        name: "firstOf".to_string(),
                        args: self.value_args(&[array]),
                    }
                } else {
                    let array = self.lower_value(array)?;
                    let index = self.lower_value(index)?;
                    Value::Call {
                        name: "valueInArray".to_string(),
                        args: self.value_args(&[array, index]),
                    }
                }
            }
            Expr::Format { text, args, .. } => {
                let (format_text, dynamic_args) = self.fold_format_constants(text, args);
                if dynamic_args.is_empty() {
                    let value = format_text;
                    return self.lower_custom_string(value, span);
                }
                if dynamic_args.len() <= 3 {
                    let text_node = self.push_value(Value::String(format_text));
                    let mut call_args = vec![text_node];
                    for arg in dynamic_args {
                        let arg = self.lower_value(arg)?;
                        call_args.push(arg);
                    }
                    Value::Call {
                        name: "customString".to_string(),
                        args: call_args,
                    }
                } else {
                    let chunks = split_format_chunks(&format_text, dynamic_args.len()).ok_or_else(|| {
                        self.unsupported(
                            "format strings with more than three replacements require sequential placeholders",
                            span,
                        )
                    })?;
                    let lowered_args = dynamic_args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut parts = Vec::with_capacity(chunks.len());
                    for (chunk, indices) in chunks {
                        let text = self.push_value(Value::String(chunk));
                        let mut call_args = vec![text];
                        call_args.extend(indices.into_iter().map(|index| lowered_args[index]));
                        let call_args = self.normalize_contextual_values("customString", call_args);
                        parts.push(self.push_value(Value::Call {
                            name: "customString".to_string(),
                            args: call_args,
                        }));
                    }
                    let separator = self.push_value(Value::String("{0}{1}".to_string()));
                    let mut value = parts[0];
                    for part in parts.into_iter().skip(1) {
                        value = self.push_call("customString", vec![separator, value, part]);
                    }
                    return Ok(value);
                }
            }
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } => Value::Call {
                name: "ifThenElse".to_string(),
                args: {
                    let condition = self.lower_value(condition)?;
                    let then_value = self.lower_value(then_value)?;
                    let else_value = self.lower_value(else_value)?;
                    self.value_args(&[condition, then_value, else_value])
                },
            },
            Expr::Binary {
                op, left, right, ..
            } => {
                if self.optimization_state_at(span.as_ref()).enabled
                    && matches!(op.as_str(), "in" | "not in")
                {
                    if let Expr::Array { elements, .. } = right.as_ref() {
                        if elements
                            .iter()
                            .any(|elem| literal_key_matches(elem, left.as_ref()))
                        {
                            return Ok(self.push_value(Value::Bool(op == "in")));
                        }
                        let strict = self.strict_optimization_active(expr);
                        if is_membership_literal(left.as_ref(), strict)
                            && elements
                                .iter()
                                .all(|elem| is_membership_literal(elem, strict))
                        {
                            return Ok(self.push_value(Value::Bool(op == "not in")));
                        }
                    }
                }
                let left = self.lower_value(left)?;
                let right = self.lower_value(right)?;
                if let Some(value) = self.fold_numeric_binary(op, left, right) {
                    Value::Number(value)
                } else {
                    match op.as_str() {
                        "==" | "!=" | "<" | "<=" | ">" | ">=" => Value::Call {
                            name: op.clone(),
                            args: self.value_args(&[left, right]),
                        },
                        "+" => Value::Call {
                            name: "add".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "-" => Value::Call {
                            name: "subtract".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "*" => Value::Call {
                            name: "multiply".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "/" => Value::Call {
                            name: "divide".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "%" => Value::Call {
                            name: "modulo".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "**" => Value::Call {
                            name: "raiseToPower".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "and" => Value::Call {
                            name: "and".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "or" => Value::Call {
                            name: "or".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "in" => Value::Call {
                            name: "arrayContains".to_string(),
                            args: self.value_args(&[right, left]),
                        },
                        "not in" => {
                            let contains = self.push_call("arrayContains", vec![right, left]);
                            Value::Call {
                                name: "not".to_string(),
                                args: self.value_args(&[contains]),
                            }
                        }
                        _ => {
                            return Err(self.unsupported(
                                format!(
                                    "binary operator '{op}' is not currently representable in canonical WIR"
                                ),
                                span,
                            ));
                        }
                    }
                }
            }
            Expr::Unary { op, operand, .. } => match op.as_str() {
                "not" => {
                    // The pinned OverPy 9.7.10 oracle lowers `not (a == b)`
                    // to the negated comparison (`a != b`), flipping every
                    // ordering comparison; `in` membership stays wrapped in
                    // `not`. Mirror that observable lowering.
                    if let Expr::Binary {
                        op: comparison,
                        left,
                        right,
                        ..
                    } = operand.as_ref()
                    {
                        if let Some(negated) = negated_comparison(comparison) {
                            let left = self.lower_value(left)?;
                            let right = self.lower_value(right)?;
                            Value::Call {
                                name: negated.to_string(),
                                args: self.value_args(&[left, right]),
                            }
                        } else {
                            let operand = self.lower_value(operand)?;
                            Value::Call {
                                name: "not".to_string(),
                                args: self.value_args(&[operand]),
                            }
                        }
                    } else {
                        let operand = self.lower_value(operand)?;
                        Value::Call {
                            name: "not".to_string(),
                            args: self.value_args(&[operand]),
                        }
                    }
                }
                "-" => {
                    let operand = self.lower_value(operand)?;
                    if let Value::Number(number) = self.value(operand) {
                        Value::Number(-number)
                    } else {
                        Value::Call {
                            name: "-".to_string(),
                            args: self.value_args(&[operand]),
                        }
                    }
                }
                "+" => return self.lower_value(operand),
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "unary operator '{op}' is not currently representable in canonical WIR"
                        ),
                        span,
                    ));
                }
            },
            Expr::Call { name, args, .. } => {
                if matches!(name.as_str(), "_" | "__" | "___") {
                    return self.lower_translation(name, args, span);
                }
                if name == "createWorkshopSetting" {
                    return self.lower_workshop_setting(args, span);
                }
                if matches!(name.as_str(), "_" | "__" | "___") {
                    return self.lower_translation(name, args, span);
                }
                if name == "buttonToString" {
                    let [button] = args.as_slice() else {
                        return Err(self.unsupported("buttonToString requires one button", span));
                    };
                    let button = self.lower_value(button)?;
                    return Ok(self.push_call("inputBindingString", vec![button]));
                }
                if matches!(
                    name.as_str(),
                    "getRealClosestPlayer"
                        | "getRealClosestPlayers"
                        | "getRealFarthestPlayer"
                        | "getRealFarthestPlayers"
                ) {
                    let [center, team] = args.as_slice() else {
                        return Err(
                            self.unsupported(format!("{name} requires center and team"), span)
                        );
                    };
                    let center = self.lower_value(center)?;
                    let team = self.lower_value(team)?;
                    let players = self.push_call("getLivingPlayers", vec![team]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let players = self.push_call("filteredArray", vec![players, spawned]);
                    let distance = self.push_call("distance", vec![current, center]);
                    let key = if matches!(
                        name.as_str(),
                        "getRealFarthestPlayer" | "getRealFarthestPlayers"
                    ) {
                        let negative_one = self.push_number(-1.0, "-1");
                        self.push_call("multiply", vec![negative_one, distance])
                    } else {
                        distance
                    };
                    let sorted = self.push_call("sortedArray", vec![players, key]);
                    return if matches!(
                        name.as_str(),
                        "getRealClosestPlayer" | "getRealFarthestPlayer"
                    ) {
                        Ok(self.push_call("firstOf", vec![sorted]))
                    } else {
                        Ok(sorted)
                    };
                }
                if name == "getRealPlayersInRadius" {
                    let lowered = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let players = self.push_call("getPlayersInRadius", lowered);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let alive = self.push_call("isAlive", vec![current]);
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let condition = self.push_call("and", vec![alive, spawned]);
                    return Ok(self.push_call("filteredArray", vec![players, condition]));
                }
                if name == "lineIntersectsSphere" {
                    let [line_start, line_direction, sphere_center, sphere_radius] =
                        args.as_slice()
                    else {
                        return Err(
                            self.unsupported("lineIntersectsSphere requires four arguments", span)
                        );
                    };
                    let line_start = self.lower_value(line_start)?;
                    let line_direction = self.lower_value(line_direction)?;
                    let sphere_center = self.lower_value(sphere_center)?;
                    let sphere_radius = self.lower_value(sphere_radius)?;
                    let center_direction =
                        self.push_call("subtract", vec![sphere_center, line_start]);
                    let angle = self.push_call(
                        "angleBetweenVectors",
                        vec![line_direction, center_direction],
                    );
                    let distance = self.push_call("distance", vec![line_start, sphere_center]);
                    let ratio = self.push_call("divide", vec![sphere_radius, distance]);
                    let limit = self.push_call("asinDeg", vec![ratio]);
                    return Ok(self.push_call("<=", vec![angle, limit]));
                }
                if name == "arrayToString" {
                    let (array, max_length) = match args.as_slice() {
                        [array] => (array, 12),
                        [array, Expr::Number { value, .. }] => {
                            if !value.is_finite() || *value < 0.0 || value.fract() != 0.0 {
                                return Err(self.unsupported(
                                    "arrayToString maxLength must be a non-negative integer literal",
                                    span,
                                ));
                            }
                            (array, (*value).min(1000.0) as usize)
                        }
                        _ => return Err(self.unsupported("arrayToString requires an array", span)),
                    };
                    let array = self.lower_value(array)?;
                    return Ok(self.lower_debug_array_text(array, max_length));
                }
                if matches!(name.as_str(), "decompressNumbers" | "decompressVectors") {
                    let [text] = args.as_slice() else {
                        return Err(self.unsupported(format!("{name} requires one string"), span));
                    };
                    return self.lower_decompression(text, name == "decompressVectors", span);
                }
                if name == "strVisualLength" {
                    let [Expr::String { value, .. }] = args.as_slice() else {
                        return Err(
                            self.unsupported("strVisualLength requires one literal string", span)
                        );
                    };
                    let width = value.chars().map(blizzard_global::width).sum::<i32>();
                    return Ok(self.push_number(width as f64, ""));
                }
                if name == "spacesForLength" {
                    let [Expr::Number { value, .. }] = args.as_slice() else {
                        return Err(
                            self.unsupported("spacesForLength requires one literal number", span)
                        );
                    };
                    if !value.is_finite() || *value < 0.0 || value.fract() != 0.0 {
                        return Err(self.unsupported(
                            "spacesForLength requires a non-negative integer literal",
                            span,
                        ));
                    }
                    return self.lower_custom_string(blizzard_global::spaces(*value as i32), span);
                }
                if name == "spacesForString" {
                    let [Expr::String { value, .. }] = args.as_slice() else {
                        if let [
                            Expr::Call {
                                name: translation,
                                args: translation_args,
                                ..
                            },
                        ] = args.as_slice()
                            && matches!(translation.as_str(), "_" | "__" | "___")
                            && let Some(text) = translation_args.last()
                            && let Expr::String {
                                value,
                                span: text_span,
                            } = text
                        {
                            let replacement = Expr::String {
                                value: blizzard_global::spaces(
                                    value.chars().map(blizzard_global::width).sum(),
                                ),
                                span: *text_span,
                            };
                            let mut translated_args = translation_args.clone();
                            *translated_args.last_mut().expect("translation text exists") =
                                replacement;
                            return self.lower_value(&Expr::Call {
                                name: translation.clone(),
                                args: translated_args,
                                debug_source: None,
                                span,
                            });
                        }
                        return Err(
                            self.unsupported("spacesForString requires one literal string", span)
                        );
                    };
                    return self.lower_custom_string(
                        blizzard_global::spaces(value.chars().map(blizzard_global::width).sum()),
                        span,
                    );
                }
                if name == "hsl" {
                    let (hue, saturation, lightness, alpha) = match args.as_slice() {
                        [hue, saturation, lightness] => (hue, saturation, lightness, None),
                        [hue, saturation, lightness, alpha] => {
                            (hue, saturation, lightness, Some(alpha))
                        }
                        _ => {
                            return Err(
                                self.unsupported("hsl requires three or four arguments", span)
                            );
                        }
                    };
                    let hue = self.lower_value(hue)?;
                    let saturation = self.lower_value(saturation)?;
                    let lightness = self.lower_value(lightness)?;
                    let alpha = match alpha {
                        Some(alpha) => self.lower_value(alpha)?,
                        None => self.push_number(255.0, "255"),
                    };
                    let one = self.push_number(1.0, "1");
                    let thirty = self.push_number(30.0, "30");
                    let hue_thirtieths = self.push_call("divide", vec![hue, thirty]);
                    let lightness_complement = self.push_call("subtract", vec![one, lightness]);
                    let lightness_limit =
                        self.push_call("min", vec![lightness, lightness_complement]);
                    let channel = |this: &mut Self, offset: f64| {
                        let offset = this.push_number(offset, "");
                        let phase = this.push_call("add", vec![offset, hue_thirtieths]);
                        let twelve = this.push_number(12.0, "12");
                        let phase = this.push_call("modulo", vec![phase, twelve]);
                        let three = this.push_number(3.0, "3");
                        let lower = this.push_call("subtract", vec![phase, three]);
                        let nine = this.push_number(9.0, "9");
                        let upper = this.push_call("subtract", vec![nine, phase]);
                        let clamped = this.push_call("min", vec![lower, upper]);
                        let negative_one = this.push_number(-1.0, "-1");
                        let clamped = this.push_call("max", vec![clamped, negative_one]);
                        let saturation_limit =
                            this.push_call("multiply", vec![saturation, lightness_limit]);
                        let adjustment =
                            this.push_call("multiply", vec![saturation_limit, clamped]);
                        let value = this.push_call("subtract", vec![lightness, adjustment]);
                        let scale = this.push_number(255.0, "255");
                        this.push_call("multiply", vec![scale, value])
                    };
                    let red = channel(self, 0.0);
                    let green = channel(self, 8.0);
                    let blue = channel(self, 4.0);
                    return Ok(self.push_call("customColor", vec![red, green, blue, alpha]));
                }
                if name == "timeToString" {
                    let [time] = args.as_slice() else {
                        return Err(self.unsupported("timeToString requires one argument", span));
                    };
                    let time = self.lower_value(time)?;
                    let three_thousand_six_hundred = self.push_number(3600.0, "3600");
                    let sixty = self.push_number(60.0, "60");
                    let hour_value =
                        self.push_call("divide", vec![time, three_thousand_six_hundred]);
                    let down = self.push_value(Value::Enum {
                        value_type: "Rounding".to_string(),
                        value: "DOWN".to_string(),
                    });
                    let hour = self.push_call("roundToInteger", vec![hour_value, down]);
                    let minute_remainder =
                        self.push_call("modulo", vec![time, three_thousand_six_hundred]);
                    let minute_value = self.push_call("divide", vec![minute_remainder, sixty]);
                    let minute = self.push_call("roundToInteger", vec![minute_value, down]);
                    let second = self.push_call("modulo", vec![time, sixty]);
                    let hundred = self.push_number(100.0, "100");
                    let first_digit = self.push_number(1.0, "1");
                    let two = self.push_number(2.0, "2");
                    let minute_with_padding = self.push_call("add", vec![minute, hundred]);
                    let padding_template = self.push_value(Value::String("{0}".to_string()));
                    let minute_with_padding =
                        self.push_call("customString", vec![padding_template, minute_with_padding]);
                    let minute_text =
                        self.push_call("stringSlice", vec![minute_with_padding, first_digit, two]);
                    let second_with_padding = self.push_call("add", vec![second, hundred]);
                    let second_with_padding =
                        self.push_call("customString", vec![padding_template, second_with_padding]);
                    let all_digits = self.push_number(9999.0, "9999");
                    let second_text = self.push_call(
                        "stringSlice",
                        vec![second_with_padding, first_digit, all_digits],
                    );
                    let template = self.push_value(Value::String("{0}:{1}:{2}".to_string()));
                    return Ok(self.push_call(
                        "customString",
                        vec![template, hour, minute_text, second_text],
                    ));
                }
                if name == "compressed" {
                    return self.lower_compressed(args, span);
                }
                if name == "compress" {
                    return self.lower_compress(args, span);
                }
                if name == "getSign" {
                    let [number] = args.as_slice() else {
                        return Err(self.unsupported("getSign requires one argument", span));
                    };
                    let number = self.lower_value(number)?;
                    let zero = self.push_number(0.0, "0");
                    let positive = self.push_call(">", vec![number, zero]);
                    let one = self.push_number(1.0, "1");
                    let negative_one = self.push_number(-1.0, "-1");
                    let sign = self.push_call("ifThenElse", vec![positive, one, negative_one]);
                    let is_zero = self.push_call("==", vec![number, zero]);
                    return Ok(self.push_call("ifThenElse", vec![is_zero, zero, sign]));
                }
                if name == "lerp" {
                    let [start, end, t] = args.as_slice() else {
                        return Err(self.unsupported("lerp requires three arguments", span));
                    };
                    let start = self.lower_value(start)?;
                    let end = self.lower_value(end)?;
                    let t = self.lower_value(t)?;
                    let one = self.push_number(1.0, "1");
                    let weight = self.push_call("subtract", vec![one, t]);
                    let start_part = self.push_call("multiply", vec![start, weight]);
                    let end_part = self.push_call("multiply", vec![end, t]);
                    return Ok(self.push_call("add", vec![start_part, end_part]));
                }
                if name == "log" {
                    let (number, base) = match args.as_slice() {
                        [number] => (number, None),
                        [number, base] => (number, Some(base)),
                        _ => {
                            return Err(self.unsupported("log requires one or two arguments", span));
                        }
                    };
                    let number = self.lower_value(number)?;
                    let exponent = self.push_number(0.0001, "0.0001");
                    let powered = self.push_call("raiseToPower", vec![number, exponent]);
                    let one = self.push_number(1.0, "1");
                    let delta = self.push_call("subtract", vec![powered, one]);
                    let scale = self.push_number(10000.0, "10000");
                    let approximation = self.push_call("multiply", vec![scale, delta]);
                    if let Some(base) = base {
                        let base = self.lower_value(base)?;
                        let base_powered = self.push_call("raiseToPower", vec![base, exponent]);
                        let base_one = self.push_number(1.0, "1");
                        let base_delta = self.push_call("subtract", vec![base_powered, base_one]);
                        let base_scale = self.push_number(10000.0, "10000");
                        let base_log = self.push_call("multiply", vec![base_scale, base_delta]);
                        return Ok(self.push_call("divide", vec![approximation, base_log]));
                    }
                    return Ok(approximation);
                }
                if matches!(name.as_str(), "attacker" | "victim") && args.is_empty() {
                    return Ok(self.push_call(name, Vec::new()));
                }
                if name == "localPlayer" && args.is_empty() {
                    return Ok(self.push_call(name, Vec::new()));
                }
                if name == "ruleCondition" {
                    if !args.is_empty() {
                        return Err(
                            self.unsupported("ruleCondition does not accept arguments", span)
                        );
                    }
                    let conditions = self.current_rule_conditions.clone().ok_or_else(|| {
                        self.unsupported("ruleCondition is only valid inside a rule", span)
                    })?;
                    let Some((first, rest)) = conditions.split_first() else {
                        return Ok(self.push_value(Value::Bool(true)));
                    };
                    let mut combined = *first;
                    for condition in rest {
                        combined = self.push_call("and", vec![combined, *condition]);
                    }
                    return Ok(combined);
                }
                if name == "vect" && args.len() == 3 {
                    let x = self.lower_value(&args[0])?;
                    let y = self.lower_value(&args[1])?;
                    let z = self.lower_value(&args[2])?;
                    if let Some(member) = self.canonical_vector_member(x, y, z) {
                        Value::Enum {
                            value_type: "Vector".to_string(),
                            value: member.to_string(),
                        }
                    } else {
                        Value::Vector { x, y, z }
                    }
                } else if matches!(
                    name.as_str(),
                    "createWorkshopSettingBool"
                        | "createWorkshopSettingEnum"
                        | "createWorkshopSettingInt"
                        | "createWorkshopSettingFloat"
                ) {
                    let mut lowered = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    if name == "createWorkshopSettingFloat" && lowered.len() == 5 {
                        lowered.push(self.push_number(0.0, "0"));
                    }
                    Value::Call {
                        name: match name.as_str() {
                            "createWorkshopSettingBool" => "workshopSettingToggle",
                            "createWorkshopSettingEnum" => "workshopSettingCombo",
                            "createWorkshopSettingInt" => "workshopSettingInteger",
                            _ => name,
                        }
                        .to_string(),
                        args: self.value_args(&lowered),
                    }
                } else if matches!(name.as_str(), "all" | "any") {
                    let call_name = if name == "all" {
                        "isTrueForAll"
                    } else {
                        "isTrueForAny"
                    };
                    let [array] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one array argument"),
                            span,
                        ));
                    };
                    let (array, condition) = match array {
                        Expr::Comprehension {
                            element,
                            variable,
                            index,
                            iterable,
                            ..
                        } => {
                            if index.is_some() {
                                return Err(self.unsupported(
                                    format!("{name} does not support an index binder"),
                                    span,
                                ));
                            }
                            let iterable = self.lower_value(iterable)?;
                            self.array_bindings.push(ArrayBinding {
                                element: variable.clone(),
                                index: None,
                            });
                            let condition = self.lower_value(element);
                            self.array_bindings.pop();
                            (iterable, condition?)
                        }
                        array => (
                            self.lower_value(array)?,
                            self.push_call("currentArrayElement", Vec::new()),
                        ),
                    };
                    Value::Call {
                        name: call_name.to_string(),
                        args: self.value_args(&[array, condition]),
                    }
                } else if matches!(name.as_str(), "ceil" | "floor" | "round") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one numeric argument"),
                            span,
                        ));
                    };
                    let rounding = match name.as_str() {
                        "ceil" => "UP",
                        "floor" => "DOWN",
                        "round" => "NEAREST",
                        _ => unreachable!(),
                    };
                    let rounding = self.push_value(Value::Enum {
                        value_type: "Rounding".to_string(),
                        value: rounding.to_string(),
                    });
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: "roundToInteger".to_string(),
                        args: self.value_args(&[value, rounding]),
                    }
                } else if name == "sorted" {
                    let (array, key) = match args.as_slice() {
                        [array] => (
                            self.lower_value(array)?,
                            self.push_call("currentArrayElement", Vec::new()),
                        ),
                        [
                            array,
                            Expr::Lambda {
                                params, body, span, ..
                            },
                        ] => {
                            let array = self.lower_value(array)?;
                            let key = self.lower_array_callback(params, body, *span)?;
                            (array, key)
                        }
                        _ => {
                            return Err(self.unsupported(
                                "sorted requires an array and an optional lambda key",
                                span,
                            ));
                        }
                    };
                    Value::Call {
                        name: "sortedArray".to_string(),
                        args: self.value_args(&[array, key]),
                    }
                } else {
                    let function = self
                        .compiler
                        .manifest
                        .resolve_function(name)
                        .ok_or_else(|| self.unsupported(format!("unknown value '{name}'"), span))?;
                    if !matches!(function.kind, FunctionKind::Value) {
                        return Err(
                            self.unsupported(format!("'{name}' is not a generic OPY value"), span)
                        );
                    }
                    let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                        self.unsupported(
                            format!(
                                "value '{}' requires a special lowering not in #46",
                                function.id
                            ),
                            span,
                        )
                    })?;
                    if function.id == "getAllPlayers" {
                        return Ok(self.lower_all_players());
                    }
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    Value::Call {
                        name: catalog_id.clone(),
                        args: self.value_args(&lowered_args),
                    }
                }
            }
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                if name == "getOppositeTeam" {
                    if !args.is_empty() {
                        return Err(self.unsupported("getOppositeTeam requires no arguments", span));
                    }
                    let receiver = self.lower_value(receiver)?;
                    let team = self.push_call("teamOf", vec![receiver]);
                    return Ok(self.push_call("oppositeTeamOf", vec![team]));
                }
                if name == "toArray" {
                    if !args.is_empty() {
                        return Err(self.unsupported("toArray requires no arguments", span));
                    }
                    let Expr::Type {
                        name: type_name, ..
                    } = receiver.as_ref()
                    else {
                        return Err(
                            self.unsupported("toArray requires an enum type receiver", span)
                        );
                    };
                    let domain_name = match type_name.as_str() {
                        "Clip" => "Clipping",
                        _ => type_name.as_str(),
                    };
                    let Some(domain) = self.compiler.catalog.enum_domain(domain_name) else {
                        return Err(
                            self.unsupported(format!("unknown enum type '{type_name}'"), span)
                        );
                    };
                    let values = domain
                        .members
                        .iter()
                        .map(|member| {
                            self.push_value(Value::Enum {
                                value_type: domain_name.to_string(),
                                value: member.member.clone(),
                            })
                        })
                        .collect();
                    return Ok(self.push_call("array", values));
                }
                if matches!(name.as_str(), "all" | "any") {
                    let receiver = self.lower_value(receiver)?;
                    let condition = match args.as_slice() {
                        [] => self.push_call("currentArrayElement", Vec::new()),
                        [
                            Expr::Lambda {
                                params, body, span, ..
                            },
                        ] => self.lower_array_callback(params, body, *span)?,
                        _ => {
                            return Err(self.unsupported(
                                format!("{name} requires zero or one lambda argument"),
                                span,
                            ));
                        }
                    };
                    let args = self.value_args(&[receiver, condition]);
                    return Ok(self.push_value(Value::Call {
                        name: if name == "all" {
                            "isTrueForAll"
                        } else {
                            "isTrueForAny"
                        }
                        .to_string(),
                        args,
                    }));
                }
                let function = self.compiler.manifest.resolve_member(name).ok_or_else(|| {
                    self.unsupported(format!("unknown member value '{name}'"), span)
                })?;
                if !matches!(function.kind, FunctionKind::MemberValue) {
                    return Err(self.unsupported(format!("'{name}' is not a member value"), span));
                }
                if function.id == "unique" {
                    if !args.is_empty() {
                        return Err(self.unsupported("unique requires no arguments", span));
                    }
                    let receiver = self.lower_value(receiver)?;
                    let current_element = self.push_call("currentArrayElement", Vec::new());
                    let first_index =
                        self.push_call("indexOfArrayValue", vec![receiver, current_element]);
                    let current_index = self.push_call("currentArrayIndex", Vec::new());
                    let condition = self.push_call("==", vec![first_index, current_index]);
                    return Ok(self.push_call("filteredArray", vec![receiver, condition]));
                }
                if function.id == "reverse" {
                    if !args.is_empty() {
                        return Err(self.unsupported("reverse requires no arguments", span));
                    }
                    let receiver = self.lower_value(receiver)?;
                    let index = self.push_call("currentArrayIndex", Vec::new());
                    let key = self.push_call("-", vec![index]);
                    return Ok(self.push_call("sortedArray", vec![receiver, key]));
                }
                if function.id == "getEffectiveHero" {
                    if !args.is_empty() {
                        return Err(
                            self.unsupported("getEffectiveHero requires no arguments", span)
                        );
                    }
                    let receiver = self.lower_value(receiver)?;
                    let duplicated = self.push_call("getHeroOfDuplication", vec![receiver]);
                    let hero = self.push_call("getHero", vec![receiver]);
                    let null = self.push_value(Value::Null);
                    let condition = self.push_call("==", vec![duplicated, null]);
                    return Ok(self.push_call("ifThenElse", vec![condition, hero, duplicated]));
                }
                if function.id == "getRealPlayersInViewAngle" {
                    let [team, view_angle] = args.as_slice() else {
                        return Err(self.unsupported(
                            "getRealPlayersInViewAngle requires team and view angle",
                            span,
                        ));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let team = self.lower_value(team)?;
                    let view_angle = self.lower_value(view_angle)?;
                    let players =
                        self.push_call("getPlayersInViewAngle", vec![receiver, team, view_angle]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let alive = self.push_call("isAlive", vec![current]);
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let condition = self.push_call("and", vec![alive, spawned]);
                    return Ok(self.push_call("filteredArray", vec![players, condition]));
                }
                if matches!(
                    function.id.as_str(),
                    "getRealPlayerClosestToReticle" | "getRealPlayersClosestToReticle"
                ) {
                    let [team] = args.as_slice() else {
                        return Err(self
                            .unsupported("getRealPlayersClosestToReticle requires a team", span));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let team = self.lower_value(team)?;
                    let players = self.push_call("getLivingPlayers", vec![team]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let not_self = self.push_call("!=", vec![current, receiver]);
                    let condition = self.push_call("and", vec![spawned, not_self]);
                    let players = self.push_call("filteredArray", vec![players, condition]);
                    let facing = self.push_call("getFacingDirection", vec![receiver]);
                    let eye_position = self.push_call("getEyePosition", vec![receiver]);
                    let direction = self.push_call("subtract", vec![current, eye_position]);
                    let angle = self.push_call("angleBetweenVectors", vec![facing, direction]);
                    let sorted = self.push_call("sortedArray", vec![players, angle]);
                    return if function.id == "getRealPlayerClosestToReticle" {
                        Ok(self.push_call("firstOf", vec![sorted]))
                    } else {
                        Ok(sorted)
                    };
                }
                if function.id == "map" {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(self.unsupported("map requires one lambda argument", span));
                    };
                    let mapped = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    return Ok(self.push_call("mappedArray", vec![receiver, mapped]));
                }
                if matches!(
                    function.id.as_str(),
                    "getHitPosition" | "getPlayerHit" | "getNormal"
                ) {
                    let member_name = function.id.as_str();
                    let Expr::Call {
                        name: receiver_name,
                        args: receiver_args,
                        ..
                    } = receiver.as_ref()
                    else {
                        return Err(self.unsupported(
                            format!("{member_name} requires a raycast receiver"),
                            span,
                        ));
                    };
                    if receiver_name != "raycast" || !args.is_empty() {
                        return Err(self.unsupported(
                            format!("{member_name} requires raycast(...) with no member arguments"),
                            span,
                        ));
                    }
                    let catalog_id = function.catalog_id.clone().ok_or_else(|| {
                        self.unsupported(
                            format!("{member_name} has no canonical catalog identity"),
                            span,
                        )
                    })?;
                    let lowered_args = receiver_args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(self.push_value(Value::Call {
                        name: catalog_id,
                        args: self.value_args(&lowered_args),
                    }));
                }
                if function.id == "filter" {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(self.unsupported("filter requires one lambda argument", span));
                    };
                    let condition = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    Value::Call {
                        name: "filteredArray".to_string(),
                        args: self.value_args(&[receiver, condition]),
                    }
                } else if matches!(function.id.as_str(), "concat" | "exclude") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{} requires exactly one argument", function.id),
                            span,
                        ));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: if function.id == "concat" {
                            "appendToArray"
                        } else {
                            "removeFromArray"
                        }
                        .to_string(),
                        args: self.value_args(&[receiver, value]),
                    }
                } else {
                    let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                        self.unsupported(
                            format!(
                                "member value '{}' has no canonical catalog identity",
                                function.id
                            ),
                            span,
                        )
                    })?;
                    let mut lowered = Vec::with_capacity(args.len() + 1);
                    lowered.push(self.lower_value(receiver)?);
                    lowered.extend(
                        args.iter()
                            .map(|arg| self.lower_value(arg))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    Value::Call {
                        name: catalog_id.clone(),
                        args: self.value_args(&lowered),
                    }
                }
            }
            Expr::Member {
                receiver, member, ..
            } => {
                let receiver = self.lower_value(receiver)?;
                if let Some(name) = match member.as_str() {
                    "x" => Some("__xComponentOf__"),
                    "y" => Some("__yComponentOf__"),
                    "z" => Some("__zComponentOf__"),
                    _ => None,
                } {
                    Value::Call {
                        name: name.to_string(),
                        args: self.value_args(&[receiver]),
                    }
                } else {
                    let member = self.push_value(Value::String(member.clone()));
                    Value::Call {
                        name: "memberAccess".to_string(),
                        args: self.value_args(&[receiver, member]),
                    }
                }
            }
            Expr::Comprehension {
                element,
                variable,
                index,
                iterable,
                condition,
                span: comprehension_span,
                ..
            } => {
                if condition.is_some() && index.is_some() {
                    return Err(self.unsupported(
                        "comprehensions with both a filter and an index binder are not currently representable in canonical WIR",
                        *comprehension_span,
                    ));
                }
                let iterable = self.lower_value(iterable)?;
                let iterable = if self.value_is_known_player(iterable) {
                    self.push_call("array", vec![iterable])
                } else {
                    iterable
                };
                let binding = ArrayBinding {
                    element: variable.clone(),
                    index: index.clone(),
                };
                self.array_bindings.push(binding);
                let predicate = condition
                    .as_deref()
                    .map(|condition| self.lower_value(condition));
                let element = self.lower_value(element);
                self.array_bindings.pop();
                let element = element?;
                let iterable = if let Some(predicate) = predicate {
                    let predicate = predicate?;
                    let filtered = self.push_call("filteredArray", vec![iterable, predicate]);
                    let optimization = self.optimization_state_at(comprehension_span.as_ref());
                    if optimization.enabled {
                        self.optimized_nodes.insert(filtered, optimization.strict);
                    }
                    filtered
                } else {
                    iterable
                };
                Value::Call {
                    name: "mappedArray".to_string(),
                    args: self.value_args(&[iterable, element]),
                }
            }
            Expr::Lambda { span, .. } => {
                return Err(self.unsupported(
                    "lambda expressions are only representable as supported array operation arguments",
                    *span,
                ));
            }
            Expr::StringModifier {
                modifier,
                value,
                span,
            } => {
                let value = match modifier.as_str() {
                    "b" => big_letters(value),
                    "c" => case_sensitive(value),
                    "w" => fullwidth(value),
                    _ => {
                        return Err(self.unsupported(
                            format!(
                                "string modifier '{modifier}' is not currently representable in canonical WIR"
                            ),
                            *span,
                        ));
                    }
                };
                return self.lower_custom_string(value, *span);
            }
            _ => {
                return Err(self.unsupported(
                    format!(
                        "expression '{}' is not currently representable in canonical WIR",
                        expr.kind_name()
                    ),
                    span,
                ));
            }
        };
        let value_id = self.push_value(value);
        let Some(Value::Call { name, args }) = self.values.get(value_id) else {
            return Ok(value_id);
        };
        let name = name.clone();
        let args = args.clone();
        let mut args = self.normalize_contextual_values(&name, args);
        self.apply_replacements_to_values(&name, &mut args, span);
        if let Some(Value::Call {
            args: target_args, ..
        }) = self.values.get_mut(value_id)
        {
            *target_args = args;
        }
        if optimization.enabled
            && matches!(
                expr,
                Expr::Binary { .. }
                    | Expr::Unary { .. }
                    | Expr::Conditional { .. }
                    | Expr::Index { .. }
                    | Expr::Comprehension { .. }
            )
        {
            self.optimized_nodes.insert(value_id, optimization.strict);
        }
        Ok(value_id)
    }

    fn apply_replacements_to_values(
        &mut self,
        call_id: &str,
        args: &mut [ValueId],
        span: Option<HirSpan>,
    ) {
        for (index, value) in args.iter_mut().enumerate() {
            *value = self.apply_replacement(*value, call_id, index, span);
        }
    }

    fn apply_replacements(&mut self, call_id: &str, args: &mut [ValueId], span: Option<HirSpan>) {
        self.apply_replacements_to_values(call_id, args, span);
    }

    fn apply_replacement(
        &mut self,
        value_id: ValueId,
        call_id: &str,
        _arg_index: usize,
        span: Option<HirSpan>,
    ) -> ValueId {
        let optimization = self.optimization_state_at(span.as_ref());
        if !optimization.enabled
            || !optimization.for_size
            || matches!(
                call_id,
                "workshopSettingToggle"
                    | "workshopSettingCombo"
                    | "workshopSettingInteger"
                    | "workshopSettingFloat"
            )
        {
            return value_id;
        }
        let replacement = |name: &str, hir: &hir::Program| {
            hir.preprocessing
                .replacements
                .iter()
                .find(|value| value.value == name)
                .is_some()
        };
        match self.value(value_id).clone() {
            Value::Number(0.0) => {
                let name = [
                    "getCapturePercentage",
                    "getPayloadProgressPercentage",
                    "isMatchComplete",
                ]
                .into_iter()
                .find(|name| replacement(name, self.hir));
                name.map_or(value_id, |name| self.push_call(name, Vec::new()))
            }
            Value::Number(1.0) => {
                if replacement("getMatchRound", self.hir) {
                    self.push_call("getMatchRound", Vec::new())
                } else {
                    value_id
                }
            }
            Value::Enum { value_type, value } if value_type == "Team" && value == "TEAM_1" => {
                if replacement("getControlScoringTeam", self.hir) {
                    self.push_call("getControlScoringTeam", Vec::new())
                } else {
                    value_id
                }
            }
            Value::String(value) if value.is_empty() => {
                if replacement("emptyArray", self.hir) {
                    self.push_call("emptyArray", Vec::new())
                } else if replacement("variable", self.hir) {
                    self.push_value(Value::GlobalVariable(EMPTY_STRING_NAME.to_string()))
                } else {
                    value_id
                }
            }
            Value::Call { name, args }
                if name == "customString"
                    && args.len() == 1
                    && self.value_is_empty_string(args[0]) =>
            {
                if replacement("emptyArray", self.hir) {
                    self.push_call("emptyArray", Vec::new())
                } else if replacement("variable", self.hir) {
                    self.push_value(Value::GlobalVariable(EMPTY_STRING_NAME.to_string()))
                } else {
                    value_id
                }
            }
            _ => value_id,
        }
    }

    fn lower_compressed(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        self.lower_compressed_mode(args, span, true)
    }

    fn lower_decompression(
        &mut self,
        text: &Expr,
        is_vector: bool,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let text = self.lower_value(text)?;
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        let split = self.push_call("stringSplit", vec![text, separator]);
        let alphabet = if has_directive(self.hir, "useVariableForCompressionAlphabet") {
            let variable = *self
                .globals
                .get(COMPRESSION_ALPHABET_NAME)
                .expect("compression alphabet variable is created");
            self.push_value(Value::GlobalVariable(self.global_names[variable].clone()))
        } else {
            self.lower_custom_string(compression_alphabet(), span)?
        };
        let decoded = if has_directive(self.hir, "useVariableForCompressionAlphabet") {
            split
        } else {
            let current = self.push_call("currentArrayElement", Vec::new());
            let alphabet = self.push_call("appendToArray", vec![current, alphabet]);
            self.push_call("mappedArray", vec![split, alphabet])
        };
        let width = if is_vector { 3 } else { 4 };
        let min_decimal_place = if is_vector { -2.0 } else { -3.0 };
        let offset = if is_vector { 5000.0 } else { 50000.0 };
        let component = |this: &mut Self, component_offset: usize| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let mut terms = Vec::with_capacity(width);
            for index in 0..width {
                let position = this.push_number((index + component_offset) as f64, "");
                let character = this.push_call("charAt", vec![current, position]);
                let formula_alphabet =
                    if has_directive(this.hir, "useVariableForCompressionAlphabet") {
                        alphabet
                    } else {
                        this.push_call("lastOf", vec![current])
                    };
                let digit = this.push_call("strIndex", vec![formula_alphabet, character]);
                let power = 100_f64.powf(index as f64 + min_decimal_place / 2.0);
                let power = this.push_number(power, "");
                terms.push(this.push_call("multiply", vec![power, digit]));
            }
            let mut value = terms
                .first()
                .copied()
                .unwrap_or_else(|| this.push_number(0.0, ""));
            for term in terms.into_iter().skip(1) {
                value = this.push_call("add", vec![value, term]);
            }
            let offset = this.push_number(offset, "");
            this.push_call("subtract", vec![value, offset])
        };
        if is_vector {
            let x = component(self, 0);
            let y = component(self, width * 2);
            let z = component(self, width);
            let vector = self.push_call("vector", vec![x, y, z]);
            Ok(self.push_call("mappedArray", vec![decoded, vector]))
        } else {
            let number = component(self, 0);
            Ok(self.push_call("mappedArray", vec![decoded, number]))
        }
    }

    fn lower_compressed_mode(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
        decode: bool,
    ) -> Result<ValueId, IntegrationError> {
        let [Expr::Array { elements, .. }] = args else {
            return Err(self.unsupported(
                "compressed requires one literal array of numbers or vectors",
                span,
            ));
        };
        if elements.is_empty() {
            return Err(self.unsupported("cannot compress an empty array", span));
        }

        let Some(numbers) = elements
            .iter()
            .map(|element| match element {
                Expr::Null { .. } => Some(vec![0.0]),
                Expr::Number { value, .. } => Some(vec![*value]),
                Expr::Unary { op, operand, .. } if matches!(op.as_str(), "+" | "-") => {
                    literal_number(operand)
                        .map(|value| vec![if op == "-" { -value } else { value }])
                }
                Expr::Vector { x, y, z, .. } => Some(vec![
                    literal_number(x)?,
                    literal_number(y)?,
                    literal_number(z)?,
                ]),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(self.unsupported("compressed requires literal numbers or vectors", span));
        };
        let is_vector = numbers.first().is_some_and(|value| value.len() == 3);
        if numbers.iter().any(|value| (value.len() == 3) != is_vector) {
            return Err(self.unsupported("compressed cannot mix numbers and vectors", span));
        }
        let flattened = numbers.iter().flatten().copied().collect::<Vec<_>>();
        let limit = if is_vector { 4999.0 } else { 49999.0 };
        if flattened.iter().any(|value| value.abs() >= limit) {
            return Err(self.unsupported("compressed values exceed the supported magnitude", span));
        }

        let max_decimals = if is_vector { 2 } else { 3 };
        let compression_offset = if decode {
            flattened.iter().copied().fold(0.0_f64, f64::min).min(0.0)
        } else if is_vector {
            -5000.0
        } else {
            -50000.0
        };
        let adjusted = flattened
            .iter()
            .map(|value| value - compression_offset)
            .collect::<Vec<_>>();
        let mut strings = adjusted
            .iter()
            .map(|value| {
                format!("{value:.precision$}", precision = max_decimals)
                    .replace('.', "")
                    .chars()
                    .rev()
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let mut min_decimal_place = -(max_decimals as i32);
        if decode {
            while strings.iter().all(|value| value.starts_with('0')) {
                for value in &mut strings {
                    value.remove(0);
                }
                min_decimal_place += 1;
            }
        } else {
            min_decimal_place = if is_vector { -2 } else { -3 };
        }
        let max_decimal_place = if decode {
            min_decimal_place + strings.iter().map(String::len).max().unwrap_or_default() as i32
        } else if is_vector {
            4
        } else {
            5
        };
        for value in &mut strings {
            let trimmed = value.trim_end_matches('0');
            *value = if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            };
        }

        let alphabet = compression_alphabet_chars();
        let encode = |value: &str| -> Option<String> {
            let mut encoded = String::new();
            let chars = value.as_bytes();
            for pair in chars.chunks(2) {
                let number = if pair.len() == 1 {
                    u16::from(pair[0] - b'0')
                } else {
                    u16::from(pair[1] - b'0') * 10 + u16::from(pair[0] - b'0')
                };
                encoded.push(*alphabet.get(number as usize)?);
            }
            Some(encoded)
        };
        let compressed = if is_vector {
            let width = (((max_decimal_place - min_decimal_place + 1) / 2) * 2) as usize;
            strings
                .chunks(3)
                .map(|values| {
                    let mut grouped = String::new();
                    for index in [0, 2, 1] {
                        let mut value = values[index].clone();
                        if index != 1 {
                            value.push_str(&"0".repeat(width.saturating_sub(value.len())));
                        } else {
                            value = value.trim_end_matches('0').to_string();
                            if value.is_empty() {
                                value.push('0');
                            }
                        }
                        grouped.push_str(&value);
                    }
                    encode(&grouped)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.unsupported("compressed value cannot be encoded", span))?
                .join("0")
        } else {
            strings
                .iter()
                .map(|value| encode(value))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.unsupported("compressed value cannot be encoded", span))?
                .join("0")
        };
        if !decode {
            return self.lower_custom_string(compressed, span);
        }
        let compressed_string = self.lower_custom_string(compressed, span)?;
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        let split = self.push_call("stringSplit", vec![compressed_string, separator]);
        let alphabet_value = if has_directive(self.hir, "useVariableForCompressionAlphabet") {
            let variable = *self
                .globals
                .get(COMPRESSION_ALPHABET_NAME)
                .expect("compression alphabet variable is created");
            self.push_value(Value::GlobalVariable(self.global_names[variable].clone()))
        } else {
            self.lower_custom_string(compression_alphabet(), span)?
        };
        let decoded = if has_directive(self.hir, "useVariableForCompressionAlphabet") {
            split
        } else {
            let current = self.push_call("currentArrayElement", Vec::new());
            let alphabet = self.push_call("appendToArray", vec![current, alphabet_value]);
            self.push_call("mappedArray", vec![split, alphabet])
        };
        let width = ((max_decimal_place - min_decimal_place + 1) / 2) as usize;
        let component = |this: &mut Self, component_offset: usize| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let mut terms = Vec::with_capacity(width);
            for index in 0..width {
                let position = this.push_number((index + component_offset) as f64, "");
                let character = this.push_call("charAt", vec![current, position]);
                let formula_alphabet =
                    if has_directive(this.hir, "useVariableForCompressionAlphabet") {
                        alphabet_value
                    } else {
                        this.push_call("lastOf", vec![current])
                    };
                let digit = this.push_call("strIndex", vec![formula_alphabet, character]);
                let power = 100_f64.powf(index as f64 + f64::from(min_decimal_place) / 2.0);
                let weighted = if power == 1.0 {
                    digit
                } else {
                    let power = this.push_number(power, "");
                    this.push_call("multiply", vec![power, digit])
                };
                terms.push(weighted);
            }
            let mut value = terms
                .first()
                .copied()
                .unwrap_or_else(|| this.push_number(0.0, ""));
            for term in terms.into_iter().skip(1) {
                value = this.push_call("add", vec![value, term]);
            }
            if is_vector || compression_offset == 0.0 {
                value
            } else {
                let offset = this.push_number(compression_offset, "");
                this.push_call("add", vec![value, offset])
            }
        };
        let value = if is_vector {
            let x = component(self, 0);
            let y = component(self, width * 2);
            let z = component(self, width);
            let vector = self.push_call("vector", vec![x, y, z]);
            let value = if compression_offset == 0.0 {
                vector
            } else {
                let offset = self.push_number(-compression_offset, "");
                let offset = self.push_call("vector", vec![offset, offset, offset]);
                self.push_call("subtract", vec![vector, offset])
            };
            self.push_call("mappedArray", vec![decoded, value])
        } else {
            let number = component(self, 0);
            self.push_call("mappedArray", vec![decoded, number])
        };
        Ok(value)
    }

    fn lower_compress(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        self.lower_compressed_mode(args, span, false)
    }

    fn strict_optimization_active(&self, expr: &Expr) -> bool {
        self.optimization_state_at(expr.span()).strict
    }

    fn optimization_state_at(&self, span: Option<&HirSpan>) -> OptimizationState {
        let Some(span) = span else {
            return self.hir.preprocessing.optimization.clone();
        };
        let mut active = None;
        for directive in &self.hir.preprocessing.directives {
            let Some(directive_span) = directive.span else {
                continue;
            };
            if directive_span.file != span.file {
                continue;
            }
            if directive_span.start.line < span.start.line
                || (directive_span.start.line == span.start.line
                    && directive_span.start.col <= span.start.col)
            {
                active = Some(directive.state.optimization.clone());
            } else {
                break;
            }
        }
        active
            .or_else(|| {
                self.hir
                    .preprocessing
                    .source_file_initial_optimization
                    .get(&span.file)
                    .cloned()
            })
            .unwrap_or_else(|| self.hir.preprocessing.optimization.clone())
    }

    fn lower_array_callback(
        &mut self,
        params: &[String],
        body: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        if !(1..=2).contains(&params.len()) {
            return Err(self.unsupported(
                "array callbacks require one element parameter and at most one index parameter",
                span,
            ));
        }
        if params.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                self.unsupported("array callback parameters must have distinct names", span)
            );
        }
        self.array_bindings.push(ArrayBinding {
            element: params[0].clone(),
            index: params.get(1).cloned(),
        });
        let result = self.lower_value(body);
        self.array_bindings.pop();
        result
    }

    fn lower_workshop_setting(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let [
            Expr::Type {
                name: setting_type,
                args: type_args,
                span: type_span,
            },
            category,
            setting_name,
            default,
            sort_order,
        ] = args
        else {
            return Err(self.unsupported(
                "createWorkshopSetting requires a type and four value arguments",
                span,
            ));
        };

        let catalog_name = match (setting_type.as_str(), type_args.as_slice()) {
            ("bool", []) => "createWorkshopSettingBool",
            ("int", [_, _]) => "createWorkshopSettingInt",
            ("float", [_, _]) => "createWorkshopSettingFloat",
            ("int", []) | ("float", []) => {
                return Err(self.unsupported(
                    format!("createWorkshopSetting type '{setting_type}' requires a numeric range"),
                    type_span.or(span),
                ));
            }
            _ => {
                return Err(self.unsupported(
                    format!("unsupported createWorkshopSetting type '{setting_type}'"),
                    type_span.or(span),
                ));
            }
        };

        // OverPy uses an ideographic space for an empty setting category so
        // the generated Workshop setting has a non-empty category value.
        let category = match category {
            Expr::String { value, .. } if value.is_empty() => {
                self.push_value(Value::String("\u{3000}".to_string()))
            }
            _ => self.lower_value(category)?,
        };
        let mut lowered = vec![
            category,
            self.lower_value(setting_name)?,
            self.lower_value(default)?,
        ];
        if let [minimum, maximum] = type_args.as_slice() {
            lowered.push(self.lower_value(minimum)?);
            lowered.push(self.lower_value(maximum)?);
        }
        lowered.push(self.lower_value(sort_order)?);
        Ok(self.push_call(catalog_name, lowered))
    }

    fn push_action(&mut self, action: Action) -> ActionId {
        let id = self.actions.len();
        self.actions.push(action);
        self.action_origins.push(None);
        self.action_argument_origins.push(Vec::new());
        id
    }

    fn mark_action_origins(&mut self, actions: &[ActionId], span: Option<HirSpan>) {
        for action in actions {
            let origin = self
                .action_origins
                .get_mut(*action)
                .expect("lowered action origin must resolve");
            if origin.is_none() {
                *origin = span;
            }
        }
    }

    fn mark_action_argument_origins<I>(&mut self, action: ActionId, spans: I)
    where
        I: IntoIterator<Item = Option<HirSpan>>,
    {
        self.action_argument_origins[action] = spans.into_iter().collect();
    }

    fn action_provenance(
        &self,
        actions: &[ActionId],
    ) -> Vec<(Option<HirSpan>, Vec<Option<HirSpan>>)> {
        self.useful_actions(actions)
            .iter()
            .map(|action| {
                (
                    self.action_origins[*action],
                    self.action_argument_origins[*action].clone(),
                )
            })
            .collect()
    }

    fn push_call_action_with_spans<I>(
        &mut self,
        name: impl Into<String>,
        args: &[ValueId],
        spans: I,
    ) -> ActionId
    where
        I: IntoIterator<Item = Option<HirSpan>>,
    {
        let action = self.push_call_action(name, args);
        self.mark_action_argument_origins(action, spans);
        action
    }

    fn value_args(&self, ids: &[ValueId]) -> Vec<ValueId> {
        ids.to_vec()
    }

    fn push_call_action(&mut self, name: impl Into<String>, args: &[ValueId]) -> ActionId {
        self.push_action(Action::Call {
            name: name.into(),
            args: args.to_vec(),
        })
    }

    fn push_if_actions(
        &mut self,
        branches: Vec<(ValueId, Vec<ActionId>)>,
        else_body: Option<Vec<ActionId>>,
    ) -> Vec<ActionId> {
        let mut result = Vec::new();
        for (index, (condition, body)) in branches.into_iter().enumerate() {
            result.push(self.push_action(if index == 0 {
                Action::If { condition }
            } else {
                Action::ElseIf { condition }
            }));
            result.extend(body);
        }
        if let Some(body) = else_body {
            result.push(self.push_action(Action::Else));
            result.extend(body);
        }
        result.push(self.push_action(Action::End));
        result
    }

    fn push_while_actions(&mut self, condition: ValueId, body: Vec<ActionId>) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::While { condition })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn push_for_global_actions(
        &mut self,
        variable: GlobalVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
    ) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::ForGlobalVariable {
            variable: self.global_names[variable].clone(),
            start,
            stop,
            step,
        })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn push_for_player_actions(
        &mut self,
        player: ValueId,
        variable: PlayerVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
    ) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::ForPlayerVariable {
            player,
            variable: self.player_names[variable].clone(),
            start,
            stop,
            step,
        })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn value(&self, id: ValueId) -> &Value {
        self.values.get(id).expect("lowered value id must resolve")
    }

    fn materialize_value(&self, id: ValueId) -> workshop_rs::Value {
        let value = self.materialize_value_inner(id);
        #[cfg(test)]
        crate::resource_metrics::record_value_materialization(&value);
        value
    }

    fn materialize_value_inner(&self, id: ValueId) -> workshop_rs::Value {
        let value = self.materialize_node(id);
        match self.optimized_nodes.get(&id) {
            Some(strict) => OperatorOptimizer::new(self.compiler, *strict).node(value),
            None => value,
        }
    }

    fn materialize_node(&self, id: ValueId) -> workshop_rs::Value {
        match self.value(id) {
            Value::Number(value) => workshop_rs::Value::Number(*value),
            Value::String(value) => workshop_rs::Value::String(value.clone()),
            Value::Bool(value) => workshop_rs::Value::Bool(*value),
            Value::Null => workshop_rs::Value::Null,
            Value::Array(elements) => workshop_rs::Value::Array(
                elements
                    .iter()
                    .map(|element| self.materialize_value_inner(*element))
                    .collect(),
            ),
            Value::Vector { x, y, z } => workshop_rs::Value::Vector {
                x: Box::new(self.materialize_value_inner(*x)),
                y: Box::new(self.materialize_value_inner(*y)),
                z: Box::new(self.materialize_value_inner(*z)),
            },
            Value::Enum { value_type, value } => workshop_rs::Value::Enum {
                value_type: value_type.clone(),
                value: value.clone(),
            },
            Value::GlobalVariable(value) => workshop_rs::Value::GlobalVariable(value.clone()),
            Value::PlayerVariable { player, variable } => workshop_rs::Value::PlayerVariable {
                player: Box::new(self.materialize_value_inner(*player)),
                variable: variable.clone(),
            },
            Value::Subroutine(value) => workshop_rs::Value::Subroutine(value.clone()),
            Value::EventPlayer => workshop_rs::Value::EventPlayer,
            Value::Call { name, args } => workshop_rs::Value::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.materialize_value_inner(*arg))
                    .collect(),
            },
        }
    }

    pub(super) fn workshop_span(
        &self,
        span: Option<HirSpan>,
    ) -> Result<Option<workshop_rs::source::Span>, IntegrationError> {
        let Some(span) = span else {
            return Ok(None);
        };
        if !self.hir.files.iter().any(|file| file.id == span.file) {
            return Err(IntegrationError::new(
                "source-file",
                format!("HIR span references unknown source file id {}", span.file),
                Some(span),
            ));
        }
        Ok(Some(workshop_rs::source::Span::new(
            workshop_rs::source::FileId::from_index(span.file as usize),
            workshop_rs::source::Position::new(span.start.line, span.start.col),
            workshop_rs::source::Position::new(span.end.line, span.end.col),
        )))
    }

    /// Modifications the pinned reference drops as useless instructions.
    fn useful_actions(&self, actions: &[ActionId]) -> Vec<ActionId> {
        actions
            .iter()
            .copied()
            .filter(|id| {
                let (op, value) = match &self.actions[*id] {
                    Action::ModifyGlobalVariable { op, value, .. }
                    | Action::ModifyPlayerVariable { op, value, .. } => (*op, *value),
                    _ => return true,
                };
                let optimization = self.optimization_state_at(self.action_origins[*id].as_ref());
                let identity = match op {
                    ModifyOp::Add | ModifyOp::Subtract => 0.0,
                    ModifyOp::Multiply | ModifyOp::Divide | ModifyOp::RaiseToPower => 1.0,
                    _ => return true,
                };
                !(optimization.enabled
                    && !optimization.strict
                    && self.value_is_number(value, identity))
            })
            .collect()
    }

    fn public_actions(&self, actions: &[ActionId]) -> Vec<workshop_rs::Action> {
        self.useful_actions(actions)
            .iter()
            .map(|id| {
                let mut action = self.materialize_action(&self.actions[*id]);
                let optimization = self.optimization_state_at(self.action_origins[*id].as_ref());
                if optimization.enabled && optimization.for_size {
                    SizeOptimizer::new(self.compiler).action(&mut action);
                }
                action
            })
            .collect()
    }

    fn materialize_action(&self, action: &Action) -> workshop_rs::Action {
        match action {
            Action::SetGlobalVariable { variable, value } => {
                workshop_rs::Action::SetGlobalVariable {
                    variable: variable.clone(),
                    value: self.materialize_value(*value),
                }
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
            } => workshop_rs::Action::ModifyGlobalVariable {
                variable: variable.clone(),
                op: *op,
                value: self.materialize_value(*value),
            },
            Action::SetPlayerVariable {
                player,
                variable,
                value,
            } => workshop_rs::Action::SetPlayerVariable {
                player: self.materialize_value(*player),
                variable: variable.clone(),
                value: self.materialize_value(*value),
            },
            Action::ModifyPlayerVariable {
                player,
                variable,
                op,
                value,
            } => workshop_rs::Action::ModifyPlayerVariable {
                player: self.materialize_value(*player),
                variable: variable.clone(),
                op: *op,
                value: self.materialize_value(*value),
            },
            Action::CallSubroutine { subroutine } => workshop_rs::Action::CallSubroutine {
                subroutine: subroutine.clone(),
            },
            Action::If { condition } => workshop_rs::Action::If {
                condition: self.materialize_value(*condition),
            },
            Action::ElseIf { condition } => workshop_rs::Action::ElseIf {
                condition: self.materialize_value(*condition),
            },
            Action::Else => workshop_rs::Action::Else,
            Action::While { condition } => workshop_rs::Action::While {
                condition: self.materialize_value(*condition),
            },
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
            } => workshop_rs::Action::ForGlobalVariable {
                variable: variable.clone(),
                start: self.materialize_value(*start),
                stop: self.materialize_value(*stop),
                step: self.materialize_value(*step),
            },
            Action::ForPlayerVariable {
                player,
                variable,
                start,
                stop,
                step,
            } => workshop_rs::Action::ForPlayerVariable {
                player: self.materialize_value(*player),
                variable: variable.clone(),
                start: self.materialize_value(*start),
                stop: self.materialize_value(*stop),
                step: self.materialize_value(*step),
            },
            Action::End => workshop_rs::Action::End,
            Action::Call { name, args } => workshop_rs::Action::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.materialize_value(*arg))
                    .collect(),
            },
        }
    }

    fn set_rule_provenance<C, A>(
        &mut self,
        rule: usize,
        span: Option<HirSpan>,
        conditions: C,
        actions: A,
    ) -> Result<(), IntegrationError>
    where
        C: IntoIterator<Item = Option<HirSpan>>,
        A: IntoIterator<Item = (Option<HirSpan>, Vec<Option<HirSpan>>)>,
    {
        self.program
            .set_rule_span(rule, self.workshop_span(span)?)
            .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        for (index, span) in conditions.into_iter().enumerate() {
            self.program
                .set_condition_span(rule, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        }
        for (index, (span, argument_spans)) in actions.into_iter().enumerate() {
            self.program
                .set_action_span(rule, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
            for (argument, span) in argument_spans.into_iter().enumerate() {
                let Some(span) = span else {
                    continue;
                };
                self.program
                    .set_action_argument_span(
                        rule,
                        index,
                        argument,
                        self.workshop_span(Some(span))?,
                    )
                    .map_err(|error| {
                        IntegrationError::new("provenance", error.to_string(), Some(span))
                    })?;
            }
        }
        Ok(())
    }

    fn unsupported(&self, message: impl Into<String>, span: Option<HirSpan>) -> IntegrationError {
        IntegrationError::new("unsupported-integration-surface", message, span)
    }
}

/// Collect the pinned OverPy implicit default global and player variables.
/// Global and player namespaces each have independent fixed Workshop slots;
/// only `eventPlayer.<name>` creates an implicit player variable.
fn implicit_default_variables(
    hir: &hir::Program,
) -> (
    BTreeMap<String, Option<HirSpan>>,
    BTreeMap<String, Option<HirSpan>>,
) {
    let declared_globals = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let declared_players = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::PlayerVariable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut globals = BTreeMap::new();
    let mut players = BTreeMap::new();
    for declaration in &hir.declarations {
        let initializer = match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => initializer.as_ref(),
            hir::Declaration::Constant { value, .. } => Some(value),
            _ => None,
        };
        if let Some(expr) = initializer {
            collect_implicit_expr(
                expr,
                &declared_globals,
                &declared_players,
                &mut globals,
                &mut players,
            );
        }
    }
    for entry in &hir.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for condition in &rule.conditions {
                    collect_implicit_expr(
                        condition,
                        &declared_globals,
                        &declared_players,
                        &mut globals,
                        &mut players,
                    );
                }
                collect_implicit_stmts(
                    &rule.actions,
                    &declared_globals,
                    &declared_players,
                    &mut globals,
                    &mut players,
                );
            }
            RuleEntry::SubroutineDef { body, .. } => collect_implicit_stmts(
                body,
                &declared_globals,
                &declared_players,
                &mut globals,
                &mut players,
            ),
        }
    }
    if hir
        .preprocessing
        .directives
        .iter()
        .any(|directive| directive.name == "translateWithPlayerVar")
    {
        players.insert("__languageIndex__".to_string(), None);
    }
    (globals, players)
}

fn collect_implicit_stmts(
    statements: &[Stmt],
    declared_globals: &HashSet<&str>,
    declared_players: &HashSet<&str>,
    globals: &mut BTreeMap<String, Option<HirSpan>>,
    players: &mut BTreeMap<String, Option<HirSpan>>,
) {
    for statement in statements {
        match statement {
            Stmt::Expr { expr, .. } => {
                collect_implicit_expr(expr, declared_globals, declared_players, globals, players)
            }
            Stmt::Assign { target, value, .. } => {
                collect_implicit_expr(target, declared_globals, declared_players, globals, players);
                collect_implicit_expr(value, declared_globals, declared_players, globals, players);
            }
            Stmt::Delete { target, .. } => {
                collect_implicit_expr(target, declared_globals, declared_players, globals, players);
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    collect_implicit_expr(
                        &branch.condition,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                    collect_implicit_stmts(
                        &branch.body,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
                if let Some(default_body) = r#else {
                    collect_implicit_stmts(
                        default_body,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                collect_implicit_expr(
                    variable,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_expr(
                    iterable,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_stmts(body, declared_globals, declared_players, globals, players);
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                condition, body, ..
            } => {
                collect_implicit_expr(
                    condition,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_stmts(body, declared_globals, declared_players, globals, players);
            }
            Stmt::Switch { value, arms, .. } => {
                collect_implicit_expr(value, declared_globals, declared_players, globals, players);
                for arm in arms {
                    match arm {
                        SwitchArm::Case { value, body, .. } => {
                            collect_implicit_expr(
                                value,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                            collect_implicit_stmts(
                                body,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                        }
                        SwitchArm::Default { body, .. } => {
                            collect_implicit_stmts(
                                body,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                        }
                    }
                }
            }
            Stmt::Goto { offset, .. } => {
                if let Some(offset) = offset {
                    collect_implicit_expr(
                        offset,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
            }
            Stmt::Break { .. }
            | Stmt::Return { .. }
            | Stmt::Continue { .. }
            | Stmt::Label { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. } => {}
        }
    }
}

fn collect_implicit_expr(
    expr: &Expr,
    declared_globals: &HashSet<&str>,
    declared_players: &HashSet<&str>,
    globals: &mut BTreeMap<String, Option<HirSpan>>,
    players: &mut BTreeMap<String, Option<HirSpan>>,
) {
    match expr {
        Expr::GlobalVar { name, span } => {
            if !declared_globals.contains(name.as_str()) && default_var_index(name).is_some() {
                globals.entry(name.clone()).or_insert(*span);
            }
        }
        Expr::Array { elements, .. } => {
            for element in elements {
                collect_implicit_expr(
                    element,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                collect_implicit_expr(
                    &entry.key,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_expr(
                    &entry.value,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            collect_implicit_expr(
                element,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                iterable,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            if let Some(condition) = condition {
                collect_implicit_expr(
                    condition,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Lambda { body, .. } => {
            collect_implicit_expr(body, declared_globals, declared_players, globals, players)
        }
        Expr::Type { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Vector { x, y, z, .. } => {
            collect_implicit_expr(x, declared_globals, declared_players, globals, players);
            collect_implicit_expr(y, declared_globals, declared_players, globals, players);
            collect_implicit_expr(z, declared_globals, declared_players, globals, players);
        }
        Expr::PlayerVar {
            player,
            name,
            member_span,
            span,
        } => {
            if !declared_players.contains(name.as_str()) && default_var_index(name).is_some() {
                players.entry(name.clone()).or_insert(member_span.or(*span));
            }
            collect_implicit_expr(player, declared_globals, declared_players, globals, players);
        }
        Expr::Member {
            receiver,
            member,
            span,
            ..
        } => {
            if !declared_players.contains(member.as_str()) && default_var_index(member).is_some() {
                players.entry(member.clone()).or_insert(*span);
            }
            collect_implicit_expr(
                receiver,
                declared_globals,
                declared_players,
                globals,
                players,
            );
        }
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            collect_implicit_expr(
                receiver,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_implicit_expr(left, declared_globals, declared_players, globals, players);
            collect_implicit_expr(right, declared_globals, declared_players, globals, players);
        }
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            collect_implicit_expr(
                then_value,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                condition,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                else_value,
                declared_globals,
                declared_players,
                globals,
                players,
            );
        }
        Expr::Unary { operand, .. } => collect_implicit_expr(
            operand,
            declared_globals,
            declared_players,
            globals,
            players,
        ),
        Expr::Index { array, index, .. } => {
            collect_implicit_expr(array, declared_globals, declared_players, globals, players);
            collect_implicit_expr(index, declared_globals, declared_players, globals, players);
        }
        Expr::Format { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. }
        | Expr::Enum { .. }
        | Expr::EventPlayer { .. }
        | Expr::HostPlayer { .. }
        | Expr::Constant { .. }
        | Expr::MacroParam { .. } => {}
    }
}

fn allocate_indices(
    entries: &[(Option<u32>, Option<HirSpan>)],
    pre_reserved: &HashSet<u32>,
    kind: &str,
) -> Result<Vec<u32>, IntegrationError> {
    let mut reserved = pre_reserved.clone();
    for (index, span) in entries {
        let Some(index) = index else {
            continue;
        };
        if !reserved.insert(*index) {
            return Err(IntegrationError::new(
                "index-collision",
                format!("duplicate explicit {kind} index {index}"),
                *span,
            ));
        }
    }

    // The pinned OverPy reference fills the remaining free slots in
    // ascending order for auto-allocated entries, regardless of where the
    // explicit indices sit in declaration order; an early explicit index
    // does not push later auto allocations above it.
    let mut next = 0;
    let mut allocated = Vec::with_capacity(entries.len());
    for (index, span) in entries {
        let assigned = if let Some(index) = index {
            *index
        } else {
            while reserved.contains(&next) {
                next = next.checked_add(1).ok_or_else(|| {
                    IntegrationError::new(
                        "index-exhausted",
                        format!("no available {kind} index remains"),
                        *span,
                    )
                })?;
            }
            reserved.insert(next);
            let assigned = next;
            next = next.checked_add(1).ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    format!("no available {kind} index remains"),
                    *span,
                )
            })?;
            assigned
        };
        allocated.push(assigned);
    }
    Ok(allocated)
}

fn player_event_kind(name: &str) -> Option<PlayerEventKind> {
    Some(match name {
        "playerDealtDamage" => PlayerEventKind::DealtDamage,
        "playerDealtFinalBlow" => PlayerEventKind::DealtFinalBlow,
        "playerDealtHealing" => PlayerEventKind::DealtHealing,
        "playerDealtKnockback" => PlayerEventKind::DealtKnockback,
        "playerDied" => PlayerEventKind::Died,
        "playerEarnedElimination" => PlayerEventKind::EarnedElimination,
        "playerJoined" => PlayerEventKind::Joined,
        "playerLeft" => PlayerEventKind::Left,
        "playerReceivedHealing" => PlayerEventKind::ReceivedHealing,
        "playerReceivedKnockback" => PlayerEventKind::ReceivedKnockback,
        "playerTookDamage" => PlayerEventKind::TookDamage,
        _ => return None,
    })
}

fn is_zero_initializer(expr: &hir::Expr) -> bool {
    matches!(
        expr,
        hir::Expr::Number { text, value, .. } if text == "0" && *value == 0.0
    )
}

fn has_directive(hir: &hir::Program, name: &str) -> bool {
    hir.preprocessing
        .directives
        .iter()
        .any(|directive| directive.name == name)
}

fn directive_value<'a>(hir: &'a hir::Program, name: &str) -> Option<&'a str> {
    hir.preprocessing
        .directives
        .iter()
        .rev()
        .find(|directive| directive.name == name)
        .and_then(|directive| directive.value.as_deref())
}

fn literal_number(expr: &hir::Expr) -> Option<f64> {
    match expr {
        hir::Expr::Null { .. } => Some(0.0),
        hir::Expr::Number { value, .. } => Some(*value),
        hir::Expr::Unary { op, operand, .. } if op == "+" => literal_number(operand),
        hir::Expr::Unary { op, operand, .. } if op == "-" => {
            literal_number(operand).map(|value| -value)
        }
        _ => None,
    }
}

fn expr_contains_random(expr: &hir::Expr) -> bool {
    match expr {
        hir::Expr::Call { name, args, .. } | hir::Expr::MacroCall { name, args, .. } => {
            name.starts_with("random.") || args.iter().any(expr_contains_random)
        }
        hir::Expr::Array { elements, .. } => elements.iter().any(expr_contains_random),
        hir::Expr::Dict { entries, .. } => entries
            .iter()
            .any(|entry| expr_contains_random(&entry.key) || expr_contains_random(&entry.value)),
        hir::Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            expr_contains_random(element)
                || expr_contains_random(iterable)
                || condition.as_deref().is_some_and(expr_contains_random)
        }
        hir::Expr::Lambda { body, .. } | hir::Expr::Unary { operand: body, .. } => {
            expr_contains_random(body)
        }
        hir::Expr::Vector { x, y, z, .. } => {
            expr_contains_random(x) || expr_contains_random(y) || expr_contains_random(z)
        }
        hir::Expr::PlayerVar { player, .. }
        | hir::Expr::Member {
            receiver: player, ..
        } => expr_contains_random(player),
        hir::Expr::ReceiverCall { receiver, args, .. } => {
            expr_contains_random(receiver) || args.iter().any(expr_contains_random)
        }
        hir::Expr::Type { args, .. } | hir::Expr::Format { args, .. } => {
            args.iter().any(expr_contains_random)
        }
        hir::Expr::Binary { left, right, .. } => {
            expr_contains_random(left) || expr_contains_random(right)
        }
        hir::Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            expr_contains_random(then_value)
                || expr_contains_random(condition)
                || expr_contains_random(else_value)
        }
        hir::Expr::Index { array, index, .. } => {
            expr_contains_random(array) || expr_contains_random(index)
        }
        hir::Expr::Number { .. }
        | hir::Expr::String { .. }
        | hir::Expr::Bool { .. }
        | hir::Expr::Null { .. }
        | hir::Expr::StringModifier { .. }
        | hir::Expr::Local { .. }
        | hir::Expr::Enum { .. }
        | hir::Expr::GlobalVar { .. }
        | hir::Expr::HostPlayer { .. }
        | hir::Expr::EventPlayer { .. }
        | hir::Expr::Constant { .. }
        | hir::Expr::MacroParam { .. } => false,
    }
}

fn compression_alphabet_chars() -> Vec<char> {
    (1..=47)
        .chain(std::iter::once(50))
        .chain(58..=64)
        .chain(std::iter::once(81))
        .chain(91..=96)
        .chain(std::iter::once(113))
        .chain(124..=127)
        .chain(128..=159)
        .chain(std::iter::once(161))
        .map(|value| char::from_u32(value).expect("compression alphabet is valid Unicode"))
        .collect()
}

fn compression_alphabet() -> String {
    compression_alphabet_chars().into_iter().collect()
}

fn literal_key_matches(left: &hir::Expr, right: &hir::Expr) -> bool {
    match (left, right) {
        (hir::Expr::Number { value: left, .. }, hir::Expr::Number { value: right, .. }) => {
            left == right
        }
        (hir::Expr::String { value: left, .. }, hir::Expr::String { value: right, .. }) => {
            left == right
        }
        (hir::Expr::Bool { value: left, .. }, hir::Expr::Bool { value: right, .. }) => {
            left == right
        }
        (hir::Expr::Null { .. }, hir::Expr::Null { .. }) => true,
        _ => false,
    }
}

fn indexed_target_parts<'a>(
    target: &'a hir::Expr,
    indices: &mut Vec<&'a hir::Expr>,
) -> Option<&'a hir::Expr> {
    match target {
        hir::Expr::Index { array, index, .. } => {
            indices.push(index);
            indexed_target_parts(array, indices)
        }
        hir::Expr::GlobalVar { .. } | hir::Expr::PlayerVar { .. } => Some(target),
        _ => None,
    }
}

fn is_literal_key(expr: &hir::Expr) -> bool {
    matches!(
        expr,
        hir::Expr::Number { .. }
            | hir::Expr::String { .. }
            | hir::Expr::Bool { .. }
            | hir::Expr::Null { .. }
    )
}

fn is_membership_literal(expr: &hir::Expr, strict: bool) -> bool {
    is_literal_key(expr) && (!strict || !matches!(expr, hir::Expr::String { .. }))
}

fn format_number_marker(index: usize) -> String {
    format_number_marker_value(index).to_string()
}

fn format_number_marker_value(index: usize) -> f64 {
    1_876_650.25 + index as f64
}

fn implicit_player_index(name: &str) -> u32 {
    if name == "__languageIndex__" {
        127
    } else {
        default_var_index(name).expect("implicit default player names resolve")
    }
}

fn big_letters(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut converted = false;
    for character in value.chars() {
        if !converted {
            if let Some(mapped) = big_letter(character) {
                output.push(mapped);
                converted = true;
                continue;
            }
        }
        output.push(character);
    }
    output
}

fn big_letter(character: char) -> Option<char> {
    Some(match character {
        'a' | 'A' => 'Α',
        'b' | 'B' => 'Β',
        'e' | 'E' => 'Ε',
        'h' | 'H' => 'Η',
        'i' | 'I' => 'Ι',
        'k' | 'K' => 'Κ',
        'm' | 'M' => 'Μ',
        'n' | 'N' => 'Ν',
        'o' | 'O' => 'Ο',
        'p' | 'P' => 'Ρ',
        't' | 'T' => 'Τ',
        'x' | 'X' => 'Χ',
        'y' | 'Y' => 'Υ',
        'z' | 'Z' => 'Ζ',
        '.' => '\u{2024}',
        ' ' => '\u{2028}',
        _ => return None,
    })
}

fn fullwidth(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            ' ' => '\u{2001}',
            '\u{00a5}' => '\u{ffe5}',
            '\u{20a9}' => '\u{ffe6}',
            '\u{00a2}' => '\u{ffe0}',
            '\u{00a3}' => '\u{ffe1}',
            '\u{00af}' => '\u{ffe3}',
            '\u{00ac}' => '\u{ffe2}',
            '\u{00a6}' => '\u{ffe4}',
            character if ('!'..='~').contains(&character) => {
                char::from_u32(character as u32 + 65248).unwrap_or(character)
            }
            _ => character,
        })
        .collect()
}

fn case_sensitive(value: &str) -> String {
    let mut output = value.replace('æ', "\u{04d5}").replace("nj", "\u{01cc}");
    output = output.replace(" a ", " ａ ");
    output
        .chars()
        .map(|character| match character {
            'a' => 'ạ',
            'b' => 'ḅ',
            'c' => 'ƈ',
            'd' => 'ḍ',
            'e' => 'ẹ',
            'f' => 'ƒ',
            'g' => 'ǥ',
            'h' => '\u{04bb}',
            'i' => 'і',
            'j' => 'ј',
            'k' => 'ḳ',
            'l' => 'I',
            'm' => 'ṃ',
            'n' => 'ṇ',
            'o' => 'ο',
            'p' => 'ṗ',
            'q' => 'ǫ',
            'r' => 'ṛ',
            's' => 'ѕ',
            't' => 'ṭ',
            'u' => 'υ',
            'v' => 'ν',
            'w' => 'ẉ',
            'x' => '\u{04b3}',
            'y' => 'ỵ',
            'z' => 'ẓ',
            _ => character,
        })
        .collect()
}

fn computed_number_text(value: f64) -> String {
    workshop_rs::format::format_number(value)
}

fn canonical_format_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut index = 0;
    while let Some(character) = chars.next() {
        if character == '{' && chars.peek() == Some(&'}') {
            chars.next();
            output.push('{');
            output.push_str(&index.to_string());
            output.push('}');
            index += 1;
        } else {
            output.push(character);
        }
    }
    output
}

fn split_format_chunks(text: &str, arg_count: usize) -> Option<Vec<(String, Vec<usize>)>> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut indices = Vec::new();
    let mut pending = String::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(open_rel) = text[cursor..].find('{') else {
            pending.push_str(&text[cursor..]);
            break;
        };
        let open = cursor + open_rel;
        let Some(close_rel) = text[open + 1..].find('}') else {
            pending.push_str(&text[cursor..]);
            break;
        };
        let close = open + 1 + close_rel;
        let marker = &text[open + 1..close];
        let Ok(index) = marker.parse::<usize>() else {
            pending.push_str(&text[cursor..=close]);
            cursor = close + 1;
            continue;
        };
        if index >= arg_count {
            return None;
        }
        pending.push_str(&text[cursor..open]);
        if indices.len() == 3 && !indices.contains(&index) {
            chunks.push((current, indices));
            current = String::new();
            indices = Vec::new();
        }
        current.push_str(&pending);
        pending.clear();
        let local = if let Some(local) = indices.iter().position(|candidate| *candidate == index) {
            local
        } else {
            indices.push(index);
            indices.len() - 1
        };
        current.push('{');
        current.push_str(&local.to_string());
        current.push('}');
        cursor = close + 1;
    }
    current.push_str(&pending);
    if current.is_empty() && chunks.is_empty() {
        return Some(vec![(text.to_string(), Vec::new())]);
    }
    chunks.push((current, indices));
    Some(chunks)
}

fn compile_time_value_text(value: crate::compile_time::Value) -> Option<String> {
    match value {
        crate::compile_time::Value::Number(value) if value.is_finite() => Some(value.to_string()),
        crate::compile_time::Value::Number(_) => None,
        crate::compile_time::Value::String(value) => Some(value),
        crate::compile_time::Value::Bool(value) => Some(value.to_string()),
        crate::compile_time::Value::Array(_) | crate::compile_time::Value::Object(_) => None,
    }
}

fn debug_expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Number { text, .. } => text.clone(),
        Expr::String { value, .. } => {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Null { .. } => "null".to_string(),
        Expr::Array { elements, .. } => format!(
            "[{}]",
            elements
                .iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Dict { entries, .. } => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|entry| format!(
                    "{}: {}",
                    debug_expr_text(&entry.key),
                    debug_expr_text(&entry.value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Comprehension {
            element,
            variable,
            iterable,
            condition,
            ..
        } => {
            let condition = condition
                .as_deref()
                .map(|condition| format!(" if {}", debug_expr_text(condition)))
                .unwrap_or_default();
            format!(
                "[{} for {} in {}{}]",
                debug_expr_text(element),
                variable,
                debug_expr_text(iterable),
                condition
            )
        }
        Expr::Lambda { params, body, .. } => {
            format!("lambda {}: {}", params.join(", "), debug_expr_text(body))
        }
        Expr::StringModifier {
            modifier, value, ..
        } => format!("{}\"{}\"", modifier, value),
        Expr::Local { name, .. }
        | Expr::GlobalVar { name, .. }
        | Expr::Constant { name, .. }
        | Expr::MacroParam { name, .. } => name.clone(),
        Expr::Type { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{}[{}]",
                    name,
                    args.iter()
                        .map(debug_expr_text)
                        .collect::<Vec<_>>()
                        .join(": ")
                )
            }
        }
        Expr::Vector { x, y, z, .. } => format!(
            "vect({}, {}, {})",
            debug_expr_text(x),
            debug_expr_text(y),
            debug_expr_text(z)
        ),
        Expr::Enum {
            value_type, value, ..
        } => format!("{}.{}", value_type, value),
        Expr::PlayerVar { player, name, .. } => {
            format!("{}.{}", debug_expr_text(player), name)
        }
        Expr::Member {
            receiver, member, ..
        } => format!("{}.{}", debug_expr_text(receiver), member),
        Expr::EventPlayer { .. } => "eventPlayer".to_string(),
        Expr::HostPlayer { .. } => "hostPlayer".to_string(),
        Expr::Call { name, args, .. } if name == "sorted" && args.len() == 2 => {
            format!(
                "sorted({}, key = {})",
                debug_expr_text(&args[0]),
                debug_expr_text(&args[1])
            )
        }
        Expr::Call { name, args, .. } | Expr::MacroCall { name, args, .. } => format!(
            "{}({})",
            name,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } => format!(
            "{}.{}({})",
            debug_expr_text(receiver),
            name,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Binary {
            left, op, right, ..
        } => format!(
            "{} {} {}",
            debug_expr_text(left),
            op,
            debug_expr_text(right)
        ),
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => format!(
            "{} if {} else {}",
            debug_expr_text(then_value),
            debug_expr_text(condition),
            debug_expr_text(else_value)
        ),
        Expr::Unary { op, operand, .. } => format!("{} {}", op, debug_expr_text(operand)),
        Expr::Index { array, index, .. } => {
            format!("{}[{}]", debug_expr_text(array), debug_expr_text(index))
        }
        Expr::Format { text, args, .. } => format!(
            "\"{}\".format({})",
            text,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn canonical_debug_text(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            'a' => 'ạ',
            'b' => 'ḅ',
            'c' => 'ƈ',
            'd' => 'ḍ',
            'e' => 'ẹ',
            'f' => 'ƒ',
            'g' => 'ǥ',
            'h' => 'һ',
            'i' => 'і',
            'j' => 'ј',
            'k' => 'ḳ',
            'l' => 'I',
            'm' => 'ṃ',
            'n' => 'ṇ',
            'o' => 'ο',
            'p' => 'ṗ',
            'q' => 'ǫ',
            'r' => 'ṛ',
            's' => 'ѕ',
            't' => 'ṭ',
            'u' => 'υ',
            'v' => 'ν',
            'w' => 'ẉ',
            'x' => 'ҳ',
            'y' => 'ỵ',
            'z' => 'ẓ',
            _ => character,
        })
        .collect()
}

fn negated_comparison(op: &str) -> Option<&'static str> {
    Some(match op {
        "==" => "!=",
        "!=" => "==",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        _ => return None,
    })
}

fn modify_op_from_str(op: &str) -> Option<ModifyOp> {
    match op {
        "+" => Some(ModifyOp::Add),
        "-" => Some(ModifyOp::Subtract),
        "*" => Some(ModifyOp::Multiply),
        "/" => Some(ModifyOp::Divide),
        "%" => Some(ModifyOp::Modulo),
        "**" => Some(ModifyOp::RaiseToPower),
        _ => None,
    }
}

fn modify_catalog_name_from_str(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("add"),
        "-" => Some("subtract"),
        "*" => Some("multiply"),
        "/" => Some("divide"),
        "%" => Some("modulo"),
        "**" => Some("raiseToPower"),
        _ => None,
    }
}

/// Rule names lose invisible formatting characters, and the Workshop's
/// filtered word `rigger` is split with a soft hyphen, as the pinned OverPy
/// does when it writes a rule name.
fn escape_rule_name(name: &str) -> String {
    let stripped: Vec<char> = name
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{FEFF}' | '\u{061C}'
            )
        })
        .collect();
    let mut escaped = String::with_capacity(name.len());
    let mut index = 0;
    while index < stripped.len() {
        escaped.push(stripped[index]);
        if matches!(stripped[index], 'a' | 'A')
            && stripped[index + 1..]
                .iter()
                .take(4)
                .collect::<String>()
                .eq_ignore_ascii_case("dmin")
        {
            escaped.push('\u{00AD}');
        }
        if matches!(stripped[index], 'r' | 'R') {
            if let Some(split) = filtered_word_split(&stripped, index) {
                escaped.extend(&stripped[index + 1..split]);
                escaped.push('\u{00AD}');
                index = split;
                continue;
            }
        }
        index += 1;
    }
    escaped
}

/// Where the soft hyphen goes when `r i gg e r` (whitespace allowed between
/// the letters, ending at a word boundary) starts at `start`.
fn filtered_word_split(text: &[char], start: usize) -> Option<usize> {
    let mut position = start + 1;
    let mut split = None;
    for letter in ['i', 'g', 'g', 'e', 'r'] {
        while text.get(position).is_some_and(|c| c.is_whitespace()) {
            position += 1;
        }
        if !text.get(position)?.eq_ignore_ascii_case(&letter) {
            return None;
        }
        if letter == 'i' {
            let mut after = position + 1;
            while text.get(after).is_some_and(|c| c.is_whitespace()) {
                after += 1;
            }
            split = Some(after);
        }
        position += 1;
    }
    let boundary = text
        .get(position)
        .is_none_or(|c| !(c.is_alphanumeric() || *c == '_'));
    boundary.then_some(split?)
}

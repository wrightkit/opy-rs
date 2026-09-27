mod action_calls;
mod assignments;
mod presentation;
mod rules;
mod values;

use super::action_optimization::ActionOptimizer;
use super::number_format::trim_numbers;
use super::operator_optimization::{OperatorOptimizer, same, self_modification};
use super::size_optimization::{SizeOptimizer, action_values, is_empty_string};
use super::string_format::split_all;
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
    used_maps: Vec<&'static str>,
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
    pub(super) fn new(compiler: &'a Compiler, hir: &'a hir::Program) -> Self {
        Self {
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
            used_maps: used_bugged_maps(hir),
            visible_labels: Vec::new(),
            deferred_gotos: Vec::new(),
            translation_uses: Vec::new(),
        }
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
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Ok(None);
        };
        self.free_global_index(
            reserved,
            translations.span,
            "no available global variable index remains for translations",
        )
    }

    fn helper_global_index(
        &self,
        reserved: &HashSet<u32>,
        directive: &str,
        message: &str,
    ) -> Result<Option<u32>, IntegrationError> {
        let Some(source) = self
            .hir
            .preprocessing
            .directives
            .iter()
            .find(|item| item.name == directive)
        else {
            return Ok(None);
        };
        self.free_global_index(reserved, source.span, message)
    }

    fn free_global_index(
        &self,
        reserved: &HashSet<u32>,
        span: Option<HirSpan>,
        message: &str,
    ) -> Result<Option<u32>, IntegrationError> {
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| IntegrationError::new("index-exhausted", message, span))
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
        let mut global_reserved = implicit_reserved;
        if let Some(index) = translation_helper_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let compression_alphabet_index = self.helper_global_index(
            &helper_reserved,
            "useVariableForCompressionAlphabet",
            "no available global variable index remains for the compression alphabet",
        )?;
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
        let mut globals = globals;
        let mut players = players;
        let mut explicit_globals = global_reserved.clone();
        explicit_globals.extend(globals.iter().filter_map(|(index, _)| *index));
        let mut explicit_players = implicit_player_reserved.clone();
        explicit_players.extend(players.iter().filter_map(|(index, _)| *index));
        let global_names =
            self.hir
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
                    _ => None,
                });
        top_allocate_reserved_names(global_names, &mut globals, &mut explicit_globals);
        let player_names =
            self.hir
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    hir::Declaration::PlayerVariable { name, .. } => Some(name.as_str()),
                    _ => None,
                });
        top_allocate_reserved_names(player_names, &mut players, &mut explicit_players);
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
                let value = self.lower_custom_string(compression_alphabet());
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
            self.program.rules.push(rule_from_parts(
                self.global_initializer_rule_name(),
                false,
                workshop_rs::Event::Global,
                Vec::new(),
                self.public_actions(&actions),
            ));
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
                let value = self.push_number(if no_tl_err { 0.1 } else { 1.1 });
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
            self.program.rules.push(rule_from_parts(
                self.player_initializer_rule_name(),
                false,
                workshop_rs::Event::EachPlayer,
                Vec::new(),
                self.public_actions(&actions),
            ));
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
        self.program.rules.push(rule_from_parts(
            name.to_string(),
            false,
            event,
            Vec::new(),
            self.public_actions(&actions),
        ));
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
        let initial = self.push_number(if no_tl_err { 0.1 } else { 1.1 });
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
        let ten = self.push_number(10.0);
        let direction_index = self.translation_language_index(translations)?;
        let horizontal = self.push_call("multiply", vec![ten, direction_index]);
        let vertical = self.push_number(5.0);
        let direction = self.push_call("directionFromAngles", vec![horizontal, vertical]);
        let turn_rate = self.push_number(999_999_999_999.0);
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
        let one_hundred = self.push_number(100.0);
        let horizontal_times_hundred =
            self.push_call("multiply", vec![horizontal_angle, one_hundred]);
        let nearest = self.push_value(Value::Enum {
            value_type: "Rounding".to_string(),
            value: "NEAREST".to_string(),
        });
        let rounded_horizontal =
            self.push_call("roundToInteger", vec![horizontal_times_hundred, nearest]);
        let thousand = self.push_number(1000.0);
        let modulo = self.push_call("modulo", vec![rounded_horizontal, thousand]);
        let zero = self.push_number(0.0);
        let modulo_zero = self.push_call("not", vec![modulo]);
        let vertical_angle = self.push_call("getVerticalFacingAngle", vec![player]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let tolerance = self.push_number(0.01);
        let vertical_close = self.push_call("<", vec![vertical_delta, tolerance]);
        let wait_condition = self.push_call("and", vec![modulo_zero, vertical_close]);
        let timeout = self.push_number(15.0);
        let wait = self.push_call_action("waitUntil", &[wait_condition, timeout]);

        let ten_for_angle = self.push_number(10.0);
        let horizontal_divided = self.push_call("divide", vec![horizontal_angle, ten_for_angle]);
        let rounded_angle = self.push_call("roundToInteger", vec![horizontal_divided, nearest]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let vertical_match = self.push_call("<", vec![vertical_delta, tolerance]);
        let one = self.push_number(1.0);
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
        self.program.rules.push(rule_from_parts(
            "OverPy translation setup - Determine the player's language".to_string(),
            false,
            Event::EachPlayer,
            vec![
                workshop_rs::Condition::new(self.materialize_value(has_spawned)),
                workshop_rs::Condition::new(self.materialize_value(not_dummy)),
                workshop_rs::Condition::new(self.materialize_value(initial_language)),
            ],
            self.public_actions(&actions),
        ));
        self.set_rule_provenance(
            rule_index,
            None,
            [None, None, None],
            self.action_provenance(&actions),
        )?;
        Ok(())
    }

    fn lower_array(&mut self, elements: Vec<ValueId>) -> ValueId {
        let name = if elements.is_empty() {
            "emptyArray"
        } else {
            "array"
        };
        let elements = self.normalize_contextual_arguments(name, elements);
        self.push_value(Value::Call {
            name: name.to_string(),
            args: self.value_args(&elements),
        })
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

    fn combine_conditions(
        &mut self,
        conditions: impl IntoIterator<Item = ValueId>,
    ) -> Option<ValueId> {
        let mut conditions = conditions.into_iter();
        let mut combined = conditions.next()?;
        for condition in conditions {
            combined = self.push_call("and", vec![combined, condition]);
        }
        Some(combined)
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
        value_id
    }

    fn normalize_contextual_arguments(
        &mut self,
        call_id: &str,
        mut args: Vec<ValueId>,
    ) -> Vec<ValueId> {
        let mut index = 0;
        while index < args.len() {
            args[index] = self.normalize_contextual_argument(call_id, index, args[index]);
            index += 1;
        }
        args
    }

    fn lower_custom_string(&mut self, value: String) -> ValueId {
        let text = self.push_value(Value::String(value));
        self.push_call("customString", vec![text])
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
                    .and_then(crate::compile_time::display)
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

    fn push_number(&mut self, value: f64) -> ValueId {
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
        self.push_loop_actions(Action::While { condition }, body)
    }

    fn push_for_global_actions(
        &mut self,
        variable: GlobalVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
    ) -> Vec<ActionId> {
        self.push_loop_actions(
            Action::ForGlobalVariable {
                variable: self.global_names[variable].clone(),
                start,
                stop,
                step,
            },
            body,
        )
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
        self.push_loop_actions(
            Action::ForPlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
                start,
                stop,
                step,
            },
            body,
        )
    }

    fn push_loop_actions(&mut self, start: Action, body: Vec<ActionId>) -> Vec<ActionId> {
        let mut result = vec![self.push_action(start)];
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
                let optimization = self.optimization_state_at(self.action_origins[*id].as_ref());
                if optimization.enabled {
                    if let Action::Call { name, args } = &self.actions[*id]
                        && name == "createHudText"
                        && args.len() > 3
                        && args[1..=3].iter().all(|text| {
                            let text = self.materialize_value(*text);
                            matches!(text, workshop_rs::Value::Null) || is_empty_string(&text)
                        })
                    {
                        return false;
                    }
                    if let Action::Call { name, args } = &self.actions[*id]
                        && name == "addToTeamScore"
                        && args.get(1).is_some_and(|score| self.value_is_number(*score, 0.0))
                    {
                        return false;
                    }
                    let assigns_itself = match &self.actions[*id] {
                        Action::SetGlobalVariable { variable, value } => {
                            matches!(self.value(*value), Value::GlobalVariable(other) if other == variable)
                        }
                        Action::SetPlayerVariable {
                            player,
                            variable,
                            value,
                        } => matches!(
                            self.value(*value),
                            Value::PlayerVariable { player: other, variable: other_variable }
                                if other_variable == variable
                                    && same(
                                        &self.materialize_value(*player),
                                        &self.materialize_value(*other),
                                    )
                        ),
                        _ => false,
                    };
                    if assigns_itself {
                        return false;
                    }
                }
                let (op, value) = match &self.actions[*id] {
                    Action::ModifyGlobalVariable { op, value, .. }
                    | Action::ModifyPlayerVariable { op, value, .. } => (*op, *value),
                    _ => return true,
                };
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
                for value in action_values(&mut action) {
                    split_all(value);
                }
                if is_zero_skip(&action) {
                    action = workshop_rs::Action::Disabled {
                        action: Box::new(workshop_rs::Action::Call {
                            name: "abort".to_string(),
                            args: Vec::new(),
                        }),
                    };
                }
                if optimization.enabled {
                    if let Some(modification) = self_modification(&action) {
                        action = modification;
                    }
                }
                ActionOptimizer::new(self.compiler, optimization.enabled).action(&mut action);
                if optimization.enabled && optimization.for_size {
                    SizeOptimizer::new(self.compiler).action(&mut action);
                }
                ActionOptimizer::new(self.compiler, optimization.enabled)
                    .wrap_booleans(&mut action);
                for value in action_values(&mut action) {
                    trim_numbers(value);
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
            Action::Call { name, .. } if name == "disabledAbort" => workshop_rs::Action::Disabled {
                action: Box::new(workshop_rs::Action::Call {
                    name: "abort".to_string(),
                    args: Vec::new(),
                }),
            },
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
    let mut collector = ImplicitVariableCollector {
        declared_globals: hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect(),
        declared_players: hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::PlayerVariable { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect(),
        globals: BTreeMap::new(),
        players: BTreeMap::new(),
    };
    for declaration in &hir.declarations {
        let initializer = match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => initializer.as_deref(),
            hir::Declaration::Constant { value, .. } => Some(value.as_ref()),
            _ => None,
        };
        if let Some(expression) = initializer {
            hir::visit::Visitor::visit_expr(&mut collector, expression);
        }
    }
    for entry in &hir.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for condition in &rule.conditions {
                    hir::visit::Visitor::visit_expr(&mut collector, condition);
                }
                hir::visit::walk_stmts(&mut collector, &rule.actions);
            }
            RuleEntry::SubroutineDef { body, .. } => {
                hir::visit::walk_stmts(&mut collector, body);
            }
        }
    }
    if hir
        .preprocessing
        .directives
        .iter()
        .any(|directive| directive.name == "translateWithPlayerVar")
    {
        collector
            .players
            .insert("__languageIndex__".to_string(), None);
    }
    (collector.globals, collector.players)
}

struct ImplicitVariableCollector<'a> {
    declared_globals: HashSet<&'a str>,
    declared_players: HashSet<&'a str>,
    globals: BTreeMap<String, Option<HirSpan>>,
    players: BTreeMap<String, Option<HirSpan>>,
}

impl hir::visit::Visitor for ImplicitVariableCollector<'_> {
    fn visit_expr(&mut self, expression: &Expr) {
        match expression {
            Expr::GlobalVar { name, span }
                if !self.declared_globals.contains(name.as_str())
                    && default_var_index(name).is_some() =>
            {
                self.globals.entry(name.clone()).or_insert(*span);
            }
            Expr::PlayerVar {
                name,
                member_span,
                span,
                ..
            } if !self.declared_players.contains(name.as_str())
                && default_var_index(name).is_some() =>
            {
                self.players
                    .entry(name.clone())
                    .or_insert(member_span.or(*span));
            }
            Expr::Member { member, span, .. }
                if !self.declared_players.contains(member.as_str())
                    && default_var_index(member).is_some() =>
            {
                self.players.entry(member.clone()).or_insert(*span);
            }
            _ => {}
        }
        hir::visit::walk_expr(self, expression);
    }
}

/// Variables named `__name__` take the highest free indices, in declaration
/// order, instead of filling the low free slots.
fn top_allocate_reserved_names<'a>(
    names: impl Iterator<Item = &'a str>,
    entries: &mut [(Option<u32>, Option<HirSpan>)],
    reserved: &mut HashSet<u32>,
) {
    for (name, entry) in names.zip(entries.iter_mut()) {
        if entry.0.is_some() || !(name.starts_with("__") && name.ends_with("__")) {
            continue;
        }
        if let Some(index) = (0..=127u32).rev().find(|index| !reserved.contains(index)) {
            reserved.insert(index);
            entry.0 = Some(index);
        }
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
    output.chars().map(case_sensitive_character).collect()
}

fn case_sensitive_character(character: char) -> char {
    match character {
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
    }
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
    text.chars().map(case_sensitive_character).collect()
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

fn modify_operator(op: &str) -> Option<(ModifyOp, &'static str)> {
    Some(match op {
        "+" => (ModifyOp::Add, "add"),
        "-" => (ModifyOp::Subtract, "subtract"),
        "*" => (ModifyOp::Multiply, "multiply"),
        "/" => (ModifyOp::Divide, "divide"),
        "%" => (ModifyOp::Modulo, "modulo"),
        "**" => (ModifyOp::RaiseToPower, "raiseToPower"),
        _ => return None,
    })
}

/// Words the Workshop refuses in a rule name. Each entry is the text before the
/// soft hyphen, the text after it, and whether the word must stand alone.
/// The pinned OverPy applies them in this order, each over the whole name.
const FILTERED_RULE_NAME_WORDS: [(&str, &str, bool); 28] = [
    ("1", "488", false),
    ("a", "ccount", false),
    ("a", "dmin", false),
    ("a", "ss", true),
    ("b", "attlenet", false),
    ("b", "liz", true),
    ("b", "lizzaard", true),
    ("b", "lizzard", false),
    ("b", "low", true),
    ("bn", "et", true),
    ("b", "razil", true),
    ("c", "anada", true),
    ("d", "enmark", true),
    ("e", "ngland", true),
    ("f", "inland", true),
    ("f", "uck", false),
    ("g", "oddamn", false),
    ("i", "reland", true),
    ("n", "etherlands", true),
    ("n", "orway", true),
    ("p", "oland", true),
    ("p", "olish", true),
    ("s", "anctuary", true),
    ("s", "atan", true),
    ("s", "ingapore", true),
    ("s", "hit", false),
    ("s", "weden", true),
    ("s", "witzerland", true),
];

/// Rule names and the `Mode Name` and `Description` settings strings lose
/// invisible formatting characters, and the Workshop's filtered words are split
/// with a soft hyphen, as the pinned OverPy does when it writes them.
pub(super) fn escape_bad_words(name: &str) -> String {
    let mut text: Vec<char> = name
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{FEFF}' | '\u{061C}'
            )
        })
        .collect();
    for (head, tail, standalone) in FILTERED_RULE_NAME_WORDS {
        text = split_filtered_word(&text, head, tail, standalone);
    }
    split_spaced_rigger(&text).into_iter().collect()
}

/// ECMAScript `\s`, which unlike Unicode White_Space excludes U+0085 and
/// includes U+FEFF.
fn is_js_whitespace(character: char) -> bool {
    matches!(character, '\u{FEFF}') || (character.is_whitespace() && character != '\u{0085}')
}

fn is_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// Puts a soft hyphen between `head` and `tail` in every case-insensitive
/// occurrence of `head` + `tail`, left to right without overlap.
fn split_filtered_word(text: &[char], head: &str, tail: &str, standalone: bool) -> Vec<char> {
    let word: Vec<char> = head.chars().chain(tail.chars()).collect();
    let head_length = head.chars().count();
    let mut result = Vec::with_capacity(text.len() + 1);
    let mut index = 0;
    while index < text.len() {
        let end = index + word.len();
        let matches = end <= text.len()
            && text[index..end]
                .iter()
                .zip(&word)
                .all(|(found, expected)| found.eq_ignore_ascii_case(expected))
            && (!standalone
                || (!index
                    .checked_sub(1)
                    .is_some_and(|before| is_word_character(text[before]))
                    && !text.get(end).is_some_and(|after| is_word_character(*after))));
        if matches {
            result.extend(&text[index..index + head_length]);
            result.push('\u{00AD}');
            result.extend(&text[index + head_length..end]);
            index = end;
        } else {
            result.push(text[index]);
            index += 1;
        }
    }
    result
}

/// The last word is `r i gg e r` with optional whitespace between the letters
/// except the two `g`,
/// ending at a word boundary; the soft hyphen follows the `i` and its whitespace.
/// The canonical Workshop value for an OverPy setting function, and its
/// parameter count without the trailing sort order.
fn workshop_setting_call(name: &str) -> (&str, usize) {
    match name {
        "createWorkshopSettingBool" => ("workshopSettingToggle", 3),
        "createWorkshopSettingEnum" => ("workshopSettingCombo", 4),
        "createWorkshopSettingInt" => ("workshopSettingInteger", 5),
        "createWorkshopSettingHero" => (name, 3),
        _ => (name, 5),
    }
}

fn split_spaced_rigger(text: &[char]) -> Vec<char> {
    let mut result = Vec::with_capacity(text.len() + 1);
    let mut index = 0;
    while index < text.len() {
        if matches!(text[index], 'r' | 'R')
            && let Some((split, end)) = spaced_rigger_match(text, index)
        {
            result.extend(&text[index..split]);
            result.push('\u{00AD}');
            result.extend(&text[split..end]);
            index = end;
        } else {
            result.push(text[index]);
            index += 1;
        }
    }
    result
}

/// Where the soft hyphen goes and where the match ends when `r i gg e r`
/// starts at `start`.
fn spaced_rigger_match(text: &[char], start: usize) -> Option<(usize, usize)> {
    let mut position = start + 1;
    let mut split = None;
    for (nth, letter) in ['i', 'g', 'g', 'e', 'r'].into_iter().enumerate() {
        // The two `g` are adjacent; whitespace may only precede the others.
        while nth != 2 && text.get(position).is_some_and(|c| is_js_whitespace(*c)) {
            position += 1;
        }
        if !text.get(position)?.eq_ignore_ascii_case(&letter) {
            return None;
        }
        position += 1;
        if letter == 'i' {
            while text.get(position).is_some_and(|c| is_js_whitespace(*c)) {
                position += 1;
            }
            split = Some(position);
        }
    }
    let boundary = text.get(position).is_none_or(|c| !is_word_character(*c));
    boundary.then_some((split?, position))
}

const BUGGED_MAPS: [&str; 4] = ["COLOSSEO", "ESPERANCA", "SAMOA", "THRONE_OF_ANUBIS"];
/// Only these are compared as text; the pinned OverPy leaves an equality
/// with `THRONE_OF_ANUBIS` as it is, though a bare current map still filters it.
const TEXT_COMPARED_MAPS: [&str; 3] = ["COLOSSEO", "ESPERANCA", "SAMOA"];

/// The maps named anywhere in the program whose value comparison the
/// Workshop gets wrong.
fn used_bugged_maps(hir: &hir::Program) -> Vec<&'static str> {
    fn collect(value: &serde_json::Value, found: &mut HashSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if fields.get("kind").and_then(|kind| kind.as_str()) == Some("enum")
                    && fields.get("type").and_then(|kind| kind.as_str()) == Some("Map")
                    && let Some(member) = fields.get("value").and_then(|value| value.as_str())
                {
                    found.insert(member.to_string());
                }
                fields.values().for_each(|field| collect(field, found));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
            _ => {}
        }
    }
    let mut found = HashSet::new();
    if let Ok(value) = serde_json::to_value(&hir.rules) {
        collect(&value, &mut found);
    }
    if let Ok(value) = serde_json::to_value(&hir.declarations) {
        collect(&value, &mut found);
    }
    BUGGED_MAPS
        .into_iter()
        .filter(|map| found.contains(*map))
        .collect()
}

/// A skip over nothing does nothing; the pinned OverPy writes it as a
/// disabled `Abort`.
fn is_zero_skip(action: &workshop_rs::Action) -> bool {
    matches!(
        action,
        workshop_rs::Action::Call { name, args }
            if matches!(name.as_str(), "skip" | "skipIf")
                && matches!(args.last(), Some(workshop_rs::Value::Number(distance)) if *distance == 0.0)
    )
}

fn rule_from_parts(
    name: String,
    disabled: bool,
    event: workshop_rs::Event,
    conditions: Vec<workshop_rs::Condition>,
    actions: Vec<workshop_rs::Action>,
) -> workshop_rs::Rule {
    let mut rule = workshop_rs::Rule::new(name, event);
    rule.disabled = disabled;
    rule.conditions = conditions;
    rule.actions = actions;
    rule
}

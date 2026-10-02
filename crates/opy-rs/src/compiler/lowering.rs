mod action_calls;
mod assignments;
mod declarations;
mod presentation;
mod rules;
mod values;

use super::action_optimization::ActionOptimizer;
use super::number_format::trim_numbers;
use super::operator_optimization::{
    NUMBER_LIMIT, OperatorOptimizer, expand_log, same, self_modification,
};
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
    optimization_mark: Option<bool>,
    optimization_override: Option<OptimizationState>,
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
    if matches!(statement, Stmt::Continue { .. }) {
        return Some(Vec::new());
    }
    let (condition, body, _) = single_if_statement(statement)?;
    let mut conditions = pure_continue_conditions(body)?;
    conditions.insert(0, condition);
    Some(conditions)
}

fn pure_goto_conditions(statement: &Stmt) -> Option<(Vec<&Expr>, &str)> {
    if let Stmt::Goto {
        label: Some(label),
        offset: None,
        rule_start: false,
        ..
    } = statement
    {
        return Some((Vec::new(), label.as_str()));
    }
    let (condition, body, _) = single_if_statement(statement)?;
    let (mut conditions, label) = pure_goto_conditions(body)?;
    conditions.insert(0, condition);
    Some((conditions, label))
}

fn single_if_statement(statement: &Stmt) -> Option<(&Expr, &Stmt, Option<HirSpan>)> {
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
    let [body] = branch.body.as_slice() else {
        return None;
    };
    Some((&branch.condition, body, *span))
}

fn direct_conditional_goto(statement: &Stmt) -> Option<(&Expr, &str, Option<HirSpan>)> {
    let (condition, body, span) = single_if_statement(statement)?;
    let Stmt::Goto {
        label: Some(label),
        offset: None,
        rule_start: false,
        ..
    } = body
    else {
        return None;
    };
    Some((condition, label.as_str(), span))
}

fn direct_conditional_dynamic_goto(statement: &Stmt) -> Option<(&Expr, &Expr, Option<HirSpan>)> {
    let (condition, body, span) = single_if_statement(statement)?;
    let Stmt::Goto {
        label: None,
        offset: Some(offset),
        rule_start: false,
        ..
    } = body
    else {
        return None;
    };
    Some((condition, offset, span))
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
            optimization_mark: None,
            optimization_override: None,
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
        self.program.settings = super::settings::workshop_settings(self.hir)?;
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
        if let Some(strict) = self.optimization_mark {
            // Nodes synthesized while lowering a source expression optimize
            // like the expression's own nodes.
            self.optimized_nodes.insert(id, strict);
        }
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
        (value.is_finite() && value.abs() <= NUMBER_LIMIT).then_some(value)
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
            // `log` expands to its power approximation even when the value is
            // not optimized; the folded form only applies under optimization.
            None => match value {
                workshop_rs::Value::Call { name, args } if name == "log" => expand_log(args, false),
                value => value,
            },
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

    fn append_rule(
        &mut self,
        rule_value: workshop_rs::Rule,
        actions: &[ActionId],
        span: Option<HirSpan>,
        condition_spans: impl IntoIterator<Item = Option<HirSpan>>,
    ) -> Result<(), IntegrationError> {
        let rule_index = self.program.rules.len();
        self.program.rules.push(rule_value);
        self.program
            .set_rule_span(rule_index, self.workshop_span(span)?)
            .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        for (index, span) in condition_spans.into_iter().enumerate() {
            self.program
                .set_condition_span(rule_index, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        }
        for (index, (span, argument_spans)) in
            self.action_provenance(actions).into_iter().enumerate()
        {
            self.program
                .set_action_span(rule_index, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
            for (argument, span) in argument_spans.into_iter().enumerate() {
                let Some(span) = span else {
                    continue;
                };
                self.program
                    .set_action_argument_span(
                        rule_index,
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

    fn global_variable_id(
        &self,
        name: &str,
        span: Option<HirSpan>,
    ) -> Result<GlobalVarId, IntegrationError> {
        self.globals
            .get(name)
            .copied()
            .ok_or_else(|| self.unsupported(format!("unknown global variable '{name}'"), span))
    }

    fn player_variable_id(
        &self,
        name: &str,
        span: Option<HirSpan>,
    ) -> Result<PlayerVarId, IntegrationError> {
        self.players
            .get(name)
            .copied()
            .ok_or_else(|| self.unsupported(format!("unknown player variable '{name}'"), span))
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

fn allocate_named_indices<'a>(
    names: impl Iterator<Item = &'a str>,
    entries: &mut [(Option<u32>, Option<HirSpan>)],
    pre_reserved: &HashSet<u32>,
    kind: &str,
) -> Result<Vec<u32>, IntegrationError> {
    let mut reserved = pre_reserved.clone();
    reserved.extend(entries.iter().filter_map(|(index, _)| *index));
    top_allocate_reserved_names(names, entries, &mut reserved);
    allocate_indices(entries, pre_reserved, kind)
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
    let mut text: Vec<char> = crate::lower::strip_rule_name_formatting(name).collect();
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

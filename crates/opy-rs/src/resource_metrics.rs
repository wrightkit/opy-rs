use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;
use workshop_rs::{Action, Value};

static LOWERING_VALUES_PEAK: AtomicUsize = AtomicUsize::new(0);
static LOWERING_VALUE_CLONE_NODES: AtomicUsize = AtomicUsize::new(0);
static LOWERING_ACTION_CLONE_EVENTS: AtomicUsize = AtomicUsize::new(0);
static COMPILER_CONTRACT_CHECKS: AtomicUsize = AtomicUsize::new(0);
static SETTINGS_CHARS_MATERIALIZED: AtomicUsize = AtomicUsize::new(0);
static MACRO_ENGINE_CREATIONS: AtomicUsize = AtomicUsize::new(0);
static MACRO_RUNTIME_CREATION_NS: AtomicUsize = AtomicUsize::new(0);
static MACRO_HOST_REGISTRATION_NS: AtomicUsize = AtomicUsize::new(0);
static MACRO_BUILTIN_EVALUATION_NS: AtomicUsize = AtomicUsize::new(0);
static MACRO_SCRIPT_EVALUATION_NS: AtomicUsize = AtomicUsize::new(0);
static MACRO_RUNTIME_TEARDOWN_NS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Default, Serialize, serde::Deserialize)]
pub(crate) struct ResourceMetrics {
    pub(crate) lowering_values_peak: usize,
    pub(crate) lowering_value_clone_nodes: usize,
    pub(crate) lowering_action_clone_events: usize,
    pub(crate) compiler_contract_checks: usize,
    pub(crate) settings_chars_materialized: usize,
    pub(crate) macro_engine_creations: usize,
    pub(crate) macro_runtime_creation_ns: usize,
    pub(crate) macro_host_registration_ns: usize,
    pub(crate) macro_builtin_evaluation_ns: usize,
    pub(crate) macro_script_evaluation_ns: usize,
    pub(crate) macro_runtime_teardown_ns: usize,
}

pub(crate) fn reset() {
    for counter in [
        &LOWERING_VALUES_PEAK,
        &LOWERING_VALUE_CLONE_NODES,
        &LOWERING_ACTION_CLONE_EVENTS,
        &COMPILER_CONTRACT_CHECKS,
        &SETTINGS_CHARS_MATERIALIZED,
        &MACRO_ENGINE_CREATIONS,
        &MACRO_RUNTIME_CREATION_NS,
        &MACRO_HOST_REGISTRATION_NS,
        &MACRO_BUILTIN_EVALUATION_NS,
        &MACRO_SCRIPT_EVALUATION_NS,
        &MACRO_RUNTIME_TEARDOWN_NS,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
}

pub(crate) fn snapshot() -> ResourceMetrics {
    ResourceMetrics {
        lowering_values_peak: LOWERING_VALUES_PEAK.load(Ordering::Relaxed),
        lowering_value_clone_nodes: LOWERING_VALUE_CLONE_NODES.load(Ordering::Relaxed),
        lowering_action_clone_events: LOWERING_ACTION_CLONE_EVENTS.load(Ordering::Relaxed),
        compiler_contract_checks: COMPILER_CONTRACT_CHECKS.load(Ordering::Relaxed),
        settings_chars_materialized: SETTINGS_CHARS_MATERIALIZED.load(Ordering::Relaxed),
        macro_engine_creations: MACRO_ENGINE_CREATIONS.load(Ordering::Relaxed),
        macro_runtime_creation_ns: MACRO_RUNTIME_CREATION_NS.load(Ordering::Relaxed),
        macro_host_registration_ns: MACRO_HOST_REGISTRATION_NS.load(Ordering::Relaxed),
        macro_builtin_evaluation_ns: MACRO_BUILTIN_EVALUATION_NS.load(Ordering::Relaxed),
        macro_script_evaluation_ns: MACRO_SCRIPT_EVALUATION_NS.load(Ordering::Relaxed),
        macro_runtime_teardown_ns: MACRO_RUNTIME_TEARDOWN_NS.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_lowering_values(len: usize) {
    LOWERING_VALUES_PEAK.fetch_max(len, Ordering::Relaxed);
}

pub(crate) fn record_value_clone(value: &Value) {
    LOWERING_VALUE_CLONE_NODES.fetch_add(value_node_count(value), Ordering::Relaxed);
}

pub(crate) fn record_action_clone(action: &Action) {
    LOWERING_ACTION_CLONE_EVENTS.fetch_add(1, Ordering::Relaxed);
    for value in action_values(action) {
        record_value_clone(value);
    }
}

pub(crate) fn record_contract_check() {
    COMPILER_CONTRACT_CHECKS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_settings_chars(len: usize) {
    SETTINGS_CHARS_MATERIALIZED.fetch_add(len, Ordering::Relaxed);
}

pub(crate) fn record_macro_engine_creation() {
    MACRO_ENGINE_CREATIONS.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MacroPhase {
    RuntimeCreation,
    HostRegistration,
    BuiltinEvaluation,
    ScriptEvaluation,
    RuntimeTeardown,
}

pub(crate) fn record_macro_phase(phase: MacroPhase, elapsed_ns: u128) {
    let elapsed_ns = usize::try_from(elapsed_ns).unwrap_or(usize::MAX);
    let counter = match phase {
        MacroPhase::RuntimeCreation => &MACRO_RUNTIME_CREATION_NS,
        MacroPhase::HostRegistration => &MACRO_HOST_REGISTRATION_NS,
        MacroPhase::BuiltinEvaluation => &MACRO_BUILTIN_EVALUATION_NS,
        MacroPhase::ScriptEvaluation => &MACRO_SCRIPT_EVALUATION_NS,
        MacroPhase::RuntimeTeardown => &MACRO_RUNTIME_TEARDOWN_NS,
    };
    counter.fetch_add(elapsed_ns, Ordering::Relaxed);
}

fn value_node_count(value: &Value) -> usize {
    1 + match value {
        Value::Call { args, .. } | Value::Array(args) => args.iter().map(value_node_count).sum(),
        Value::Vector { x, y, z } => {
            value_node_count(x) + value_node_count(y) + value_node_count(z)
        }
        Value::PlayerVariable { player, .. } => value_node_count(player),
        Value::Number(_)
        | Value::String(_)
        | Value::LocalizedString(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Enum { .. }
        | Value::GlobalVariable(_)
        | Value::Subroutine(_)
        | Value::EventPlayer => 0,
    }
}

fn action_values(action: &Action) -> Vec<&Value> {
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::AssignMember { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => vec![value],
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => vec![player, value],
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => vec![start, stop, step],
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => vec![player, start, stop, step],
        Action::Call { args, .. } => args.iter().collect(),
        Action::Disabled { action } => action_values(action),
        Action::CallSubroutine { .. } | Action::Else | Action::End => Vec::new(),
    }
}

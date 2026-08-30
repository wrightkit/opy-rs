//! Execution limits for JavaScript invocations.
//!
//! Default values mirror the pinned OverPy reference constants
//! (`src/quickjs.ts`): `MACRO_TIMEOUT_MS = 1000`,
//! `POST_COMPILE_HOOK_TIMEOUT_MS = 2000`, `MAX_RUNTIME_MEMORY_BYTES =
//! 64 * 1024 * 1024`, `MAX_RUNTIME_STACK_BYTES = 512 * 1024`.

use std::time::Duration;

/// Resource limits enforced on each JavaScript invocation.
///
/// Every field is applied to the engine that executes a single macro or hook
/// invocation (see [`super::MacroRuntime`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Wall-clock budget for macro invocations
    /// ([`super::MacroRuntime::run_macro`]).
    ///
    /// Enforced by a deadline-based interrupt handler: once the deadline
    /// passes, the engine aborts the script with the QuickJS `"interrupted"`
    /// error.
    pub(crate) macro_time_budget: Duration,
    /// Wall-clock budget for post-compile hook invocations
    /// ([`super::MacroRuntime::run_hook`]).
    pub(crate) hook_time_budget: Duration,
    /// Maximum engine memory in bytes (`JS_SetMemoryLimit` semantics).
    ///
    /// When the engine's tracked allocations exceed the limit, the script
    /// aborts with the `"out of memory"` error.
    pub(crate) memory_limit_bytes: usize,
    /// Maximum JavaScript stack size in bytes (`JS_SetMaxStackSize` semantics).
    ///
    /// Deep recursion aborts with `"Maximum call stack size exceeded"`.
    pub(crate) max_stack_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            macro_time_budget: Duration::from_millis(1000),
            hook_time_budget: Duration::from_millis(2000),
            memory_limit_bytes: 64 * 1024 * 1024,
            max_stack_bytes: 512 * 1024,
        }
    }
}

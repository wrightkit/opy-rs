//! Bounded JavaScript macro and post-compile hook runtime for OverPy-compatible
//! OPY tooling.
//!
//! This crate reproduces the observable compile-time JavaScript ABI of the
//! pinned OverPy reference (v9.7.10) for `#!define ... __script__("...")`
//! macros and `#!postCompileHook` scripts, without turning `opy-rs` into a
//! Node.js host:
//!
//! * macros receive their call-site arguments as injected `var <name>=<raw>;`
//!   declarations and are evaluated in global scope; the completion value of
//!   the script must be a string, which becomes the expanded text;
//! * post-compile hooks receive the compiled content as a `content` variable
//!   and must return the transformed content as a string;
//! * thrown exceptions surface as [`ScriptError`] with script name and
//!   line/column where the engine provides them;
//! * non-string results are rejected with the upstream error message
//!   "JavaScript macro returned value with type of `<typeof>`, expected string.
//!   Try using .toString()";
//! * execution is bounded by [`Limits`]: wall-clock time budgets
//!   (deadline-based interruption), a runtime memory limit, and a maximum JS
//!   stack size.
//!
//! # Host capability boundary
//!
//! The embedded engine is created per invocation with no host capabilities
//! beyond the JavaScript language intrinsics QuickJS provides (`Math`, `JSON`,
//! `Date`, `String`, `Array`, ...). There is **no** filesystem, process, shell,
//! or network access of any kind, and this crate registers no engine hooks
//! that could provide one. `console.log` is captured into
//! [`MacroResult::console_output`] instead of reaching the host.
//!
//! # Engine choice and replaceability
//!
//! Scripts run on [QuickJS-NG] embedded through the `libquickjs-ng-sys` crate
//! (the QuickJS-NG FFI layer maintained behind `quickjs-rusty`; the upstream
//! `quick-js-ng` crate name is not published on crates.io). QuickJS-NG is the
//! engine family the OverPy reference uses (quickjs-ng wasm), which keeps
//! observable language behavior aligned (completion values, `typeof`, error
//! messages such as `"interrupted"`). The binding is isolated behind the
//! crate-private [`JsEngine`] trait, so the concrete engine crate can be
//! swapped without touching the runtime logic.
//!
//! Building `libquickjs-ng-sys` compiles the QuickJS-NG C sources, which
//! requires a C compiler toolchain (`cc`/`clang`); on macOS the Xcode Command
//! Line Tools are sufficient.
//!
//! # Resource limits and context lifetime
//!
//! * Each invocation creates a fresh QuickJS runtime + context. A
//!   [`MacroRuntime`] instance is reusable, but **no JavaScript state is
//!   shared between invocations**: a script cannot observe globals set by an
//!   earlier script.
//! * Time budgets are enforced with a deadline-based interrupt handler
//!   (upstream `shouldInterruptAfterDeadline` semantics). When the budget is
//!   exceeded the script is aborted with the QuickJS `"interrupted"` error.
//! * The memory limit is enforced by the engine (`JS_SetMemoryLimit`
//!   semantics); the script is aborted with `"out of memory"`. The stack limit
//!   is enforced by `JS_SetMaxStackSize`; deep recursion aborts with
//!   `"Maximum call stack size exceeded"`.
//! * Defaults mirror the upstream constants (see [`Limits`]): 1000 ms macro
//!   budget, 2000 ms hook budget, 64 MiB memory, 512 KiB stack.
//!
//! # Supported helper surface
//!
//! Mirrors the upstream `builtInJsFunctions` block and `console` install
//! (`src/globalVars.ts`, `src/quickjs.ts`):
//!
//! * `vect(x, y, z)` returning `{x, y, z}` with a `toString()` producing
//!   `"vect(<x>,<y>,<z>)"`;
//! * the constant objects `Map`, `Hero`, `Gamemode`, `Color`, `Team`, `Button`
//!   — always defined, empty by default; populate them via
//!   [`Helpers::set_constant`] from catalog data (owned by `workshop-rs`);
//! * `console.log(...)` — captured into [`MacroResult::console_output`];
//! * engine intrinsics (`Math`, `JSON`, `String`, `Array`, ...).
//!
//! # What is lowering-dependent
//!
//! This crate is standalone and Workshop-independent:
//!
//! * wiring hook output into actual Workshop emission/backend integration
//!   (`workshop-rs`);
//! * the frontend's `__script__("...")` macro declaration parsing, script path
//!   resolution, and argument-count validation (OverPy-compatible OPY
//!   preprocessing, tracked separately);
//! * populating `Map`/`Hero`/... constants from the Workshop catalog;
//! * the macro-expansion indentation rule (each expansion line gets the
//!   call-site indentation prepended).
//!
//! Browser/WASM execution is not supported; the engine binding targets native
//! hosts only (the OverPy reference restricts script execution to Node, too).
//!
//! [QuickJS-NG]: https://github.com/quickjs-ng/quickjs
//! [`JsEngine`]: crate::engine::JsEngine

mod engine;
mod error;
mod helpers;
mod limits;
mod runtime;

pub use error::{MacroError, ScriptError};
pub use helpers::Helpers;
pub use limits::Limits;
pub use runtime::{MacroArg, MacroResult, MacroRuntime};

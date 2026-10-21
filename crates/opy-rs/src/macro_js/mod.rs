//! # Engine choice
//!
//! Scripts run on [QuickJS-NG] embedded through the `libquickjs-ng-sys` crate
//! (the QuickJS-NG FFI layer maintained behind `quickjs-rusty`; the upstream
//! `quick-js-ng` crate name is not published on crates.io). QuickJS-NG is the
//! engine family the OverPy reference uses (quickjs-ng wasm), which keeps
//! observable language behavior aligned (completion values, `typeof`, error
//! messages such as `"interrupted"`).
//!
//! Building `libquickjs-ng-sys` compiles the QuickJS-NG C sources, which
//! requires a C compiler toolchain (`cc`/`clang`); on macOS the Xcode Command
//! Line Tools are sufficient.
//!
//! [QuickJS-NG]: https://github.com/quickjs-ng/quickjs

mod engine;
mod error;
mod helpers;
mod limits;
mod runtime;

pub(crate) use error::MacroError;
pub(crate) use limits::Limits;
pub(crate) use runtime::{MacroArg, MacroRuntime};

#[cfg(test)]
mod tests;

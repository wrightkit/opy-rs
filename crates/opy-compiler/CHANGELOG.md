# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/wrightkit/opy-rs/compare/opy-compiler-v0.1.2...opy-compiler-v0.1.4) - 2026-08-28

### Added

- *(compiler)* integrate Workshop backend slices ([#82](https://github.com/wrightkit/opy-rs/pull/82))
- *(compiler)* close catalog-backed lowering ([#78](https://github.com/wrightkit/opy-rs/pull/78))

### Other

- release v0.1.3 ([#81](https://github.com/wrightkit/opy-rs/pull/81))

## [0.1.3](https://github.com/wrightkit/opy-rs/compare/opy-compiler-v0.1.2...opy-compiler-v0.1.3) - 2026-08-28

### Added

- *(compiler)* integrate Workshop backend slices ([#82](https://github.com/wrightkit/opy-rs/pull/82))
- *(compiler)* close catalog-backed lowering ([#78](https://github.com/wrightkit/opy-rs/pull/78))

## [0.1.2](https://github.com/wrightkit/opy-rs/compare/opy-compiler-v0.1.1...opy-compiler-v0.1.2) - 2026-08-28

### Fixed

- move catalog validation and settings lowering to owner ([#79](https://github.com/wrightkit/opy-rs/pull/79))

## [0.1.1](https://github.com/wrightkit/opy-rs/compare/opy-compiler-v0.1.0...opy-compiler-v0.1.1) - 2026-08-27

### Added

- *(opy-compiler)* complete issue 47 control-flow lowering
- *(opy-compiler)* lower control flow into canonical WIR
- *(opy-compiler)* lower non-control-flow primitives to canonical WIR
- *(opy-compiler)* lower OPY structure into canonical WIR
- *(opy)* establish Workshop integration boundary

### Fixed

- *(release)* rearm unpublished packages ([#74](https://github.com/wrightkit/opy-rs/pull/74))
- handle released Workshop modify operators
- *(opy)* consume canonical control-flow layout
- *(opy-compiler)* complete #46 primitive compatibility
- *(opy-compiler)* adopt workshop-rs 0.1.8 power contract
- *(opy-compiler)* close PR #54 review blockers for #46
- *(opy-compiler)* preserve subroutine identity and oracle evidence
- *(opy)* remove remaining metadata drift
- *(opy)* reconcile issue 35 review findings

### Other

- release v0.1.0 ([#71](https://github.com/wrightkit/opy-rs/pull/71))
- format owner implementation
- make opy-rs the source implementation owner
- *(workspace)* centralize shared dependencies ([#56](https://github.com/wrightkit/opy-rs/pull/56))
- *(opy-compiler)* rebaseline on workshop-rs 0.1.5
- *(opy-compiler)* remove dead compile_source entry and CompileError ([#50](https://github.com/wrightkit/opy-rs/pull/50))

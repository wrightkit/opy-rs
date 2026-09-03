# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.13](https://github.com/wrightkit/opy-rs/compare/v0.1.12...v0.1.13) - 2026-09-03

### Added

- *(provider)* ship first-party LPP provider ([#171](https://github.com/wrightkit/opy-rs/pull/171))

## [0.1.12](https://github.com/wrightkit/opy-rs/compare/v0.1.11...v0.1.12) - 2026-09-02

### Fixed

- *(opy)* converge source failure frontiers ([#168](https://github.com/wrightkit/opy-rs/pull/168))
- *(opy)* converge semantic failure frontiers ([#166](https://github.com/wrightkit/opy-rs/pull/166))
- *(opy)* converge project preprocessing composition ([#164](https://github.com/wrightkit/opy-rs/pull/164))

## [0.1.11](https://github.com/wrightkit/opy-rs/compare/v0.1.10...v0.1.11) - 2026-09-01

### Added

- *(opy)* complete final #88 source convergence ([#156](https://github.com/wrightkit/opy-rs/pull/156))
- *(opy)* complete source semantic and project state convergence ([#155](https://github.com/wrightkit/opy-rs/pull/155))
- *(opy)* complete rule condition and weapon builtins ([#152](https://github.com/wrightkit/opy-rs/pull/152))
- *(opy)* complete bounded canonical lowering ([#150](https://github.com/wrightkit/opy-rs/pull/150))
- *(opy)* expand audited OverPy builtin surface ([#151](https://github.com/wrightkit/opy-rs/pull/151))
- *(opy)* complete semantic HIR subroutine resolution
- *(opy)* complete preprocessing, macros, and project composition ([#147](https://github.com/wrightkit/opy-rs/pull/147))
- *(opy)* complete audited statement grammar surface ([#146](https://github.com/wrightkit/opy-rs/pull/146))

### Fixed

- *(opy)* complete project preprocessing state ([#154](https://github.com/wrightkit/opy-rs/pull/154))
- *(opy)* preserve top-level semantic order

### Other

- Merge pull request #149 from wrightkit/codex/issue-143-semantic-hir

## [0.1.10](https://github.com/wrightkit/opy-rs/compare/v0.1.9...v0.1.10) - 2026-08-31

### Added

- *(opy)* support getHorizontalFacingAngle receiver member ([#138](https://github.com/wrightkit/opy-rs/pull/138))
- *(opy)* support SpecVisibility.NEVER ([#139](https://github.com/wrightkit/opy-rs/pull/139))

## [0.1.9](https://github.com/wrightkit/opy-rs/compare/v0.1.8...v0.1.9) - 2026-08-31

### Added

- *(opy-compiler)* support player-variable range binders ([#136](https://github.com/wrightkit/opy-rs/pull/136))

### Fixed

- *(opy)* accept mainFile in included sources ([#134](https://github.com/wrightkit/opy-rs/pull/134))
- *(opy)* support numeric Team enum members ([#135](https://github.com/wrightkit/opy-rs/pull/135))

## [0.1.8](https://github.com/wrightkit/opy-rs/compare/v0.1.7...v0.1.8) - 2026-08-30

### Added

- *(opy)* lower bounded higher-order array operations ([#126](https://github.com/wrightkit/opy-rs/pull/126))
- *(opy)* support numeric range setting types ([#127](https://github.com/wrightkit/opy-rs/pull/127))

## [0.1.7](https://github.com/wrightkit/opy-rs/compare/v0.1.6...v0.1.7) - 2026-08-30

### Fixed

- *(opy-compiler)* close semantic HIR gaps ([#98](https://github.com/wrightkit/opy-rs/pull/98))

### Other

- *(deps)* consume workshop-rs 0.1.16 contracts ([#108](https://github.com/wrightkit/opy-rs/pull/108))

## [0.1.5](https://github.com/wrightkit/opy-rs/compare/v0.1.4...v0.1.5) - 2026-08-29

### Other

- Merge remote-tracking branch 'origin/main' into codex/issue-38-compile

## [0.1.3](https://github.com/wrightkit/opy-rs/compare/v0.1.2...v0.1.3) - 2026-08-28

### Added

- *(compiler)* integrate Workshop backend slices ([#82](https://github.com/wrightkit/opy-rs/pull/82))
- *(compiler)* close catalog-backed lowering ([#78](https://github.com/wrightkit/opy-rs/pull/78))

## [0.1.2](https://github.com/wrightkit/opy-rs/compare/v0.1.1...v0.1.2) - 2026-08-28

### Fixed

- move catalog validation and settings lowering to owner ([#79](https://github.com/wrightkit/opy-rs/pull/79))

## [0.1.1](https://github.com/wrightkit/opy-rs/compare/v0.1.0...v0.1.1) - 2026-08-27

### Added

- *(release)* automate crates.io releases ([#70](https://github.com/wrightkit/opy-rs/pull/70))

### Fixed

- *(release)* rearm unpublished packages ([#74](https://github.com/wrightkit/opy-rs/pull/74))
- *(release)* include support matrix in opy package ([#72](https://github.com/wrightkit/opy-rs/pull/72))

### Other

- release v0.1.0 ([#71](https://github.com/wrightkit/opy-rs/pull/71))
- make opy-rs the source implementation owner

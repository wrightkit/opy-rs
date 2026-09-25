# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.58](https://github.com/wrightkit/opy-rs/compare/v0.1.57...v0.1.58) - 2026-09-25

### Fixed

- *(compiler)* spell startForcingThrottle limits 0 and 1 as False and True under optimizeForSize ([#382](https://github.com/wrightkit/opy-rs/pull/382))

## [0.1.57](https://github.com/wrightkit/opy-rs/compare/v0.1.56...v0.1.57) - 2026-09-25

### Other

- *(compiler)* migrate to the workshop-rs 0.7.0 non-exhaustive Rule ([#381](https://github.com/wrightkit/opy-rs/pull/381))

## [0.1.56](https://github.com/wrightkit/opy-rs/compare/v0.1.55...v0.1.56) - 2026-09-25

### Added

- *(compiler)* escape filtered words in rule names and settings strings ([#376](https://github.com/wrightkit/opy-rs/pull/376))
- *(provider)* emit workshop-rs/mapped-text-v1 from opy-provider ([#368](https://github.com/wrightkit/opy-rs/pull/368))

### Fixed

- *(compiler)* fold builtin calls on the authored literal ([#373](https://github.com/wrightkit/opy-rs/pull/373)) ([#377](https://github.com/wrightkit/opy-rs/pull/377))
- *(compiler)* close structural gaps outside the Bastion entrypoints (#366, partial) ([#370](https://github.com/wrightkit/opy-rs/pull/370))

## [0.1.55](https://github.com/wrightkit/opy-rs/compare/v0.1.54...v0.1.55) - 2026-09-24

### Added

- *(compiler)* write skips over nothing as a disabled Abort
- *(compiler)* compress split dictionary arrays and keep unit weights like pinned OverPy
- *(compiler)* optimize every lowered value
- *(compiler)* compare and select bugged current maps like pinned OverPy
- *(compiler)* merge and split custom strings like pinned OverPy
- *(compiler)* rewrite self assignments and fold array and string reads
- *(compiler)* fold literal array reads like pinned OverPy
- *(compiler)* optimize every source-derived value and fold rounding
- *(compiler)* allocate reserved-name variables from the top
- *(compiler)* escape rule names like pinned OverPy
- *(compiler)* elide identity maps and exclude filters like pinned OverPy
- *(compiler)* spell empty strings and compact vectors under optimizeForSize
- *(compiler)* fold terminal conditionals and compact vectors like pinned OverPy
- *(compiler)* converge operator and size optimization on pinned OverPy

### Fixed

- *(compiler)* match the reference on rounding, bare returns and map checks
- *(compiler)* keep goto distance exact when the output drops instructions
- *(compiler)* keep numeric operators unfolded when optimizations are disabled
- *(compiler)* spell game mode values as the Game Mode value
- *(compiler)* keep goto out of a loop as a conditional skip
- *(compiler)* write general team settings before per-hero groups
- *(parser)* attach else to the enclosing conditional after a nested one
- *(compiler)* keep HUD text values as written
- *(compiler)* match pinned OverPy defaults and HUD color replacement
- *(compiler)* match pinned OverPy manifest defaults, chase forms, and condition operands
- *(compiler)* format substituted numbers and fold direction vectors like pinned OverPy
- *(compiler)* render generated rule names through the prefix template

### Other

- *(compat)* decide the semantic gate by structural identity
- compare rules and variables structurally in the convergence fixtures
- cover direction constants in the declarations fixture
- add structural fixtures for strings, arrays, maps, builtins, compression, and settings
- *(compat)* compare reparsed native output and promote converged fixtures
- Merge remote-tracking branch 'origin/main' into feat/319-structural-convergence
- *(compiler)* simplify chase argument replacement

## [0.1.54](https://github.com/wrightkit/opy-rs/compare/v0.1.53...v0.1.54) - 2026-09-23

### Other

- Migrate OPY to the public Workshop API ([#360](https://github.com/wrightkit/opy-rs/pull/360))

## [0.1.53](https://github.com/wrightkit/opy-rs/compare/v0.1.52...v0.1.53) - 2026-09-23

### Fixed

- *(compiler)* handle future Workshop modify operations ([#354](https://github.com/wrightkit/opy-rs/pull/354))

### Other

- use canonical Workshop exports ([#356](https://github.com/wrightkit/opy-rs/pull/356))

## [0.1.52](https://github.com/wrightkit/opy-rs/compare/v0.1.51...v0.1.52) - 2026-09-22

### Other

- pin Bastion condition and coercion semantics ([#352](https://github.com/wrightkit/opy-rs/pull/352))

## [0.1.51](https://github.com/wrightkit/opy-rs/compare/v0.1.50...v0.1.51) - 2026-09-21

### Fixed

- *(opy)* close v0.1.50 forward-language residuals ([#351](https://github.com/wrightkit/opy-rs/pull/351))

### Other

- *(opy)* finalize v0.1.50 language support audit ([#348](https://github.com/wrightkit/opy-rs/pull/348))

## [0.1.50](https://github.com/wrightkit/opy-rs/compare/v0.1.49...v0.1.50) - 2026-09-21

### Fixed

- close remaining Bastion rule compatibility gaps ([#346](https://github.com/wrightkit/opy-rs/pull/346))

## [0.1.49](https://github.com/wrightkit/opy-rs/compare/v0.1.48...v0.1.49) - 2026-09-21

### Added

- *(opy)* complete Workshop output locale integration ([#344](https://github.com/wrightkit/opy-rs/pull/344))

## [0.1.48](https://github.com/wrightkit/opy-rs/compare/v0.1.47...v0.1.48) - 2026-09-20

### Added

- *(opy)* complete translations and external settings ([#339](https://github.com/wrightkit/opy-rs/pull/339))
- *(opy)* complete compiler directives and replacements ([#341](https://github.com/wrightkit/opy-rs/pull/341))
- *(opy)* complete pinned callable surface ([#340](https://github.com/wrightkit/opy-rs/pull/340))
- *(opy)* complete enum and constant domain coverage ([#334](https://github.com/wrightkit/opy-rs/pull/334))
- *(opy)* complete collection mutation and control flow ([#335](https://github.com/wrightkit/opy-rs/pull/335))

### Other

- *(opy)* consolidate compatibility test metadata ([#338](https://github.com/wrightkit/opy-rs/pull/338))

## [0.1.47](https://github.com/wrightkit/opy-rs/compare/v0.1.46...v0.1.47) - 2026-09-19

### Added

- *(opy)* support Array.unique() member semantics ([#323](https://github.com/wrightkit/opy-rs/pull/323))

## [0.1.46](https://github.com/wrightkit/opy-rs/compare/v0.1.45...v0.1.46) - 2026-09-18

### Fixed

- *(opy)* lower conditional forward gotos correctly ([#320](https://github.com/wrightkit/opy-rs/pull/320))

## [0.1.45](https://github.com/wrightkit/opy-rs/compare/v0.1.44...v0.1.45) - 2026-09-17

### Other

- update workshop-rs to 0.4.1 ([#317](https://github.com/wrightkit/opy-rs/pull/317))

## [0.1.44](https://github.com/wrightkit/opy-rs/compare/v0.1.43...v0.1.44) - 2026-09-17

### Fixed

- *(opy)* converge Bastion-owned lowering identities ([#315](https://github.com/wrightkit/opy-rs/pull/315))

## [0.1.43](https://github.com/wrightkit/opy-rs/compare/v0.1.42...v0.1.43) - 2026-09-16

### Fixed

- *(settings)* remove whole-source char buffering ([#309](https://github.com/wrightkit/opy-rs/pull/309))

### Other

- *(lowering)* reduce recursive Value cloning ([#312](https://github.com/wrightkit/opy-rs/pull/312))
- *(macro)* measure QuickJS lifecycle and preserve isolation ([#311](https://github.com/wrightkit/opy-rs/pull/311))
- *(compiler)* cache immutable contract linking ([#308](https://github.com/wrightkit/opy-rs/pull/308))

## [0.1.42](https://github.com/wrightkit/opy-rs/compare/v0.1.41...v0.1.42) - 2026-09-16

### Added

- establish compiler resource baseline ([#305](https://github.com/wrightkit/opy-rs/pull/305))

## [0.1.41](https://github.com/wrightkit/opy-rs/compare/v0.1.40...v0.1.41) - 2026-09-16

### Fixed

- *(compiler)* converge residual OPY compiler and canonical-WIR gaps ([#293](https://github.com/wrightkit/opy-rs/pull/293))

## [0.1.40](https://github.com/wrightkit/opy-rs/compare/v0.1.39...v0.1.40) - 2026-09-16

### Fixed

- support observable optimizeStrict semantics ([#291](https://github.com/wrightkit/opy-rs/pull/291))
- support useVariableForCompressionAlphabet ([#292](https://github.com/wrightkit/opy-rs/pull/292))
- preserve optimized default wait semantics ([#287](https://github.com/wrightkit/opy-rs/pull/287))
- *(compiler)* preserve Unicode names through emission ([#286](https://github.com/wrightkit/opy-rs/pull/286))

### Other

- cover disabled OPY rule emission ([#285](https://github.com/wrightkit/opy-rs/pull/285))

## [0.1.39](https://github.com/wrightkit/opy-rs/compare/v0.1.38...v0.1.39) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.1.38](https://github.com/wrightkit/opy-rs/compare/v0.1.37...v0.1.38) - 2026-09-14

### Fixed

- lower indexed receiver mutations ([#277](https://github.com/wrightkit/opy-rs/pull/277))
- lower extension directives into canonical settings (Fixes #270) ([#278](https://github.com/wrightkit/opy-rs/pull/278))
- *(compiler)* preserve explicit null initializers ([#276](https://github.com/wrightkit/opy-rs/pull/276))
- *(lexer)* decode evidenced Unicode escapes (Fixes #267) ([#273](https://github.com/wrightkit/opy-rs/pull/273))
- preserve disabled rules and consume delimiters ([#275](https://github.com/wrightkit/opy-rs/pull/275))
- reject unsupported backend directives explicitly ([#271](https://github.com/wrightkit/opy-rs/pull/271)) ([#274](https://github.com/wrightkit/opy-rs/pull/274))

## [0.1.37](https://github.com/wrightkit/opy-rs/compare/v0.1.36...v0.1.37) - 2026-09-14

### Fixed

- preserve observable OverPy lowering semantics ([#265](https://github.com/wrightkit/opy-rs/pull/265))

## [0.1.36](https://github.com/wrightkit/opy-rs/compare/v0.1.35...v0.1.36) - 2026-09-13

### Fixed

- *(preprocess)* match OverPy macro evaluation order ([#263](https://github.com/wrightkit/opy-rs/pull/263))
- *(preprocess)* preserve multiline macro compatibility ([#262](https://github.com/wrightkit/opy-rs/pull/262))

## [0.1.35](https://github.com/wrightkit/opy-rs/compare/v0.1.34...v0.1.35) - 2026-09-13

### Fixed

- *(lexer)* preserve multiline define statement boundaries ([#261](https://github.com/wrightkit/opy-rs/pull/261))

## [0.1.34](https://github.com/wrightkit/opy-rs/compare/v0.1.33...v0.1.34) - 2026-09-13

### Fixed

- match OverPy legacy include resolution ([#259](https://github.com/wrightkit/opy-rs/pull/259))

## [0.1.33](https://github.com/wrightkit/opy-rs/compare/v0.1.32...v0.1.33) - 2026-09-12

### Added

- *(project)* accept directory provider targets ([#253](https://github.com/wrightkit/opy-rs/pull/253))

## [0.1.32](https://github.com/wrightkit/opy-rs/compare/v0.1.31...v0.1.32) - 2026-09-12

### Added

- *(compiler)* migrate OPY lowering to Program API

### Fixed

- *(compiler)* align range argument provenance
- *(compiler)* preserve initializer argument provenance
- *(compiler)* preserve canonical action provenance

### Other

- Merge pull request #248 from wrightkit/codex/opy-rs-issue-244
- *(compiler)* consume workshop-rs public provenance API

## [0.1.31](https://github.com/wrightkit/opy-rs/compare/v0.1.29...v0.1.31) - 2026-09-11

### Fixed

- *(compiler)* converge canonical WIR residuals ([#240](https://github.com/wrightkit/opy-rs/pull/240))

### Other

- release v0.1.30 ([#241](https://github.com/wrightkit/opy-rs/pull/241))
- *(settings)* prove canonical consumer queries ([#243](https://github.com/wrightkit/opy-rs/pull/243))

## [0.1.30](https://github.com/wrightkit/opy-rs/compare/v0.1.29...v0.1.30) - 2026-09-10

### Fixed

- *(compiler)* converge canonical WIR residuals ([#240](https://github.com/wrightkit/opy-rs/pull/240))

### Other

- *(settings)* prove canonical consumer queries ([#243](https://github.com/wrightkit/opy-rs/pull/243))

## [0.1.28](https://github.com/wrightkit/opy-rs/compare/v0.1.27...v0.1.28) - 2026-09-09

### Other

- *(opy)* remove redundant Rust prose ([#234](https://github.com/wrightkit/opy-rs/pull/234))

## [0.1.27](https://github.com/wrightkit/opy-rs/compare/v0.1.26...v0.1.27) - 2026-09-08

### Fixed

- *(compiler)* adopt workshop-rs 0.3 ([#232](https://github.com/wrightkit/opy-rs/pull/232))

## [0.1.26](https://github.com/wrightkit/opy-rs/compare/v0.1.24...v0.1.26) - 2026-09-08

### Other

- release v0.1.25 ([#220](https://github.com/wrightkit/opy-rs/pull/220))
- localize contextual lowering policy ([#222](https://github.com/wrightkit/opy-rs/pull/222))

## [0.1.25](https://github.com/wrightkit/opy-rs/compare/v0.1.24...v0.1.25) - 2026-09-08

### Other

- localize contextual lowering policy ([#222](https://github.com/wrightkit/opy-rs/pull/222))

## [0.1.24](https://github.com/wrightkit/opy-rs/compare/v0.1.23...v0.1.24) - 2026-09-08

### Fixed

- *(compiler)* align canonical vector and number emission ([#219](https://github.com/wrightkit/opy-rs/pull/219))

## [0.1.23](https://github.com/wrightkit/opy-rs/compare/v0.1.22...v0.1.23) - 2026-09-08

### Fixed

- *(opy)* map getAllPlayers to all teams ([#217](https://github.com/wrightkit/opy-rs/pull/217))

## [0.1.22](https://github.com/wrightkit/opy-rs/compare/v0.1.21...v0.1.22) - 2026-09-07

### Other

- *(compiler)* split feature-local ownership modules ([#214](https://github.com/wrightkit/opy-rs/pull/214))
- *(preprocess)* split OPY ownership modules ([#213](https://github.com/wrightkit/opy-rs/pull/213))

## [0.1.21](https://github.com/wrightkit/opy-rs/compare/v0.1.20...v0.1.21) - 2026-09-07

### Other

- *(parser)* give grammar responsibilities feature-local ownership ([#210](https://github.com/wrightkit/opy-rs/pull/210))
- *(lower)* split semantic ownership modules ([#205](https://github.com/wrightkit/opy-rs/pull/205))

## [0.1.20](https://github.com/wrightkit/opy-rs/compare/v0.1.19...v0.1.20) - 2026-09-06

### Fixed

- *(deps)* consume workshop-rs 0.1.19 ([#200](https://github.com/wrightkit/opy-rs/pull/200))

### Other

- *(opy)* retire top-level compatibility subsystem ([#196](https://github.com/wrightkit/opy-rs/pull/196))
- *(opy)* normalize test ownership around features ([#194](https://github.com/wrightkit/opy-rs/pull/194))

## [0.1.19](https://github.com/wrightkit/opy-rs/compare/v0.1.18...v0.1.19) - 2026-09-05

### Fixed

- *(opy)* resolve compile-time settings expressions ([#189](https://github.com/wrightkit/opy-rs/pull/189))

## [0.1.18](https://github.com/wrightkit/opy-rs/compare/v0.1.17...v0.1.18) - 2026-09-05

### Fixed

- *(deps)* consume workshop-rs 0.1.18 ([#186](https://github.com/wrightkit/opy-rs/pull/186))

## [0.1.15](https://github.com/wrightkit/opy-rs/compare/v0.1.14...v0.1.15) - 2026-09-04

### Other

- deliver and verify the integrated first-party `opy-provider` binary distribution path ([#176](https://github.com/wrightkit/opy-rs/issues/176), [#178](https://github.com/wrightkit/opy-rs/pull/178))

## [0.1.14](https://github.com/wrightkit/opy-rs/compare/v0.1.12...v0.1.14) - 2026-09-03

### Added

- *(provider)* ship first-party LPP provider ([#171](https://github.com/wrightkit/opy-rs/pull/171))

### Other

- release v0.1.13 ([#169](https://github.com/wrightkit/opy-rs/pull/169))

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

# opy-rs Architecture

Status: accepted baseline. Documents the opy-rs repository architecture,
ownership boundary, and stable compatibility contracts as of the merged
`main` baseline (PRs #9–#14).
Scope: project intent, current capability, the integration boundary toward
`workshop-rs`, and the stable contracts that downstream tooling and agents can
rely on.

## Pipeline

```
OPY source ──▶ lexer ──▶ preprocess ──▶ CST/parser ──▶ semantic resolution
   (includes, ──▶ ──▶ ──▶ ──▶ ──▶ ──▶ ──▶ OPY semantic model (Opy HIR v1)
   #!define,        settings blocks, #!postCompileHook record
   __script__ macros, includes)
                              │
                              ▼
                    [ integration boundary ]
                              │
                              ▼
                      workshop-rs (lowering + Workshop emission, catalog,
                                   member/domain/settings/locale data)
```

The frontend pipeline is fully Workshop-independent up to the integration
boundary. `opy-rs` produces the Opy HIR v1 semantic model
([`docs/hir/opy-hir-v1.md`](../hir/opy-hir-v1.md)) with structured,
source-located diagnostics, and stops there. Anything that requires canonical
Anything that requires the complete canonical Workshop surface (catalog
spellings, member/domain existence beyond the linked slice, settings-schema
content, locale data, optimizer effects, and `#!postCompileHook` execution
against final Workshop text) remains `lowering-dependent` in the support
matrix. The first compiler slice is implemented in `opy-compiler`; the wider
lowering track remains outside issue #35 and must not be silently reclassified.

## Ownership boundary

`opy-rs` owns the OPY **language and API** layer:

* OverPy `.opy` syntax: lexer, preprocessor (includes, `#!define`, script
  macros, settings blocks, post-compile-hook records), CST/parser, semantic
  resolution, and the Opy HIR v1 semantic model;
* the OPY semantic compatibility manifest
  ([`compat-manifest-spec.md`](compat-manifest-spec.md)): builtin
  action/value/member identities, signatures, aliases, contextual-domain
  dispatch, and `catalogId` links, never Workshop catalog content;
* Workshop-independent tooling: the `check`/`inspect` library API and the
  `opy-cli` CLI ([`tooling-api.md`](tooling-api.md)).

`workshop-rs` owns (or will own, at integration) the Workshop layer:

* canonical Workshop semantics, WIR, emission, and the catalog (member
  lists, enum domains, settings schema, hero/map/gamemode content);
* locale/localization data and the en-US emission surface;
* member/domain/catalog existence validation and settings-key validation.

`opy-rs` never approximates Workshop-owned validation with a local allowlist
or a temporary Workshop IR; those checks are deferred (`lowering-dependent`,
#8) rather than approximated.

## First integration boundary (#35)

The repository now contains a dedicated `opy-compiler` crate with this
dependency direction:

```
opy-frontend (OPY lexer/parser/semantic HIR)
        │
        ▼
opy-compiler (OPY HIR adapter and integration diagnostics)
        │
        ▼
workshop-rs v0.1.1 (Catalog, WIR, validation, emission)
```

`opy-frontend` remains buildable and testable without `workshop-rs`. The
integration crate pins the published `workshop-rs` v0.1.1 contract and constructs
its compiler only after mechanically checking every manifest `catalogId` and
enum/domain link against the canonical `Catalog`. The resulting compilation
artifact exposes the canonical catalog identity and digest for downstream
consumers.

The accepted vertical slice consumes resolved HIR for a global rule,
global-variable literal assignments, and a catalog-backed generic action call.
It copies HIR source files into the Workshop source-file arena,
maps all lowered spans to those typed file ids, validates canonical WIR, and
emits deterministic en-US Workshop. Unsupported HIR constructs fail with a
source-attributed integration diagnostic; they are not treated as frontend or
Workshop catalog support.

The slice is evidenced by
`compatibility/fixtures/synthetic/issue-35-integration` and the
`opy-compiler` tests. It establishes the boundary, not full OPY compilation:
settings/content, locale selection, optimizer effects, hook execution, and
the remaining OPY lowering surface remain explicit follow-up work.

## Integration input contract (#8)

The frontend-to-Workshop boundary has two inputs, both already available
without a `workshop-rs` dependency in this repository:

* the resolved Opy HIR v1 program, whose `call` and `receiverCall` nodes retain
  the OPY source identity and source span; and
* the validated OPY semantic manifest (`Manifest::builtin()`), whose function
  entries provide the call kind, receiver category, parameter binding/defaults,
  aliases, contextual dispatch, and optional canonical `catalogId`.

The #8 adapter combines those inputs by looking up a call's resolved OPY
identity in the manifest. It does not require parser or HIR redesign and it
does not infer Workshop content from an absent entry. For an entry with a
`catalogId`, `workshop-rs` must cross-check that the canonical id exists in its
catalog and has the compatible action/value/member kind. It must also
cross-check manifest alias targets and contextual dispatch targets against the
same canonical-id namespace. Parameter `domain` values and contextual option
domains are identity links; member lists, domain membership, localized
spellings, and settings/content data remain `workshop-rs` inputs.

Entries without a `catalogId` are explicit frontend special forms or
integration gaps (`debug`, `print`, `chase`, `stopChasing`, `range`, `append`,
`getHero`, and `hasStatus` in the current manifest). The adapter must handle
those cases by their documented source semantics or report a structured
lowering-dependent gap; it must not synthesize a catalog id or copy Workshop
data into `opy-rs`.

## Stable contracts

The following contracts are stable and preserved verbatim across the
documentation:

* **Observable semantic compatibility, not compiler-output identity.**
  Compatibility is defined by observable semantics (accepted/rejected
  surface, diagnostics, and the semantic model), not by byte/text identity,
  formatting, optimizer choices, or internal representation. Presentation
  differences (e.g. `Global.<name>` vs a bare variable name in emitted text)
  are not compatibility bugs unless they change observable semantics.
* **Corpus-defined support.** Every declared feature is backed by the
  compatibility corpus (`compatibility/fixtures/**`, pinned OverPy 9.7.10
  oracle snapshots) or explicitly marked as investigation; support states
  are tracked mechanically in
  [`compatibility/support-matrix.json`](../../compatibility/support-matrix.json)
  (`planned`, `frontend-supported`, `semantic-supported`,
  `lowering-dependent`, `end-to-end-supported`) and validated by
  `compatibility/tests/test_support_matrix.py`.
* **No WrightKit-only OPY dialect.** The language surface targets the pinned
  OverPy reference; opy-rs does not invent syntax or semantics that would
  make `.opy` sources non-portable. Deviations from the reference are
  documented, corpus-evidenced differences (e.g. explicit rejections), not
  new dialect features.
* **Source-aware validated edits as the default tooling model.** Tooling
  operates on authored source ranges with full provenance (spans, file
  registry, include and macro expansion attribution) and validates before
  editing instead of regenerating whole files. Trivia/comments are not
  retained (see
  [`trivia-retention-policy.md`](trivia-retention-policy.md)); reconstruction
  and byte-stable regeneration are deferred to the reconstruction surface.

## Bounded JavaScript runtime

`__script__("…")` macros execute at compile time through the embedded
QuickJS-NG runtime (`crates/opy-macro-js`; no Node required). Resource limits
mirror the pinned reference constants (`Limits::default()`: 1000 ms macro
budget, 64 MiB memory, 512 KiB stack), and failures map to structured
`script-*` diagnostics with script path/line/column provenance. The runtime
is a bounded failure/capability boundary: thrown exceptions, timeouts,
memory/stack aborts, and non-string results are diagnostics, never panics or
unbounded execution. `#!postCompileHook` is parsed, validated, and recorded
only. The frontend never executes it and never fabricates a Workshop payload.

## Current capability and readiness

Implemented and CI-covered in the #7 readiness Draft PR series (issues #2–#6
complete; #28/#29/#30/#33 executed):

* the full Workshop-independent frontend pipeline to Opy HIR v1;
* the OPY semantic compatibility manifest with oracle-validated probes;
* JavaScript macro execution and record-only post-compile hooks;
* the `check`/`inspect`/`support` tooling API and `opy-cli`;
* the 42-fixture compatibility corpus with pinned oracle snapshots and the
  native differential suite (`cargo test -p opy-frontend --test
  differential`).

Readiness: issue #7's Workshop-independent compatibility gate is implemented
in the four Draft PR tracks. Issue #30 separates the manifest-declared OPY
semantic overlay from the canonical Workshop catalog; the receiver residual
`A = B.C` is represented with provenance-preserving member HIR, and the
support matrix has no remaining `planned` entries. Issue #35 establishes the
first OPY-to-canonical-WIR boundary on workshop-rs v0.1.1. The broader #8
lowering surface (full catalog/content breadth, settings, locales, optimizer
effects, decompilation, and post-compile-hook execution against Workshop text)
remains explicitly not started here.

## Validation

* Rust quality gates: `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo test
  --workspace --all-targets --all-features` (CI: Ubuntu, Rust stable and
  1.85.0).
* Cross-platform runtime tests: `opy-macro-js` suite on macOS and Windows
  (CI).
* Compatibility harness (oracle-free): `python3 -m unittest discover -s
  compatibility/tests`.
* Oracle-required steps run standalone: `compatibility/run_oracle.py` and the
  manifest probe validator (`crates/opy-frontend/src/manifest/probes/`).

## Authoritative documents

* [`support-matrix.md`](support-matrix.md): declared corpus-evidenced
  surface. [`compatibility/support-matrix.json`](../../compatibility/support-matrix.json)
  is the single mechanically checked state source.
* [`compatibility-baseline.md`](compatibility-baseline.md): tiered planning
  for the remaining surface.
* [`compat-manifest-spec.md`](compat-manifest-spec.md): the semantic manifest
  schema and ownership boundary.
* [`../hir/opy-hir-v1.md`](../hir/opy-hir-v1.md): the Opy HIR v1 wire
  contract.
* [`tooling-api.md`](tooling-api.md): the `check`/`inspect` API and CLI
  contract.
* [`../compatibility/upstream-references.md`](../compatibility/upstream-references.md):
  the pinned OverPy reference identity and clean-room/provenance policy.
* [`trivia-retention-policy.md`](trivia-retention-policy.md): what the
  frontend retains and discards from authored source.

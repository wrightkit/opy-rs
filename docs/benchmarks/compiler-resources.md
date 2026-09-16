# Compiler resource baseline

This is the reproducible resource baseline for [opy-rs#296](https://github.com/wrightkit/opy-rs/issues/296), before the optimization work in #295. It is manual evidence infrastructure, not a correctness gate: the workload test is ignored, and no timing threshold is asserted in CI.

The accepted baseline record is [`compiler-resources-baseline.json`](compiler-resources-baseline.json), generated from the audited `origin/main` revision on macOS arm64 with five samples per workload. It records seven independent workloads; the instrumented candidate workflow adds the `macro-runtime-heavy` workload:

| Workload | Mechanism isolated | Input and provenance |
| --- | --- | --- |
| `nested-expression-lowering` | Nested lowering values and subtree copies | Generated source, depth 24, SHA-256 recorded in the baseline record |
| `wide-call-value-construction` | Wide value construction and nested argument copies | Generated `[vect(...)]`, 32 vectors, SHA-256 recorded in the baseline record |
| `many-rule-compilation` | Retained lowering state across rules | Generated source, 128 rules, SHA-256 recorded in the baseline record |
| `compiler-contract-initialization` | Repeated manifest/catalog contract initialization | Generated `Compiler::new()` workload, 64 constructions, SHA-256 recorded in the baseline record |
| `macro-runtime` | QuickJS engine creation per invocation | Fixed script `(x + 2).toString();`, 64 invocations, SHA-256 recorded in the baseline record |
| `macro-runtime-heavy` | QuickJS lifecycle cost with non-trivial script work | Helper- and collection-heavy script, 64 invocations, SHA-256 recorded in the candidate record |
| `large-settings-source` | Settings scanner materialization | Generated JSONC-like block with 1,000 keys, SHA-256 recorded in the baseline record |
| `real-world-parabola` | End-to-end compiler path on a provenance-linked project | `tests/fixtures/corpus/real-world/overpy-parabola`, including its `fixture.json` provenance and source SHA-256 |

The test-only counters report retained lowering values, copied value-tree nodes, copied actions, manifest/catalog contract checks, materialized settings characters, and QuickJS engine creations. For the two macro workloads they also report aggregate nanoseconds for runtime/context creation, host registration, builtin-helper evaluation, user-script evaluation, and runtime/context teardown. They are compiled only for unit tests and do not add a production API or telemetry surface. Each workload runs in a separate child test process; peak RSS is read with `getrusage(RUSAGE_SELF)` inside that child. macOS reports bytes directly; Linux converts the kernel's KiB value to bytes. Other platforms report `null` for RSS.

## Baseline command

Run from the `opy-rs` repository at the revision being measured:

```sh
BASELINE_REVISION="$(python3 -c 'import json, pathlib; print(json.loads(pathlib.Path("docs/benchmarks/compiler-resources-baseline.json").read_text())["baselineRevision"])')"
CANDIDATE_REVISION="$(git rev-parse HEAD)"
OPY_RESOURCE_BASELINE_REVISION="$BASELINE_REVISION" \
OPY_RESOURCE_CANDIDATE_REVISION="$CANDIDATE_REVISION" \
cargo test -p opy-rs --lib resource_baseline -- --ignored --nocapture
```

The command must be run from the `opy-rs` repository root. It reads the immutable `baselineRevision` from the checked-in `compiler-resources-baseline.json`; it must not recompute the baseline from moving `origin/main`. Set `CANDIDATE_REVISION` to the revision being measured. The JSON printed at the end is a candidate comparison record; the checked-in record remains the pinned before record for this audit.

To intentionally refresh the pinned record, run the command at the audited default-branch revision with `OPY_RESOURCE_BASELINE_REVISION` set to that revision and without `OPY_RESOURCE_CANDIDATE_REVISION`, then review the workload identities and environment before replacing the JSON file.

For a before/after comparison, run the command once at the audited `origin/main` revision and once at the candidate revision, keeping OS, architecture, Rust version, build profile, and workload parameters unchanged. Compare elapsed time directionally and compare mechanism counters exactly; do not turn a single-machine timing into a fixed percentage contract. Repeat the command when a directional result matters and report run count and environmental noise with the result.

The real-project case is intentionally the small, successful `overpy-parabola` fixture so a complete lowering/emission path is exercised without making a large corpus failure the benchmark's success condition. Its committed `fixture.json`, source hash, upstream commit, license, and source URL remain the provenance authority.

## QuickJS lifecycle conclusion (#300)

The macro evidence must be read from the `macro-runtime` and `macro-runtime-heavy` records produced by the command above. The phase counters attribute the current fresh-engine design without changing it; the isolation regression suite in `src/macro_js/tests/isolation.rs` independently covers globals, prototype mutation, helper bindings, console output, exceptions, interrupted execution, and concurrent runtime instances.

The accepted conclusion is **no change**: retain one fresh QuickJS runtime/context per invocation. Runtime/context creation and teardown are measurable parts of a successful invocation, but the evidence does not establish a material end-to-end bottleneck that justifies introducing lifecycle state. A shared mutable context remains outside the contract because it would require preserving global/prototype/helper/console isolation, exception recovery, resource limits, and concurrency behavior. No shared-context implementation is part of this issue.

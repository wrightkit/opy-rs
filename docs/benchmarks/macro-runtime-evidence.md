# QuickJS macro-runtime evidence

This record addresses [opy-rs#300](https://github.com/wrightkit/opy-rs/issues/300). It was produced on 2026-09-16 from candidate revision `ee0151ece63fcbc47d1516a8ae65603223224883`, using the pinned baseline revision `5e65d504529f8790ae4dcf4b76bbe1b2b94d70a6`, macOS arm64, Rust 1.97.1, the test profile, and five child-process samples per workload.

The command and workload identities are defined in [`compiler-resources.md`](compiler-resources.md) and [`resource_baseline.rs`](../../crates/opy-rs/src/compiler/tests/resource_baseline.rs). The original `macro-runtime` input is the #296 workload `(x + 2).toString();` with `x = 40`, repeated 64 times. `macro-runtime-heavy` uses the same invocation count and evaluates a 64-element `Array.from`/`vect`/`map`/`join` script. Both are synthetic AGPL-3.0-or-later audit inputs; the emitted candidate record includes their SHA-256 identities.

The following are median aggregate values across the five samples. Times are for all 64 invocations in a workload, not one invocation.

| Workload | Total | Runtime/context creation | Host registration | Builtin evaluation | User-script evaluation | Runtime/context teardown | Peak RSS range |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `macro-runtime` | 22.908 ms | 14.779 ms | 0.074 ms | 2.449 ms | 0.575 ms | 4.940 ms | 5.10–5.44 MiB |
| `macro-runtime-heavy` | 36.125 ms | 15.006 ms | 0.075 ms | 2.501 ms | 13.399 ms | 5.059 ms | 5.45–5.81 MiB |

Creation and teardown are substantial for the tiny script, but the representative heavier script makes user evaluation comparable to the lifecycle cost. The five samples were directionally stable; this is local evidence, not a fixed performance contract.

## Conclusion

No change is justified. Keep the fresh QuickJS runtime/context per invocation. The measured startup cost is real, but the selected workloads do not establish a material end-to-end bottleneck that warrants introducing reusable lifecycle state. The isolation suite independently covers globals, prototype mutation, helper bindings, console output, exceptions, interrupted execution, and concurrent runtime instances. A shared mutable context would change those existing resource and isolation invariants, so no lifecycle reuse implementation is included in this issue.

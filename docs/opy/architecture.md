# opy-rs Architecture — compatibility pointer

The current architecture contracts are maintained under
[`docs/architecture/`](../architecture/README.md):

- [`language-core.md`](../architecture/language-core.md) — OverPy semantic ownership, upstream executable-specification scope, typed implementation model, and feature locality;
- [`workshop-boundary.md`](../architecture/workshop-boundary.md) — canonical Workshop dependency/lowering/reconstruction boundary.

This path is retained for existing links. The previous "accepted living architecture" combined current contracts, implementation mechanisms, capability status, and references to the semantic manifest. That content remains available in Git history but is no longer the current architecture authority.

Current support must be established from [`../language-support.md`](../language-support.md) and executable evidence. Current implementation reality comes from source, tests, Cargo metadata, corpus/differential evidence, and real-project workflows. ADRs or older design prose do not prove either.

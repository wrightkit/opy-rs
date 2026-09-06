# opy-rs Current Architecture

This directory routes the **current** architecture contracts for `opy-rs`.

Use it for substantive implementation preflight. Architecture intent, implementation reality, compatibility evidence, and historical design notes are separate:

- documents here state the current durable language/ownership contracts;
- source, Cargo metadata, tests, corpus, differential evidence, and real projects establish implementation reality;
- `docs/language-support.md` records current evidenced support, not the definition of what the OverPy core language is;
- older implementation documents under `docs/opy/` describe prior/current mechanisms but are not semantic authority merely because code already uses them.

## Routing

| Concern | Current contract / authority |
| --- | --- |
| OverPy language ownership, upstream core scope, semantic implementation model | [`language-core.md`](language-core.md) |
| Boundary with canonical Workshop and source→Workshop / Workshop→source responsibilities | [`workshop-boundary.md`](workshop-boundary.md) |
| Current evidenced support | [`../language-support.md`](../language-support.md) plus executable evidence |
| Pinned upstream identity and provenance | [`../compatibility/upstream-references.md`](../compatibility/upstream-references.md) |
| Opy HIR wire/semantic representation | [`../hir/opy-hir-v2.md`](../hir/opy-hir-v2.md) where still consistent with current contracts and code reality |
| Tooling/API contract | [`../opy/tooling-api.md`](../opy/tooling-api.md) |
| Provider integration | [`../opy/provider.md`](../opy/provider.md) |

Do not put release versions, feature counts, current Issue progress, or transient compiler gaps in this directory. If an Issue or older design document conflicts with these contracts or current code reality, surface the mismatch before implementation.
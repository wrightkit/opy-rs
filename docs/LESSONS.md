# Lessons

Short review queue of reusable failure classes from merged opy-rs work. Each
entry names a prevention action and the evidence behind it. Promote an entry
into a regression test, an owner contract, or an existing engineering standard,
then remove it here. The procedure is the `merged-pr-retro` skill in
`wrightkit/.agents`.

### Documents and declared status may only claim what executed checks verify

- **Rule:** When writing or changing a support page, architecture contract, or
  fixture/test status declaration, verify each claim against the gate or
  contract it cites. A claim that outruns the executed check — support asserted
  for un-gated behavior, a fixture declared equivalent while the gate reports a
  regression, a "current reality" statement falsified by its own patch — is a
  defect, not documentation polish.
- **Why:** Stale or inflated claims read as authoritative to humans and agents
  and silently widen the promised compatibility surface; review has had to
  correct the same class five times in two weeks.
- **Prevention home:** existing policy — the workspace `AGENTS.md`
  documentation-synchronization invariant and this repository's support-claims
  rule; review surface `test-design-review`. Escalation: #556
  (closed not-planned).
- **Incidents:** [#196](https://github.com/wrightkit/opy-rs/pull/196),
  [#222](https://github.com/wrightkit/opy-rs/pull/222),
  [#252](https://github.com/wrightkit/opy-rs/pull/252),
  [#327](https://github.com/wrightkit/opy-rs/pull/327),
  [#341](https://github.com/wrightkit/opy-rs/pull/341),
  [#570](https://github.com/wrightkit/opy-rs/pull/570) — observed 2026-09-06,
  2026-09-08, 2026-09-12, 2026-09-19, 2026-09-20, 2026-10-16

### The linked issue's scope is an implementation boundary

- **Rule:** Implement what the linked issue assigns to the PR — no more.
  Behavior the issue keeps shared, assigns to a sibling stage or a later issue,
  or explicitly excludes stays out even when the code could carry it. When the
  issue's contract and the code disagree, report the mismatch instead of
  implementing around it.
- **Why:** Review repeatedly found work the linked issues had assigned
  elsewhere or excluded: shared indentation infrastructure made parser-local, a
  post-compile hook stage persisted in the wrong module, a sibling issue's
  matcher built early, and a structured field the contract excludes. Each cost
  a blocked review round and an in-flight correction.
- **Prevention home:** existing policy — `wrightkit/.github`
  `docs/pr-review.md` linked-issue scope check, applied at authoring time.
  Escalation: #558 (closed not-planned).
- **Incidents:** [#210](https://github.com/wrightkit/opy-rs/pull/210),
  [#214](https://github.com/wrightkit/opy-rs/pull/214),
  [#467](https://github.com/wrightkit/opy-rs/pull/467) — observed 2026-09-07,
  2026-10-06

### The MSRV gate catches syntax the local toolchain accepts
- **Rule:** Code must compile on the declared `rust-version` (1.88.0), so the workspace gates run on that toolchain — `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings` — before pushing. Syntax stabilized after the MSRV (for example let-chains in 1.88) compiles under a newer local toolchain and fails only in the `Rust (1.85.0)` CI job.
- **Why:** A PR written under a newer local toolchain shipped let-chains and needed a pushed fix after the MSRV job failed; the same class hit wright#743 the same day. `workshop-rs` already carried the same rule in its `AGENTS.md` since 2026-10-15, but the rule had not propagated to sibling repositories.
- **Prevention home:** the `cargo +1.88.0` pre-push gate now stated in `AGENTS.md` (ported from workshop-rs#457); the `Rust (1.85.0)` CI job remains the backstop.
- **Incidents:** [opy-rs#574](https://github.com/wrightkit/opy-rs/pull/574), observed 2026-10-18; same class in [wright#743](https://github.com/wrightkit/wright/pull/743) the same day, recorded in wright's `docs/LESSONS.md` and the escalation issue.

### A scope-defining enumeration needs an exhaustive audit, not a hand-picked list
- **Rule:** When a change enumerates which members belong to a contract class — which diagnostics count as `syntaxError`, which declaration forms upstream accepts — audit every member against the authority (emit sites, the pinned oracle) and pin the enumeration by a test asserting each member lands in the list or a documented exclusion, before merge.
- **Why:** #578 narrowed `lpp/validateEdits` `syntaxError` to document-local malformation with a hand-built allowlist; two follow-up PRs each found omitted document-local codes that then returned `{"valid": true}` — malformed text the contract must reject was silently accepted. #580 accepted the reported indexed declaration (`subroutine <name> <index>`) without auditing the sibling forms the oracle accepts; indexed initializers stayed rejected until #598 was filed, and the stale reconstruct refusal survived to #583.
- **Prevention home:** an exhaustive diagnostic-code classification test in this repository — every emitted code appears exactly once in the `syntaxError` list or a documented exclusion list; the same enumeration discipline against the oracle for acceptance families.
- **Incidents:** [opy-rs#578](https://github.com/wrightkit/opy-rs/pull/578) merged incomplete; omissions fixed by [opy-rs#587](https://github.com/wrightkit/opy-rs/pull/587) and [opy-rs#588](https://github.com/wrightkit/opy-rs/pull/588) — observed 2026-10-18. [opy-rs#580](https://github.com/wrightkit/opy-rs/pull/580) merged incomplete; sibling forms fixed by [opy-rs#610](https://github.com/wrightkit/opy-rs/pull/610) (fixes [#598](https://github.com/wrightkit/opy-rs/issues/598)) and the stale refusal removed by [opy-rs#583](https://github.com/wrightkit/opy-rs/pull/583) — observed 2026-10-18, 2026-10-19

### Sibling commands consume the shared acceptance path, not a re-derived subset
- **Rule:** When two commands judge the same input — `check` beside `compile` — the second must run the established acceptance path (or its extracted core), never a cheaper re-derivation under different defaults such as a fixed locale.
- **Why:** `check` resolved enum member spellings under a fixed `en-US` locale while `compile` used the request's active locale, so `check` passed a real 205-file project that `compile` rejected (`unknown enum member spelling 'Map.格里姆火山'`). The same class as `wright`'s sibling-surface divergences and `workshop-rs#534`, tracked under `wrightkit/workshop-rs#451`.
- **Prevention home:** `check` now runs the compile-stage acceptance pass under the request locale (#623); the `check`/`compile` enum-spelling parity test landed in `wright#852`.
- **Incidents:** [opy-rs#623](https://github.com/wrightkit/opy-rs/pull/623) (fixes [opy-rs#620](https://github.com/wrightkit/opy-rs/issues/620)) — observed 2026-10-20

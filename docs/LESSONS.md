# Lessons

Short review queue of reusable failure classes from merged opy-rs work. Each
entry names a prevention action and the evidence behind it. Promote an entry
into a regression test, an owner contract, or an existing engineering standard,
then remove it here. The procedure is the `wrightkit-retro` skill in
`wrightkit/.agents`.

### Semantic acceptance and emission need oracle evidence in both directions

- **Rule:** Before changing what a construct accepts, rejects, or emits, run
  the minimal case through the pinned OverPy oracle (`overpy@9.7.17`) in both
  directions — what it accepts and what it rejects — and record any intentional
  divergence as an approved exception. Do not infer behavior from the Workshop
  catalog, a sibling spelling, or a plausible reading of upstream source.
- **Why:** Eleven fixes in nine days repaired divergences that shipped in
  releases and surfaced through real projects and benchmark rounds: `None` and
  bare `playervar` names accepted where the oracle rejects them, numeric `Team`
  literals and empty call forms rejected where it accepts, hero-settings keys
  misplaced, `len()` and unknown-key settings emitted differently. Review
  caught the same class before merge more than a dozen additional times, so the
  defect recurs at authoring time, not only at release time.
- **Prevention home:** executable — oracle differential coverage over the
  escaping construct classes (`tools/overpy/` probes and the corpus gate);
  standing rule in `wrightkit/.github` `docs/compatibility-testing.md`.
  Escalation: #554.
- **Incidents:** [#468](https://github.com/wrightkit/opy-rs/pull/468),
  [#509](https://github.com/wrightkit/opy-rs/pull/509),
  [#520](https://github.com/wrightkit/opy-rs/pull/520),
  [#533](https://github.com/wrightkit/opy-rs/pull/533),
  [#534](https://github.com/wrightkit/opy-rs/pull/534),
  [#535](https://github.com/wrightkit/opy-rs/pull/535),
  [#536](https://github.com/wrightkit/opy-rs/pull/536),
  [#538](https://github.com/wrightkit/opy-rs/pull/538),
  [#547](https://github.com/wrightkit/opy-rs/pull/547),
  [#548](https://github.com/wrightkit/opy-rs/pull/548),
  [#550](https://github.com/wrightkit/opy-rs/pull/550) — observed 2026-10-06,
  2026-10-09, 2026-10-10, 2026-10-12, 2026-10-14

### A compatibility check must exercise the reference, not a copy of the implementation

- **Rule:** A gate, probe, or test that claims oracle parity must execute the
  oracle path or compare recorded oracle output. A second hard-coded table, a
  first-divergence-only report, an exception matcher that ignores recorded
  values, or a status that cannot tell which side failed does not verify the
  claim — and removing a gate without a replacement silently drops the check it
  performed.
- **Why:** Guards passed while hiding real divergence: a structural comparison
  reported only the first divergent line so a recorded exception masked sibling
  errors, exception matching never compared recorded values, a probe was
  consumed only by the oracle-side validator, and a consistency test compared
  two hand-maintained copies of the same names.
- **Prevention home:** existing policy — `wrightkit/.github`
  `docs/testing-policy.md` requires every expected result to have an
  independent source; review surface `wrightkit-test-design-review`.
  Escalation: #555.
- **Incidents:** [#327](https://github.com/wrightkit/opy-rs/pull/327),
  [#370](https://github.com/wrightkit/opy-rs/pull/370),
  [#379](https://github.com/wrightkit/opy-rs/pull/379),
  [#397](https://github.com/wrightkit/opy-rs/pull/397),
  [#467](https://github.com/wrightkit/opy-rs/pull/467),
  [#468](https://github.com/wrightkit/opy-rs/pull/468) — observed 2026-09-19,
  2026-09-25, 2026-09-26, 2026-10-06

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
  rule; review surface `wrightkit-test-design-review`. Escalation: #556.
- **Incidents:** [#196](https://github.com/wrightkit/opy-rs/pull/196),
  [#222](https://github.com/wrightkit/opy-rs/pull/222),
  [#252](https://github.com/wrightkit/opy-rs/pull/252),
  [#327](https://github.com/wrightkit/opy-rs/pull/327),
  [#341](https://github.com/wrightkit/opy-rs/pull/341) — observed 2026-09-06,
  2026-09-08, 2026-09-12, 2026-09-19, 2026-09-20

### An issue closes when its acceptance criteria verify, not when its PRs merge

- **Rule:** A PR may close a tracker or parent issue only after the issue's
  verifiable outcome is demonstrated end-to-end. A stack that lands partial
  scope keeps the parent open and uses `Refs` until the remainder merges.
- **Why:** On 2026-08-31 four convergence PRs closed their tracker issues on
  merge; a next-morning audit reopened all of them, and #141 was closed
  prematurely twice. #170 was reopened within two hours of its closing PR, and
  #192 stayed closed for thirteen days before residual gaps reopened it.
- **Prevention home:** existing policy — `wrightkit/.github`
  `docs/issue-readiness.md` verifiable outcomes. Escalation: #557.
- **Incidents:** [#146](https://github.com/wrightkit/opy-rs/pull/146),
  [#147](https://github.com/wrightkit/opy-rs/pull/147),
  [#148](https://github.com/wrightkit/opy-rs/pull/148),
  [#149](https://github.com/wrightkit/opy-rs/pull/149),
  [#171](https://github.com/wrightkit/opy-rs/pull/171),
  [#195](https://github.com/wrightkit/opy-rs/pull/195) — observed 2026-08-31,
  2026-09-01, 2026-09-03, 2026-09-19

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
  Escalation: #558.
- **Incidents:** [#210](https://github.com/wrightkit/opy-rs/pull/210),
  [#214](https://github.com/wrightkit/opy-rs/pull/214),
  [#467](https://github.com/wrightkit/opy-rs/pull/467) — observed 2026-09-07,
  2026-10-06

### Provider results must project the resolved core outcome, not re-derive it

- **Rule:** Provider-side projection of check results, diagnostics, and
  document sets consumes the effective entry, resolved source context, and
  failure state that core tooling produced — not the initially loaded text, a
  `None`-model fallback, or preprocessing rules reimplemented with different
  ordering.
- **Why:** LPP-facing results diverged from standalone `check` on redirected
  entries, errored models, and multi-file include sets; one instance shipped in
  a release and returned as a bug report.
- **Prevention home:** regression tests over redirected-entry, failure-state,
  and multi-file include inputs asserting parity with standalone `check`.
  Escalation: #559.
- **Incidents:** [#257](https://github.com/wrightkit/opy-rs/pull/257),
  [#456](https://github.com/wrightkit/opy-rs/pull/456),
  [#480](https://github.com/wrightkit/opy-rs/pull/480) — observed 2026-09-13,
  2026-10-05, 2026-10-08

# oracle-both-directions

**Applies when:** a change alters what an OverPy construct accepts, rejects, or emits.

**Principle:** Run the minimal case through the pinned OverPy oracle before changing behavior, and check both what it accepts and what it rejects. Record an intentional divergence as an approved exception. Never infer behavior from the Workshop catalog, a sibling spelling, or a plausible reading of upstream source.

**Changes the choice between:** trusting a reasonable inference about the construct, and spending a probe run to observe the oracle.

**Enforced by:** partly. Oracle differential probes and the corpus gate catch escaped classes after the fact; extending that coverage is tracked in #554. Judgment decides to probe before editing.

**Why:** a long run of fixes repaired divergences that had already shipped: names accepted where the oracle rejects them, literals and empty call forms rejected where it accepts them, settings keys misplaced or emitted differently. Review caught the same class before merge many more times.

**Detail:** [language-core.md](../architecture/language-core.md#compatibility-target); [upstream-references.md](../compatibility/upstream-references.md)

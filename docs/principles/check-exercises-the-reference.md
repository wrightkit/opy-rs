# check-exercises-the-reference

**Applies when:** writing or changing a gate, probe, or test that claims oracle parity, or removing one.

**Principle:** A parity claim must execute the oracle path or compare recorded oracle output. A second hand-kept table, a first-divergence-only report, an exception matcher that ignores recorded values, or a status that cannot say which side failed does not verify it. Removing a gate without a replacement drops the check it performed.

**Changes the choice between:** a check that is cheap and stays green, and one that can fail on a real divergence from the reference.

**Enforced by:** judgment. The independent-source rule for expected results is in the organization testing policy; `wrightkit-test-design-review` is the review surface.

**Why:** guards passed while hiding divergence: a comparison reported only the first divergent line so a recorded exception masked sibling errors, exception matching never compared recorded values, and a consistency test compared two hand-kept copies of the same names.

**Detail:** [testing-policy.md](https://github.com/wrightkit/.github/blob/main/docs/testing-policy.md)

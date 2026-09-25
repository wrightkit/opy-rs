# ADR-0007: Filtered-word escaping as enumerated output facts

- Status: Accepted
- Date: 2026-09-25
- Related history: [OPY Issue #374](https://github.com/wrightkit/opy-rs/issues/374), [OPY Issue #366](https://github.com/wrightkit/opy-rs/issues/366), [OPY PR #370](https://github.com/wrightkit/opy-rs/pull/370)

## Context

When the pinned OverPy 9.7.10 writes a rule name or the `Mode Name` and
`Description` settings strings, it removes a few invisible formatting
characters and inserts a soft hyphen (U+00AD) into a fixed set of about 30
words, each matched anywhere or only as a whole word, case-insensitively, in a
fixed order. The words are those the Overwatch client is believed to censor;
the soft hyphen keeps pasted code from being rejected. opy-rs splits only
`rigger` and `admin` in rule names and nothing in settings strings, so any
such text differs structurally from the reference.

The [compatibility target](../architecture/language-core.md#compatibility-target)
requires structural convergence with the pinned output. The
[source policy](../compatibility/source-policy.md) builds the core from
observed behavior and prohibits importing upstream implementation or data
tables. Unlike the Blizzard Global glyph facts, these words cannot be found by
probing the oracle without already knowing them: the set can only be read from
upstream and then confirmed against the oracle. PR #370 implemented the set,
and review removed it because no decision allowed this provenance.

The client's own censor list is not observable in full and differs from the
upstream set, so it cannot be the source for convergence with the reference.

## Decision

The filtered-word escaping is part of the reference's observable output
contract, and opy-rs reproduces it for rule names and the `main.modeName` and
`main.description` settings strings.

The word set is accepted as **enumerated output facts**: facts whose content is
fully determined by the pinned output, so that every converging implementation
must contain the same entries in the same order. The set is recorded under the
conditions in the source policy: an opy-rs-owned representation attributed to
the pin, every entry confirmed by an oracle-backed fixture, and changes only
with an oracle pin change.

This decision approves only this set. It is not a precedent for importing any
other upstream table; another set of enumerated output facts needs its own
owner decision.

## Consequences

Rule names and settings strings containing a filtered word converge
structurally with the reference. The core carries a small static word list
whose content mirrors upstream, with its provenance recorded in the source
policy. Settings values that are enumerations are never escaped. Warnings for
censored variable names and client-side censor checks are outside this
decision.

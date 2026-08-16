# OPY Semantic Compatibility Manifest — Specification

Status: accepted specification — opy-rs-owned semantic contract, ported and
adapted from the WrightKit evidence base (issue #2); implemented by the
frontend workstream (issues #4/#5) and merged on `main` (PRs #9/#13)
Scope: the opy-rs-owned representation for builtin action/value identities,
member functions, signatures, parameter enum-domain identities (catalog
links), and source aliases; reference-validated and consumed by the native
frontend. The implementation lives in `crates/opy-frontend/src/manifest/`
(data in `data/manifest.json`, probe evidence in `probes/`); this document is
the schema and boundary contract for that data.

## Purpose and boundary

The compatibility inventory (issue #2) concluded that a machine-readable,
opy-rs-owned manifest is justified: the declared parse surface exceeds the
initial semantic surface, and residual `unknown-action`/`unknown-value`/
`unsupported-member` gaps are semantic-coverage gaps, not grammar gaps. The
manifest replaced the hardcoded `KNOWN_ENUMS` table (removed from
`crates/opy-frontend/src/lower.rs` in the ownership-fixed frontend, PR #9)
with data and gives the frontend a single, reference-validated source for:

* builtin actions and values (generic and member);
* member-function metadata (receiver + argument signatures);
* signatures, argument names/order, and defaults;
* parameter enum-domain **identities** as catalog links (the domain a
  parameter requires, e.g. `Invis`, `Status`, `Transform`, `ChaseTimeReeval`),
  **not** the authoritative member lists of those domains;
* source aliases (non-contextual rewrites such as `stopChasingVariable`).

It is **language-compatibility metadata**, distinct from:

* the Workshop emission/localization layer (en-US spellings, emitter
  output), owned by `workshop-rs`; the manifest links to it by canonical id
  (`catalogId`) rather than duplicating spellings;
* authoritative Workshop enum member lists, hero/map/mode/settings content,
  locale spellings, and canonical member/domain existence — Workshop-owned
  catalog content that the frontend **never approximates**: member accesses on
  a declared domain identity resolve as opaque identities, and
  member/domain/catalog validation stays `lowering-dependent` (#8); and
* a runtime content registry (heroes/maps/abilities content data, extension
  boundaries, independent version identities) — deferred; this inventory
  found no architecture trigger that requires opening one.

## Data model (schema v1)

```jsonc
{
  "schemaVersion": 1,
  "reference": {
    "name": "overpy",
    "version": "9.7.10",
    "contentCommit": "889d9749d1def17f146548cbddb94ea1ab015847",
    "integrity": "sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw=="
  },
  "functions": [
    {
      "id": "chaseOverTime",          // manifest id; opy-rs-owned spelling
      "kind": "action",               // action | value | memberAction | memberValue
      "receiver": "Player",           // members only: Player | Variable | String | Any
      "params": [
        { "name": "variable", "variable": true },
        { "name": "destination" },
        { "name": "duration" },
        { "name": "reevaluation", "domain": "ChaseTimeReeval",
          "default": "DESTINATION_AND_DURATION" }
      ],
      "catalogId": "chaseOverTime",   // link to the Workshop emission catalog id
      "evidence": ["chase-over-time"] // oracle probes validating this entry
    },
    {
      "id": "chase",                  // the reference's keyword special form
      "kind": "action",
      "params": [
        { "name": "variable", "positionalOnly": true, "variable": true },
        { "name": "destination", "positionalOnly": true },
        { "name": "rate", "keywordOnly": true, "alternateNames": ["duration"] },
        { "name": "reevaluation", "domain": "ChaseReeval" }
      ],
      "contextualDomain": {
        "domain": "ChaseReeval",      // resolves only in this signature's context
        "by": "rate",                 // the keyword spelling selects the option
        "options": {
          "rate":     { "domain": "ChaseRateReeval", "target": "chaseAtRate" },
          "duration": { "domain": "ChaseTimeReeval", "target": "chaseOverTime" }
        }
      },
      "evidence": ["chase-keywords", "chase-reeval-context"]
    }
  ],
  "aliases": [
    { "source": "stopChasingVariable", "target": "stopChasing",
      "kind": "functionAlias", "evidence": ["aliases"] }
  ],
  "provenance": {
    "generator": "opy-rs semantic compatibility manifest v1 (opy-rs-authored; probe-validated against the pinned OverPy 9.7.10 oracle)",
    "reviewed": true,
    "license": "AGPL-3.0-or-later"
  }
}
```

There is **no `enumDomains` table**: the manifest records enum *domain
identities* as parameter metadata (`"domain": "ChaseTimeReeval"`) and as the
contextual `chase` dispatch record, but it does not carry authoritative
Workshop member lists. A member access on a declared domain identity resolves
as an opaque `enum` node; canonical member-existence and domain validation is
Workshop-owned catalog content and stays `lowering-dependent` (#8).

Entry semantics:

* `kind` — `action`/`value` are generic builtins; `memberAction`/`memberValue`
  are receiver methods whose `params` are the **explicit** arguments (the
  receiver is separate). The frontend enforces action/value position
  (`value-in-action-position`, `action-in-value-position`).
* `receiver` — the declared receiver category. `Player` is metadata for
  player-oriented members (the pinned reference does not type-check those
  receivers, so the frontend accepts any receiver); `Variable` and `String`
  are enforced where the reference semantics are clear (`.append` requires an
  assignable receiver, `.format` a string literal).
* `params` — ordered arguments. Arity is `(first defaulted/optional param
  index, params.len())`; `"optional": true` marks an omittable argument
  without an emitted expansion, `"default"` an expansion value. Only
  enum-member defaults are expanded at lowering (e.g. `chaseOverTime(g, 10,
  3)` fills `DESTINATION_AND_DURATION`, matching the reference emission).
  `"unbounded": true` (`.format` placeholders) accepts any argument count.
  Named/keyword argument binding consumes these parameter names directly —
  they are the reference's declared parameter names (e.g. `wait` binds
  `time`/`waitBehavior`, `print` binds `text`, `len` binds `array`):
  * `"keywordOnly": true` — the argument must be passed as `name = expr`
    (the reference `chase` form requires its 3rd argument to be
    `rate = ...` or `duration = ...`);
  * `"positionalOnly": true` — keyword binding is rejected for this
    parameter (the `chase` form's leading arguments);
  * `"alternateNames": [...]` — additional accepted keyword spellings
    (`chase` accepts both `rate` and `duration` for its 3rd parameter);
  * `"variable": true` — the argument must be a variable reference (a
    `globalvar` or a `playervar`); the chase family requires a variable
    first argument to select the global/player emission form.
* `keywordArgs` — whether the entry accepts keyword arguments at all;
  defaults to `true` (the pinned reference's generic binder applies to
  every workshop function). Entries the reference routes around that
  mechanism declare `"keywordArgs": false` (`range`, `random.*`,
  `.format`).
* `contextualDomain` — the `chase` dispatch record: a merged enum domain
  (`ChaseReeval`) that has no standalone member list and resolves **only**
  within this entry's signature context, selected by the keyword spelling
  bound to the `by` parameter. Each option maps a keyword spelling to the
  concrete enum domain and the function the call lowers to (`rate` →
  `ChaseRateReeval` / `chaseAtRate`; `duration` → `ChaseTimeReeval` /
  `chaseOverTime`). The contextual domain is deliberately *not* a declared
  enum domain: a bare `ChaseReeval.MEMBER` outside the `chase` signature is
  rejected like the reference rejects it.
* `context` — a call-context restriction; `"forIterable"` (`range`) is only
  valid as a `for ... in` iterable.
* `catalogId` — the canonical Workshop emission catalog id; absent for
  special emission forms (`debug`/`print`, `append` via Modify) or emission
  surfaces not yet catalog-covered (documented gaps). Catalog linkage is a
  `workshop-rs` integration concern.
* `evidence` — every entry must reference at least one probe recording
  oracle acceptance (deterministic `check` failure otherwise).

Entries carry the minimal semantic data the frontend needs to resolve names,
check arity, resolve enum domains, and lower; they deliberately omit upstream
description/localization text.

## Data provenance and licensing rule

The manifest is **opy-rs-authored data validated against observed oracle
behavior**. It must not be produced by mechanically converting OverPy's
GPL-3.0 TypeScript data files (`src/data/*.ts`) into the manifest: the
clean-room policy in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)
forbids importing OverPy implementation details into the core, and observed
behavior through documented compatibility tests is the permitted input. Every
entry records the reference probe that validates it (`probes/probes.json`
carries the probe source hash, expected oracle status, normalized emission
hash, and — for rejections — the diagnostic category fragment).

## Validation rules

* `Manifest::load` (`crates/opy-frontend/src/manifest`) — schema validation,
  duplicate/colliding ids, colliding or missing aliases, declared parameter
  domain identities (tracked for the frontend's opaque member resolution),
  enum-member defaults requiring a declared domain, keyword-binding data
  sanity (`keywordOnly`/`positionalOnly` are mutually exclusive, alternate
  spellings do not collide with other parameters), contextual-domain
  integrity (the contextual domain is not a standalone declared identity, the
  selector parameter exists and its keyword spellings cover the options,
  every option domain is declared), and entries lacking oracle evidence all
  fail deterministically; a canonical-rewrite test pins the data file to its
  byte-canonical form, and a cross-check test pins every `catalogId` (and
  contextual option target) to the Workshop emission catalog once the
  `workshop-rs` catalog contract exists.
* `probes/validate.py` — reference validation: every probe runs against the
  pinned oracle and must match its recorded accept/reject, normalized
  emission hash, and diagnostic category. The probe set and validator are
  frontend-workstream-owned; the validator requires the pinned oracle (Node +
  pnpm) and runs standalone like `compatibility/run_oracle.py`, so it is not
  part of the oracle-less harness suite. The probe names are recorded as
  evidence references in `compatibility/support-matrix.json`, and the native
  differential suite (`crates/opy-frontend/tests/differential.rs`, merged in
  PR #13) covers the same surface end-to-end in `cargo test`.
* The frontend consumes the manifest in `lower.rs`: unknown names, wrong
  action/value position, invalid arity, invalid receiver category, and
  named/keyword argument binding (`unknown-keyword`, `duplicate-argument`,
  `missing-argument`, `positional-after-keyword`, `keyword-required`,
  `keyword-unsupported`, `invalid-argument`) produce structured,
  source-located frontend diagnostics before Workshop emission. Workshop
  enum member/domain mismatch checks were removed from the core in PR #9
  (they require canonical Workshop catalog knowledge) and are
  `lowering-dependent` (#8); custom user-declared enum member validation is
  OPY-level source semantics and stays frontend-owned.

## Consumers

* the opy-rs frontend (`crates/opy-frontend`) — name/member/enum resolution,
  arity and signature checks, early resolution of unknown-action/value
  errors;
* `workshop-rs` — canonical-id linkage to the emission catalog and
  member/domain/catalog validation (validated by the cross-check test at
  integration time, issue #8);
* differential and systematic reference tests (the native differential suite
  and the oracle-required probe validator);
* documentation, agents, and future release metadata can consume the same
  declared boundary.

## Non-goals

* A runtime-downloadable or hot-updating content registry.
* Workshop content data (heroes/abilities/maps) as manifest entries.
* Contextual aliases beyond the manifest's declared data, raw-Workshop enum
  resolution beyond the declared contextual domains, and further
  named-argument shapes without reference/corpus evidence.
* Preserving upstream implementation structure for its own sake.

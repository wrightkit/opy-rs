# Opy HIR v1: opy-rs frontend protocol

Status: accepted baseline for v0.1; opy-rs-owned contract (adopted from the
WrightKit evidence base, issue #2)
Scope: the interchange format produced by the opy-rs native frontend and
consumed by opy-rs tooling (and, later, WrightKit tooling consumers)

This document is the normative specification for the Opy HIR protocol version
`1.1.0`. It defines the JSON payload that the native frontend in
`crates/opy-frontend` (the lowering stage) emits and that the Rust consumer
in the same crate validates and consumes. The frontend parses `.opy` source
directly and owns the mapping from OPY syntax onto this schema; no component
imports or wraps the reference implementation's AST (clean-room boundary,
[`upstream-references.md`](../compatibility/upstream-references.md)).

The protocol is an opy-rs-owned contract. It is not `JSON.stringify()` of an
OverPy AST, and no node in it is named after an OverPy-internal class. Node
kinds, operator spellings, and structural choices are opy-rs's.

> Wire-identity note: the protocol name `wright/opy-hir` and the recorded
> generator identities below are preserved verbatim from the original
> contract so that existing WrightKit consumers remain compatible. A future
> rename of the protocol identity string is a major-version contract change
> and must be reviewed together with every consumer.

## 1. Goals

The protocol must:

1. describe the parsed program semantics the v0.1 compatibility corpus needs:
   declarations, rules, events, conditions, statements, and expressions;
2. preserve file, line, and column provenance so later stages can report
   diagnostics against source;
3. be deterministic: the same source and frontend version produce
   byte-identical JSON;
4. be versioned so a producer and consumer can agree on compatibility without
   inspecting each other's implementation;
5. fail loudly on constructs the frontend cannot map, and be rejected or
   reported by the consumer rather than silently ignored.

## 2. Protocol envelope

Every payload is a JSON object with the following top-level fields.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `protocol` | object | yes | Protocol identity and version. |
| `generator` | object | yes | Producer identity for provenance. |
| `files` | array | yes | Source-file registry referenced by spans. |
| `defines` | array | no | Preprocessor constant/function macros seen by the frontend. |
| `declarations` | array | yes | Symbols declared at program scope, grouped by kind, each group in declaration order. |
| `rules` | array | yes | Rule and subroutine-definition bodies, in source order. |
| `settings` | object | no | The typed custom-game-settings block, when the source had one (§2.5). |

### 2.1 `protocol`

```jsonc
{
  "name": "wright/opy-hir",
  "version": "1.1.0"
}
```

* `name` must be exactly `wright/opy-hir`.
* `version` is a semantic version (`major.minor.patch`). The major component
  is the compatibility boundary described in §7. The `1` in `v1` refers to
  this major version.

### 2.2 `generator`

```jsonc
{
  "name": "wright/opy-native",
  "version": "0.1.0",
  "frontend": "overpy@9.7.10"
}
```

* `name` identifies the producer.
* `version` is the producer's own version.
* `frontend` records the exact external frontend identity (package and
  version) the producer translated from, so compatibility evidence can name
  the reference. The opy-rs native frontend records its own identity here
  (`FRONTEND_NAME` = `wright/opy-native`, version = the crate version); the
  pinned reference identity is `overpy@9.7.10` (content commit
  `889d9749d1def17f146548cbddb94ea1ab015847`, see
  [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)).

### 2.3 `files`

```jsonc
[
  { "id": 0, "path": "source.opy" },
  { "id": 1, "path": "shared.opy" }
]
```

* `id` is a non-negative integer, unique within the payload.
* `path` is the file name as the frontend reported it, unique within the
  payload. Paths are recorded for diagnostics; they are not canonicalized by
  the protocol.

### 2.4 `defines`

Preprocessing definitions (`#!define` constants and function macros) that the
frontend expanded before parsing. They are recorded for provenance so a
diagnostic can explain where a value came from; they carry no semantic
payload because expansion already happened.

```jsonc
{ "name": "CAKE_SIDE_LENGTH", "isFunction": false, "span": { "file": 0, "start": { "line": 10, "col": 1 }, "end": { "line": 10, "col": 24 } } }
```

### 2.5 `settings` (v1.1.0, additive)

The typed custom-game-settings block (`settings { ... }` in the source),
carried as an ordered tree so the consumer can validate it against the
fixture-evidenced emission table and emit the Workshop `settings` section.
The payload is opy-rs-owned typed data (JSONC parsed by the producers), not
a raw text blob: enum-ness and list domains are table data at validation and
emission, never wire data.

```jsonc
{
  "span": { "file": 0, "start": { "line": 7, "col": 1 }, "end": { "line": 31, "col": 2 } },
  "children": [
    {
      "kind": "group", "name": "gamemodes",
      "children": [
        {
          "kind": "group", "name": "assault",
          "children": [
            { "kind": "list", "name": "enabledMaps", "elements": [], "span": { ... } },
            { "kind": "string", "name": "roleLimit", "value": "2OfEachRolePerTeam", "span": { ... } }
          ],
          "span": { ... }
        }
      ],
      "span": { ... }
    }
  ]
}
```

Node grammar:

* `settings` has an optional `span` (the whole block) and a `children` array.
* Every child is an object discriminated by `kind` (`group`, `number`,
  `bool`, `string`, `list`), carries a non-empty `name` (the source key) and
  an optional `span` covering the key..value member region.
* `group` has a `children` array; `number` carries an `f64` `value`; `bool`
  carries a boolean `value`; `string` carries a string `value`; `list` has an
  `elements` array of `{ "value": string, "span": optional }` objects.
* A valid block must contain a `gamemodes` group. Domain checks (known keys,
  known enum values, known map/hero list elements) run at validation against
  the declared emission-table contract; the table data itself is Workshop
  data owned by `workshop-rs` (integration boundary, §8).

## 3. Source provenance

Every node that originates from source carries a `span`. A span is a
half-open interval in a file:

```jsonc
{ "file": 0, "start": { "line": 6, "col": 5 }, "end": { "line": 6, "col": 21 } }
```

* `file` indexes into `files`.
* `line` and `col` are 1-based. `end` is exclusive: it is the position just
  past the last character of the node.
* A synthetic node (for example a compiler-generated initializer) carries the
  span of the source text that caused it to exist, or is omitted when no
  source text exists.

Declaration, rule, and `subroutineDef` nodes additionally carry an optional
`name_span` field (wire spelling `name_span`): the exact source span of the
identifier token (the declared or defined identifier, or the rule name inside
its string literal) when the frontend can record it. It is optional and
omitted when absent; it is never emitted as `null`, and it has the same shape
and validation as any other span (§8). The native frontend records it; the
differential suite's normalization strips `span`-family fields from the
per-fixture native wire-payload artifact (`target/opy-differential/`) as
documented frontend-internal provenance. Protocol and generator identities
are kept, and the oracle comparison itself uses status, rule-name, and
diagnostic evidence rather than span data.

Spans are for diagnostics and identity, not for byte-accurate reconstruction.
The frontend producer is responsible for emitting them; the consumer
validates them (§8). A span whose end would precede its start (for example a node expanded
from a preprocessor macro that mixes call-site and definition-site positions)
must be normalized to a degenerate interval anchored at the start, so every
emitted span is structurally valid.

## 4. Declarations

Declarations appear in `declarations` in source order. Each is an object
discriminated by `kind`. All kinds carry `name` and `span` unless noted.

### 4.1 `globalVariable` / `playerVariable`

```jsonc
{
  "kind": "globalVariable",
  "name": "score",
  "index": null,
  "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 17 } },
  "initializer": null
}
```

* `index` is the explicit index the source requested (`globalvar x 5`), or
  `null` when the frontend assigns it later.
* `initializer` is an expression or `null`. It is present only when the source
  provided a non-trivial initializer; the frontend's implicit defaults are
  not emitted.
* `name_span` is the exact span of the declared identifier token (see §3).

### 4.2 `subroutine`

A subroutine declaration (`subroutine name`), independent of any `def`.

```jsonc
{ "kind": "subroutine", "name": "showStatus", "index": null, "span": { ... } }
```

* `name_span` is the exact span of the declared identifier token (see §3).

### 4.3 `subroutineDef`

A subroutine definition (`def name():`) with its statement body. A definition
is a program body, so it appears in `rules` (§5) rather than in
`declarations`; this section defines its node shape.

```jsonc
{
  "kind": "subroutineDef",
  "name": "showStatus",
  "span": { ... },
  "body": [ /* statements */ ]
}
```

* `name_span` is the exact span of the identifier in `def name():` (see §3).

### 4.4 `constant`

A source-level constant (`macro name = value`), kept so constant references
stay resolvable.

```jsonc
{
  "kind": "constant",
  "name": "PI",
  "span": { ... },
  "value": { /* expression */ }
}
```

### 4.5 `macro`

A source-level function macro (`macro name(a, b):`). Macro *calls* remain
explicit in expressions (§6.9); the definition is recorded so a later stage
can expand or lower it without re-parsing source.

```jsonc
{
  "kind": "macro",
  "name": "double",
  "args": ["value"],
  "span": { ... },
  "body": [ /* statements */ ]
}
```

## 5. Rules

A rule is an object with the fields below. Each entry in `rules` is either a
rule object or a `subroutineDef` node (§4.3). Rules appear in source order.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | The rule name as written (empty is allowed for delimiter rules). |
| `span` | span | yes | The `rule` line. |
| `name_span` | span | no | The exact span of the rule name inside its string literal, when the frontend records it. |
| `disabled` | boolean | yes | `true` when the rule is disabled by annotation. |
| `event` | event | yes | The rule's event. |
| `conditions` | array | yes | `@Condition` expressions, in source order. |
| `actions` | array | yes | Statements, in source order. |

### 5.1 Event

An event is an object with `name`, `args`, and `span`:

```jsonc
{ "name": "global", "args": [], "span": { ... } }
{ "name": "eachPlayer", "args": [], "span": { ... } }
{ "name": "onFlag", "args": [ { "kind": "string", "value": "FLAG", "span": { ... } } ], "span": { ... } }
```

`name` is the event keyword as written. `args` are the event's parameters as
expressions.

## 6. Statements and expressions

Statements and expressions are JSON objects discriminated by `kind`. A node
whose `kind` the consumer does not recognize is an *unsupported node* (§7.3).

### 6.1 Statement kinds

| Kind | Fields | Meaning |
| --- | --- | --- |
| `expr` | `expr`, `span` | An expression statement (typically a call with side effects). |
| `assign` | `target`, `value`, `span` | Assignment. Compound assignments are desugared by the frontend. |
| `if` | `branches`, `else`, `span` | Conditional. `branches` is an array of `{ "condition", "body" }`; `else` is an array of statements or `null`. |
| `for` | `variable`, `iterable`, `body`, `span` | Iteration. `variable` is an expression naming the loop variable (a `globalVar` reference). |
| `while` | `condition`, `body`, `span` | Loop. |
| `doWhile` | `body`, `condition`, `span` | Loop whose body executes before its condition. |
| `switch` | `value`, `arms`, `span` | Source-order arms; execution falls through until a `break` or the end of the switch. Each arm is tagged `case` or `default`; a case has `value` and `body`, while a default has `body`, and each arm may carry `span`. At most one default arm is valid. |
| `break` | `span` | Exit the innermost switch or loop; invalid contexts are rejected by the frontend. |
| `callSubroutine` | `name`, `span` | Call a subroutine by name. |
| `pass` | `span` | A no-op emitted by the frontend. |

Example `for` with `if`:

```jsonc
{
  "kind": "for",
  "variable": { "kind": "globalVar", "name": "index", "span": { ... } },
  "iterable": { "kind": "call", "name": "range", "args": [ { "kind": "number", "value": 3, "text": "3", "span": { ... } } ], "span": { ... } },
  "body": [
    {
      "kind": "if",
      "branches": [
        { "condition": { "kind": "binary", "op": "==", "left": { ... }, "right": { ... }, "span": { ... } }, "body": [ { "kind": "expr", "expr": { "kind": "call", "name": "debug", "args": [ { ... } ], "span": { ... } }, "span": { ... } } ] }
      ],
      "else": null,
      "span": { ... }
    }
  ],
  "span": { ... }
}
```

### 6.2 Expression kinds: literals

| Kind | Fields | Meaning |
| --- | --- | --- |
| `number` | `value`, `text`, `span` | Numeric literal. `value` is the JSON number; `text` is the source spelling. |
| `string` | `value`, `span` | String literal without format placeholders. |
| `bool` | `value`, `span` | `true` or `false`. |
| `null` | `span` | The null literal. |
| `array` | `elements`, `span` | Array literal, possibly empty. |
| `vector` | `x`, `y`, `z`, `span` | Vector literal (`vect(x, y, z)`). |
| `enum` | `type`, `value`, `span` | A built-in enumerated value, e.g. `Team.ALL`, `Color.WHITE`, `Beam.GRAPPLE`. `type` is the value domain, `value` the member name. |

### 6.3 Expression kinds: references

| Kind | Fields | Meaning |
| --- | --- | --- |
| `globalVar` | `name`, `span` | Reference to a global variable. |
| `playerVar` | `player`, `name`, `span` | Reference to a player variable on `player` (an expression). |
| `eventPlayer` | `span` | The `eventPlayer` pseudo-symbol. |
| `constant` | `name`, `span` | Reference to a source-level constant. |

### 6.4 Expression kinds: operations

| Kind | Fields | Meaning |
| --- | --- | --- |
| `call` | `name`, `args`, `span` | Function call. `name` is the function name; member calls use `receiver` (below). |
| `receiverCall` | `receiver`, `name`, `args`, `span` | Member/extension call: `receiver.name(args)`. |
| `macroCall` | `name`, `args`, `span` | A source-level macro invocation kept explicit. |
| `macroParam` | `name`, `span` | A reference to a macro parameter inside a macro definition body. |
| `binary` | `op`, `left`, `right`, `span` | Binary operation. `op` is one of `+ - * / % ** == != < <= > >= and or`. |
| `unary` | `op`, `operand`, `span` | Unary operation. `op` is `-` or `not`. |
| `index` | `array`, `index`, `span` | Indexing `array[index]`. |
| `format` | `text`, `args`, `span` | A string with `{0}`, `{1}` style placeholders and their argument expressions. |

### 6.5 Operator semantics

Operators are opy-rs spellings for the semantics the frontend parsed:

* arithmetic: `+ - * / % **`;
* comparison: `== != < <= > >=` (non-strict, Workshop semantics);
* logical: `and or`, with `not` as a unary operator.

The frontend maps parsed OPY operator syntax onto these fixed spellings. The
consumer treats `op` as an opaque string and validates it only structurally
(§8).

## 7. Versioning and compatibility rules

### 7.1 Version meaning

The protocol version is semver. Within the same major version:

* **Additive change**: a producer may add new optional fields to existing
  nodes and new `kind` variants for constructs the consumer can treat as
  opaque *only if* the consumer is updated to understand them. A consumer
  must reject a `kind` it does not recognize (§7.3), so an additive change
  ships with a matching consumer update and does not require a major bump.
* **Breaking change**: removing or renaming a node, changing the meaning of
  a field, or changing required-ness is a major-version change. Consumers of
  an older major version must reject the payload before inspecting its
  contents.

Minor and patch versions describe producer-visible evolution inside a major
version (documentation, new optional producer metadata) and do not change the
node grammar.

### 7.2 Major-version handling

A consumer must check `protocol.name` and `protocol.version` before any other
validation. If `name` is not `wright/opy-hir`, or the major version is not
supported, the consumer returns a structured *incompatible protocol* error
that names the expected and received identity. It must not attempt to parse
the program body.

### 7.3 Unsupported nodes

A node with an unknown `kind` (or an unknown statement/expression variant) is
an *unsupported node*. The consumer reports a structured error that names the
node kind and its span, so a regression report is explicit. Unsupported is
never a silent pass: the frontend refuses to emit nodes it cannot map, and the
consumer refuses to consume nodes it cannot understand.

## 8. Validation requirements

A consumer must validate, in order:

1. **Envelope**: `protocol` identity and major version (§7.2).
2. **Shape**: the payload is a JSON object with the required top-level fields;
   `files`, `declarations`, `rules` are arrays.
3. **Provenance**: every span's `file` indexes an entry in `files`; line and
   column values are ≥ 1; `end` is not before `start`.
4. **Identifiers**: `declarations` names are non-empty strings; within a
   declaration kind, names are unique; rule names are strings (may be empty);
   `defines` names are unique.
5. **References**: `globalVar`, `playerVar`, and `constant` references resolve
   to a matching declaration; `callSubroutine` references resolve to a
   `subroutine` declaration or a `subroutineDef` in `rules`; loop variables in
   `for` resolve to a global variable.
6. **Settings**: every settings node's span is valid; keys are non-empty; a
   `gamemodes` group is present; every leaf key resolves against the declared
   settings emission-table contract (`settings-unknown-key` otherwise); enum
   values and list elements resolve against their declared table domains
   (`settings-unknown-value` otherwise). The table data itself is Workshop
   data owned by `workshop-rs` (integration boundary); until it exists the
   consumer validates structure and placement only.
7. **Unsupported nodes**: unknown node kinds produce the §7.3 error.

Validation failures are structured: they carry a stable code, a message, and
the offending span or path when available. Human-readable wording is not part
of the stable contract; the code and structured fields are.

## 9. Determinism and debug output

For the same input and frontend version, the producer must
emit byte-identical JSON: object keys are emitted in a fixed order and
collections (files, declarations, rules, branches, args) preserve source
order. The consumer's debug dump (§10) must be stable for the same validated
payload so tests and issue reports can compare dumps byte-for-byte.

## 10. Debug dump

The consumer provides a deterministic, human-readable rendering of a
validated payload, intended for tests and issue reports. It is an
implementation-defined presentation, not part of the wire contract. It must:

* be reproducible byte-for-byte for the same validated payload;
* show protocol identity, files, declarations, settings (when present),
  rules, events, conditions, statements, and expressions with their spans;
  and
* print in a stable order matching the payload order.

## 11. Out of scope for v1

The following are intentionally not modeled in v1 and are rejected by the
frontend as unsupported when encountered:

* rule labels and relative gotos (`__skip__` / `__distanceTo__` forms);
* decompilation-only constructs;
* semantic analysis beyond structural validation (type checking, dead code,
  optimization).

These are not promises; they are the v0.1 boundary. A construct that appears
in the corpus and is not listed here is a bug in this specification, not a
reason to extend the schema silently.

## 12. Ownership

* The protocol contract is owned by opy-rs and lives in this document.
* The native frontend owns all knowledge of how OPY source maps to this
  schema. It never imports or depends on the reference implementation's
  types (clean-room boundary,
  [`upstream-references.md`](../compatibility/upstream-references.md)).
* Changes to the node grammar require a review of this document, the frontend
  producer, the Rust consumer, and the corpus fixtures together (see
  [`docs/opy/support-matrix.md`](../opy/support-matrix.md) and
  [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)).

## 13. Version history

* **1.1.0** (v1-additive): adds the optional `settings` payload (§2.5) with
  typed settings nodes, validation checks (§8 item 6), and a settings dump
  section. No existing node or field changed; consumers of the 1.x major
  accept the payload unchanged (`check_envelope` gates the major only).
  The opy-rs native frontend emits 1.1.0.

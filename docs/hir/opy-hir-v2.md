# Opy HIR v2: ordered switch-arm protocol

Status: accepted opy-rs-owned major migration for issue #47.

The current wire contract is `wright/opy-hir@2.0.0`. It retains the Opy HIR
v1 payload shape and validation rules unless this document says otherwise.
The complete v1 baseline remains in [`opy-hir-v1.md`](opy-hir-v1.md).

## Breaking change

`Stmt::Switch` is serialized as:

```json
{
  "kind": "switch",
  "value": { "kind": "globalVar", "name": "selector" },
  "arms": [
    { "kind": "default", "body": [] },
    { "kind": "case", "value": { "kind": "number", "value": 1 }, "body": [] }
  ]
}
```

`arms` is an ordered array. It replaces the v1 `cases` plus `default`
fields so default-before-case fallthrough is lossless. A v1 consumer must
reject a v2 payload at the protocol envelope before inspecting its body, and
the v2 consumer rejects protocol major 1 for the same reason.

The producer emits `2.0.0`, and the opy-rs HIR parser validates major `2`.
The v1 and v2 grammars are not silently accepted under one version.

## Additive expression node

HIR v2 adds the `conditional` expression node without changing the retained
v1 baseline:

| Kind | Fields | Meaning |
| --- | --- | --- |
| `conditional` | `thenValue`, `condition`, `elseValue`, `span` | Conditional value `thenValue if condition else elseValue`; chained forms are right-associative. |

The condition is parsed as an `or` expression, and the else branch accepts
another conditional value.

## Additive player reference

HIR v2 also supports the `hostPlayer` pseudo-symbol as a source reference.
Player-variable `for` binders are represented by the existing `playerVar`
expression with the receiver preserved in `player`; this includes receivers
such as `eventPlayer`, `hostPlayer`, and a player-valued variable expression.
The optional `member_span` field preserves the exact source span of the player
variable member identifier (for example, `I` in `hostPlayer.I`); `span`
continues to cover the complete member expression.

## Additive statement nodes

The source frontend also retains the audited control-flow statements that are
not yet representable in canonical Workshop WIR:

| Kind | Fields | Meaning |
| --- | --- | --- |
| `delete` | `target`, `span` | Delete an element addressed by an array index. |
| `continue` | `span` | Continue the innermost loop; source lowering rejects it outside a loop. |
| `goto` | `label`, `offset`, `span` | Jump to a named label or to a relative `loc+` offset; exactly one target field is present. |
| `label` | `name`, `span` | A named jump target. |

These nodes are source-semantic and preserve provenance. The bounded compiler
reports an explicit integration diagnostic for them until canonical WIR owns
the corresponding Workshop control-flow semantics.

## Consumer migration

Every external `wright/opy-hir` consumer must migrate its protocol gate and
switch representation together. This opy-rs change does not modify another
repository's consumer; that coordination belongs to the owning consumer
repository.

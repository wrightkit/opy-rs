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

## Consumer migration

Every external `wright/opy-hir` consumer must migrate its protocol gate and
switch representation together. This opy-rs change does not modify another
repository's consumer; that coordination belongs to the owning consumer
repository.

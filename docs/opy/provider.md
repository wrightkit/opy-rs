# OPY LPP provider

`opy-provider` is the first-party stdio process for the Language Provider
Protocol. It is an owner-side adapter over the existing `opy-rs` frontend and
compiler; it does not expose OPY AST/HIR or Workshop WIR.

## Protocol and capabilities

The provider serves language id `opy` and the `opy` extension. It supports LPP
`1.0` for document-supplied requests, LPP `1.1` for file-entry project loading,
LPP `1.2` for provider-owned directory targets, LPP `1.3` for source identity,
and LPP `1.4` for compile artifact format negotiation.

| Capability | Method | Behavior |
| --- | --- | --- |
| Check | `lpp/check` | Loads the selected entry's OPY project and returns source diagnostics. |
| Compile | `lpp/compile` | Uses the same project loading path and returns canonical Workshop text when clean. |
| Project loading | `lpp/check`, `lpp/compile` | LPP 1.1 loads from a client-selected file entry; LPP 1.2 also lets the provider select the default entry from a directory target. |

All other LPP v1 capabilities are advertised as unavailable until they are
implemented end to end.

## Entry-based project loading

For the entry/project-loading extension defined by
`language-provider-protocol#16`, the client must negotiate LPP 1.1. Then
`lpp/check` and `lpp/compile` accept an `entry` object without a complete
`documents` map:

```json
{
  "entry": {
    "uri": "file:///workspace/main.opy",
    "languageId": "opy",
    "version": 7
  },
  "projectRoot": "file:///workspace"
}
```

The provider reads the entry from the filesystem. Its parent directory is the
include root, so `#!mainFile`, reachable `#!include` files, preprocessing, and
macros are resolved by the existing `opy-rs` pipeline. `projectRoot` remains
informational. A single supplied `Document` is also accepted for document-set
compatibility; multi-file OPY loading uses the entry path instead of requiring
the client to enumerate the source closure.

Results identify every file discovered by preprocessing with a `file://` URI.
Owner source spans are converted to LPP's zero-based UTF-16 positions, and
every filesystem-loaded result echoes the entry version. Missing or unreadable
required files fail the request with the structured `projectLoadFailed` error;
unsupported entry URI, language, or version uses `invalidEntry`.

Document-supplied requests remain available in both protocol versions. The
provider analyzes every supplied document from that request snapshot. A
document-supplied compile request with more than one document is refused with
`compile.requiresSingleDocument` because the OPY compiler emits one project
artifact.

With LPP 1.2, a client may send the same `entry` object with
`"kind": "directory"`. The provider passes that directory to the OPY owner
project loader, which selects `main.opy` or `src/main.opy` and resolves the
remaining source closure. Wright does not enumerate OPY files or interpret
project directives.

## Compile artifact

Successful compilation returns the opaque LPP artifact:

```json
{
  "format": "workshop-rs/text-v1",
  "content": "<canonical Workshop text>"
}
```

The artifact contains only emitted Workshop text. It is `null` whenever
compilation produces an error-severity diagnostic.

### Mapped artifact (LPP 1.4)

A client that lists `workshop-rs/mapped-text-v1` in `acceptedArtifactFormats`
receives that format, and the provider returns the first listed format it can
produce (`workshop-rs/text-v1` or `workshop-rs/mapped-text-v1`). Without the
field, or when only `text-v1` is listed, the output is unchanged. When no listed
format is producible, the request is refused with
`compile.artifactFormatUnsupported`; the field is rejected with `-32602` before
LPP 1.4.

The mapping is built with the `workshop-rs` `SourceMap` from the spans lowering
attaches, and is checked against the re-parsed Workshop text before it is
returned. Attribution is owned here:

- Included files map to their own document URIs, which are the `files` entries
  of the artifact.
- Macro-expanded code maps to the invocation site, never into the macro
  definition.
- Generated helper nodes (initializer rules, translation helpers) carry no span.

Columns are 1-based Unicode scalar values, matching `Position`.

## Running locally

```sh
cargo run --release -p opy-provider
```

The process reads one JSON-RPC request per UTF-8 line from stdin and writes one
response per line to stdout. Human-readable logging belongs on stderr; the
provider emits no protocol data there.

Release archives are target-specific and named
`opy-provider-<version>-<target>.tar.gz`, with a matching `.sha256` checksum.

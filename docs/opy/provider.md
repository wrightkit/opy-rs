# OPY LPP provider

`opy-provider` is the first-party stdio process for the Language Provider
Protocol. It is an owner-side adapter over the existing `opy-rs` frontend and
compiler; it does not expose OPY AST/HIR or Workshop WIR.

## Capabilities

The provider serves language id `opy` and the `opy` extension. Its initial
capabilities are:

| Capability | Method | Behavior |
| --- | --- | --- |
| Check | `lpp/check` | Loads the selected entry's OPY project and returns source diagnostics. |
| Compile | `lpp/compile` | Uses the same project loading path and returns canonical Workshop text when clean. |

All other LPP v1 capabilities are advertised as unavailable until they are
implemented end to end.

## Entry-based project loading

For the entry/project-loading extension owned by
`language-provider-protocol#16`, `lpp/check` and `lpp/compile` accept an
`entryUri` file URI without a complete `documents` map:

```json
{
  "entryUri": "file:///workspace/main.opy",
  "locale": "en-US"
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
filesystem-discovered documents use version `0` because they are not client
overlays.

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

## Running locally

```sh
cargo run --release -p opy-provider
```

The process reads one JSON-RPC request per UTF-8 line from stdin and writes one
response per line to stdout. Human-readable logging belongs on stderr; the
provider emits no protocol data there.

Release archives are target-specific and named
`opy-provider-<version>-<target>.tar.gz`, with a matching `.sha256` checksum.

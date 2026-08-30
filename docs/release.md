# Release automation

`release-plz` maintains the release PR, publishes the workspace crates to
crates.io, and creates the canonical `vX.Y.Z` tag.

The two publishable workspace packages share one version group: `opy-rs` and
`opy-cli`. Compiler lowering and the JavaScript macro runtime are internal
`opy-rs` modules, not independently published packages. The release PR updates
package versions, internal dependency versions, and the generated changelog.
Merging that PR into `main` runs `release-plz release`; pull-request heads do
not publish packages.

Previously published versions of the removed packages remain available; only
future publication is removed.

The protected GitHub Actions `release` environment must provide
`CARGO_REGISTRY_TOKEN`, able to publish both packages. The repository
Actions secrets must provide `GH_TOKEN`, a fine-grained token with repository
Contents and pull-request read/write access for release PR and tag operations.
Credentials must never be committed.

The publication job uses a stable concurrency group with
`cancel-in-progress: false`. Normal CI remains the source-change quality gate;
release-specific checks should include `actionlint` and package dry-runs:

```sh
cargo package --locked -p opy-rs
cargo package --locked -p opy-cli
```

These checks do not prove publication. Completion requires observing all package
versions on crates.io and the matching tag after a real release. If publication
fails, correct the failed package and rerun the release path; do not republish
an already-published version under a different version.

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

## First-party provider artifacts

The tag-driven `provider-release` workflow builds `opy-provider` for the
supported Wright targets and uploads one archive plus checksum per target to the
same GitHub Release. Artifact names are
`opy-provider-<version>-<target>.tar.gz` and
`opy-provider-<version>-<target>.tar.gz.sha256`. The archive contains only the
provider executable (`opy-provider` or `opy-provider.exe`); it is independent of
the crates.io publication path.

After the GitHub Release upload succeeds, the same archives and checksums are
published to the WrightKit R2 distribution bucket. The public, version-pinned
URLs are:

```text
https://releases.wrightkit.dev/opy-rs/releases/<version>/opy-provider-<version>-<target>.tar.gz
https://releases.wrightkit.dev/opy-rs/releases/<version>/opy-provider-<version>-<target>.tar.gz.sha256
```

The version is the release version without the `v` tag prefix. R2 publication
does not rebuild the provider. Each object is written with a conditional
create, and an existing object is accepted only when its bytes exactly match
the release artifact; version-pinned objects are therefore immutable. The
publication job downloads every public object and verifies both byte identity
and the archive SHA-256 before it succeeds. The URLs use long-lived immutable
cache semantics and do not provide a moving `latest` alias.

The R2 publication requires repository Actions secrets
`R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, and `CLOUDFLARE_ACCOUNT_ID`. These
credentials should be limited to the shared `wrightkit-release` bucket. GitHub
Releases remain the canonical release and provenance record; downstream
repositories own migration to these URLs and must keep their own consumer
tests and rollout evidence.

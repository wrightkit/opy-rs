# Release automation

`release-plz` maintains the release PR, publishes the workspace crates to
crates.io, and creates the canonical `vX.Y.Z` tag. The `opy-rs` GitHub Release
is created as a draft (`git_release_draft` in `release-plz.toml`) and becomes
public only after the provider artifact set is verified.

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

After `release-plz` creates the release draft and tag, the `release-plz`
workflow's `provider` matrix builds `opy-provider` for the supported Wright
targets and stages one archive plus checksum per target. Artifact names are
`opy-provider-<version>-<target>.tar.gz` and
`opy-provider-<version>-<target>.tar.gz.sha256`. The archive contains only the
provider executable (`opy-provider` or `opy-provider.exe`); it is independent of
the crates.io publication path.

Two jobs consume the same staged artifacts without rebuilding:
`publish-provider-github` uploads them to the draft release (draft assets are
not public), and `publish-provider-r2` publishes them to the WrightKit R2
distribution bucket. The public, version-pinned URLs are:

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
cache semantics. Publication does not reconcile an existing version: GitHub
asset upload refuses duplicate names without clobbering, and R2 uses a native
conditional create that refuses an existing object. A rerun therefore fails
without overwriting either published copy and requires explicit maintainer
recovery for any partial release; a draft release abandoned by a permanently
failed pipeline is deleted manually.

Once the draft carries the complete artifact set and the R2 objects pass
public verification, `promote-release` publishes the GitHub Release. Only then
does `advance-latest` write the released semantic version as plain text to:

```text
https://releases.wrightkit.dev/opy-rs/latest/version
```

This pointer uses `Cache-Control: no-store` and is the only moving provider
object. Provider archives and checksums are not duplicated under `latest/`;
consumers resolve the pointer and then download exclusively from the immutable
`/opy-rs/releases/<version>/...` paths.

The R2 publication requires repository Actions secrets
`R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, and `CLOUDFLARE_ACCOUNT_ID`. These
credentials should be limited to the shared `wrightkit-release` bucket. GitHub
Releases remain the canonical release and provenance record; downstream
repositories own migration to these URLs and must keep their own consumer
tests and rollout evidence.

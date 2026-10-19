# Reconstruction fixtures

This directory contains Workshop source inputs used by the reconstruction
tests in `crates/opy-rs/src/compiler/reconstruct.rs`. Each file is a raw
Workshop text export that `workshop_rs::parser` parses into the canonical
`Program`; the test reconstructs OPY, reparses it through the OPY frontend,
compiles, and compares the result with the canonical comparator.

These files are ordinary test data. They are never imported by core code and
never bundled into core builds or release artifacts. The general corpus
policy in [`../corpus/README.md`](../corpus/README.md) and the repository's
source and licensing policy in `docs/compatibility/source-policy.md` apply.

## Upstream decompiler inputs

Pinned decompiler test inputs from the OverPy reference (GPL-3.0). They are
the Workshop text the upstream `decompileCustomGameSettings` path consumes
and anchor the reconstruction output to the same semantic content.

| File | Upstream path | SHA-256 |
| --- | --- | --- |
| `customGameSettings.txt` | `src/tests/decompiler/customGameSettings.txt` | `1ba106636ab368abf2b17e6b2966ca68d2c2a7a02fd992c51f6ab4d1d30b9c44` |
| `shionSettings.txt` | `src/tests/decompiler/shionSettings.txt` | `ad74946d95a005f926dbd98075d2765b4a7543a8aac69ba5e51f142777c2340a` |

- Origin: `Zezombye/overpy` (OverPy)
- License: GPL-3.0 (`LICENSE` in the upstream repository)
- Redistribution: retained as test fixtures only; not distributed in release artifacts
- Source commit: `5a7d0e294b8cad73b9701987bb584d0551d7fa4d`
- Source URL: `https://github.com/Zezombye/overpy/tree/5a7d0e294b8cad73b9701987bb584d0551d7fa4d/src/tests/decompiler/`
- License URL: `https://github.com/Zezombye/overpy/blob/5a7d0e294b8cad73b9701987bb584d0551d7fa4d/LICENSE`

`customGameSettings.txt` contains the variant-qualified map entry
`Workshop Chamber 972777519512064577`, which the canonical settings model
parses as a map-list element but cannot resolve through the current map
catalog (`workshop_rs::settings::check_emission` reports
`unknown map 'Workshop Chamber 972777519512064577'`). The fixture therefore
pins the `unsupported-settings` refusal path until a canonical
representation exists; the canonical-side gap is tracked as
`wrightkit/workshop-rs#518`.

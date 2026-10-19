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

Pinned decompiler test inputs from the OverPy reference (GPL-3.0): the
complete `src/tests/decompiler/*.txt` set the upstream `decompileAllRules`
tests consume. `classification.json` records each input's pinned SHA-256
and its current coverage classification (`equivalent`, `refused` with the
reconstruction refusal codes, or `unparseable` when the canonical
`workshop_rs::parser` rejects the input); the corpus gate in
`crates/opy-rs/tests/reconstruction_corpus.rs` verifies the record.

| File | Upstream path | SHA-256 |
| --- | --- | --- |
| `customGameSettings.txt` | `src/tests/decompiler/customGameSettings.txt` | `1ba106636ab368abf2b17e6b2966ca68d2c2a7a02fd992c51f6ab4d1d30b9c44` |
| `mysteryUltimates.txt` | `src/tests/decompiler/mysteryUltimates.txt` | `b2b1ab8dd81a585e024795f1326d32142196c85794a0da679af89e85cdef1cd1` |
| `shionSettings.txt` | `src/tests/decompiler/shionSettings.txt` | `ad74946d95a005f926dbd98075d2765b4a7543a8aac69ba5e51f142777c2340a` |
| `z_1v1arena.txt` | `src/tests/decompiler/z_1v1arena.txt` | `dafc37bd55677e726da68945db238c67a608affd9a7762266b6c4a1d988973ac` |
| `z_1v1buildyourhero.txt` | `src/tests/decompiler/z_1v1buildyourhero.txt` | `5dba2585627794758b1e3ad3c74b9f2d0cd039cc72ca773db28522a95a2103d0` |
| `z_anapaintball.txt` | `src/tests/decompiler/z_anapaintball.txt` | `3265e1c24a6b78d8e4c8ca34e96a060e271d1612f8a1326f50e41a6630f76914` |
| `z_basewars.txt` | `src/tests/decompiler/z_basewars.txt` | `8327b1ffb50dab12738aca183ed21c53d396579f09a93480e75eca5326ff3153` |
| `z_fivenightsatecopoint.txt` | `src/tests/decompiler/z_fivenightsatecopoint.txt` | `7839fd3827c42b4139f5ddf1e7f74a9998ca2eb7cd4ef39ee1bb8fb9ba6c3eb1` |
| `z_heatstreet.txt` | `src/tests/decompiler/z_heatstreet.txt` | `b9c73395a4a43fd90d3190f8adc2e3b85c5f12ca29b85e744d312d96f8a1a787` |
| `z_islandparty.txt` | `src/tests/decompiler/z_islandparty.txt` | `1ea9c1868b90dc7d4ba5d614b42e769ece9bc9212fb33bc6c0e78388d7425960` |
| `z_junkensteinsvictory.txt` | `src/tests/decompiler/z_junkensteinsvictory.txt` | `507a0fae24bf0575684575a8e5f775281d3cf00d340ff3293c8538ba1806e46a` |
| `z_lootquest.txt` | `src/tests/decompiler/z_lootquest.txt` | `b5310fd9dd0adbc8f0443d165db0ee320d2477a78af2b004737eaba147ddba29` |
| `z_mauganipplepve.txt` | `src/tests/decompiler/z_mauganipplepve.txt` | `da6d1a798c782b5d9a33590c372b7e4ca6de07b793809e63e14c717b7e42acfc` |
| `z_owware.txt` | `src/tests/decompiler/z_owware.txt` | `354a3b5963699e6e3d0d95242ea9e93ea28408fb3ff95b7cc474a6c7e193eddd` |
| `z_pictionary.txt` | `src/tests/decompiler/z_pictionary.txt` | `fc5101b153ce51c3caef1a446b849f0d51d9a89edbf289b9875576b40e7281e8` |
| `z_uno.txt` | `src/tests/decompiler/z_uno.txt` | `4d6dc1533d03e69d53f5e58ddb8fe757b4ba30201c00331b34043d2fceba890a` |

Upstream's pinned expected outputs (`src/tests/decompiler/results/<id>.opy`)
are not vendored: the gate compares the canonical input program with the
recompiled reconstruction under `workshop_rs::compare`, never upstream
output text.

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

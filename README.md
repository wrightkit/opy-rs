# opy-rs

Standalone, Workshop-independent Rust frontend for the OverPy `.opy` source
language, extracted as the OPY language provider within WrightKit.

Pipeline: `lexer → preprocess → CST/parser → semantic resolution → OPY semantic
model (Opy HIR)`, with a documented integration boundary toward `workshop-rs`
for canonical Workshop lowering and emission.

This repository is part of the WrightKit multi-repository workspace. Follow the
workspace-level `AGENTS.md` first, then this repository's local rules.

See the roadmap issues and `docs/` for the compatibility surface, support
matrix, and provenance policy.

License: AGPL-3.0-or-later (see `LICENSE`).

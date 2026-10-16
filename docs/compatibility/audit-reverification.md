# opy-rs adversarial audit — independent re-verification checklist

For another agent to re-verify every checked finding. Oracle: `overpy@9.7.17` at `tools/overpy/oracle/node_modules/overpy/cli.js` (content commit `5a7d0e294b8cad73b9701987bb584d0551d7fa4d`); the original audit ran against the `9.7.10` pin (commit `1e2688954302a402d076944b46db07efb14d7b61`). Native CLI: `target/debug/opy-cli` — **always `cargo build -p opy-cli` after pulling before comparing** (stale binaries caused false diffs during the audit).

Conventions: `$ORACLE` = `node tools/overpy/oracle/node_modules/overpy/cli.js compile -i FILE.opy --root .` · `$NATIVE` = `target/debug/opy-cli compile FILE.opy`. Put scratch `.opy` files under `target/audit-scratch/` (git-ignored).

## Verified bugs — confirm each is now fixed

- [ ] **#1 enum auto-increment** — `enum Mode:\n    OFF = 0\n    ON = 5\n    AUTO` → `Mode.AUTO` must emit `6` (was `2`). Both oracle and native now emit `Set Global Variable(g, 6);`.
  - Edge: `enum Neg:\n    A = -5\n    B\n    C` → `B = -4`, `C = -3`.
  - Edge: `X = "hi"` member inlines `Custom String("hi")` at use sites.
  - Error path: `enum E:\n    X = 1 + 2\n    Y` → both sides reject with `Cannot auto-increment enum member, as last value was operator '+' or '+='`. Also check `"s"` → `function '__customString__'`, `Hero.ANA` → `enum 'Hero'`, `vect(1,2,3)` → `function 'vect'`.
  - Tests: `compiler::integration_tests::compiler_semantics::enum_members_resume_counting_after_explicit_values`, `::enum_auto_increment_rejects_a_non_numeric_predecessor`.
  - Files: `cst.rs` (members now `(name, Option<Expr>, Span)`), `parser/declarations.rs` (keeps `= expr`), `lower.rs` (`EnumMember`/`EnumCounter`), `lower/declarations.rs` (`enum_members`), `lower/expressions.rs` (member lookup), `tools/overpy/diff.py` (`enum-increment` frontier).

- [ ] **#2 literal domain** — `g = 3 != false` → oracle `Not Equals(3, False)` (was folded `True`); `g = 3 == null` → `Equals(3, Null)`; `g = 1 in []` unrelated. Confirm `3 == 4` still folds `False`, `Hero.ANA == 3` stays `Compare`. File: `compiler/operator_optimization.rs` `fn literal`.

- [ ] **#3 `in` membership** — matrix all identical to oracle:
  - `1 in [1, x]` → `True` (AST-equal hit), `x in [x]` → `True`, `a[0] in [a[0]]` → `True`
  - `1 in [1.0, x]` → `Array Contains(…, 1)` keeps `x` only (spelling-aware)
  - `1 in [x, y]` → `Array Contains` reduced to non-literal survivors
  - `1 in [x]` → `Equals(1, x)`; `1 in []` → `False`; `x not in []` → `True` (not-wrapped)
  - `vect(1,2,3) in [vect(1,2,3), x]` → `True`
  - Files: `compile_time.rs::evaluate_membership`, `compiler/lowering.rs::membership_equal`, `compiler/lowering/values.rs` (`Expr::Binary{in}` arm).

- [ ] **#4 string repetition** — `g = "a" * 3` → `Custom String("aaa")` (oracle emits noncanonical `Multiply(CS,3)`; fold is the accepted deviation). `2 * "x"` → `"xx"`. `"a" * 0` / `"a" * -1` → `""`. `"x" * 2 + "y"` → `"xxy"`. `"a" / 2`, `-"a"` → `catalog-validation` error (no canonical equivalent — expected, not a bug).
  - File: `compile_time.rs::fold_string_repetition` (runs before `*0`/`*1` peepholes; bound 8192).

- [ ] **#5 string escapes** — `g = "A\x41B"` → `AAB` decoded; `"\q"` → `lex-error` (oracle: `Invalid escape sequence`); `"line\` + newline → error both sides; `"\uD800"` → U+FFFD both. Files: `lexer.rs::decode_string_escape` callers strict; settings lenient per upstream.

- [ ] **#6 debug literal splicing** — `debug("lit")` → format text contains `lit` inline (oracle identical); `debug(null)` → `0` inline; `debug(2.5)` → `2.50` inline; `debug(true)` → keeps `{0}` + `True` arg; `debug("a{0}b")` → arity error both sides.

- [ ] **#7 format arity** — `f"{0}".format()` and `f"{0}{1}".format(x)` → error `expected N argument(s)`; `"{0}".format(1,2)` → compiles (extra numbered dropped); `f"{}".format(1,2)` → error; `"{0}{}".format(a,b)` → mixing error; `"{{0}}"` literal braces OK.

- [ ] **#8 string modifiers** — `fw"{g}!"` (with global `g`) byte-identical to oracle; `p"x"` accepted as plain; `b"abc"` → error when no mappable char (oracle: same); `b"ab"`/`c"..."`/`w"..."` mappings match.

- [ ] **#9 f-string `#!define`** — `#!define S "a"` + `f"{S}"` → `a` inline (oracle identical); object and function macros expand inside `{…}` fragments. Files: `preprocess/macros.rs::expand_define_text`, `parser.rs` plumbing.

- [ ] **#10 string concat** — `g = "a" + "b"` → `Custom String("ab")` (oracle emits noncanonical `Add(CS,CS)` + `w_string_concat` warning — intentional deviation).

- [ ] **#11 settings/debug strings** — settings block string values with escapes/interpolation match oracle (covered by corpus fixtures + earlier probes).

## Checked non-bugs — confirm classification still holds

- [ ] `goto`/label/switch: oracle emits `"pass"` + `//done:` comments; compare **canonical WIR** (parse both through `workshop_rs::parser::parse`) — must be equal.
- [ ] `End;` after `If`: workshop-rs emits implicit close; oracle explicit. Canonical-equal.
- [ ] Number spellings `1e3 2E3 1.50 010 0b101 0o17`: oracle echoes invalid spellings; native normalizes. Canonical boundary.
- [ ] `12345678901234567890` → f64 `…560000`: no spelling channel in `workshop_rs::Value::Number`. Boundary.
- [ ] `7 % 0` / `NaN`: oracle emits `NaN` text; native rejects non-finite. Boundary.
- [ ] `del d["k"]`: oracle emits invalid call; native errors. Boundary.
- [ ] bare `loop`: oracle silently drops rule; native `unknown-identifier`. Invalid source — native safer.
- [ ] `.indexOf`/unknown members: both reject (message wording differs — `unknown-member` frontier is mapped).

## Gate commands (must all pass)

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p workshop-rs --bin workshop-catalog-gen -- check   # if workspace covers it
git diff --check
python3 tools/overpy/run_native.py   # 144 fixtures: 129 match + 15 known-gap expected
```

## Watchpoints for the re-checking agent

- Rebuild `opy-cli` before any CLI diff — the audit hit stale-binary false diffs twice.
- Distinguish *decorative* diffs (comments, `End;`, diagnostic wording) from *semantic* diffs by comparing canonical-parsed programs, not raw text.
- `catalog-validation` on `Add/Multiply/Divide` over `Custom String` is the deliberate canonical boundary: fold what is expressible, error on the rest.
- The `enums` table in `lower.rs` now stores `EnumMember { name, value }`; tooling/diagnostic consumers take `.name`.

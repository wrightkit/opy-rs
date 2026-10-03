// Audits manifest function parameter names against the pinned OverPy 9.7.10
// `funcKw` tables, the same tables `parseArgs` uses to bind `name=` keyword
// arguments. Every manifest param name must match the upstream arg name at
// its position, so upstream keyword spellings are accepted and others are
// rejected.
//
//   pnpm install --dir tools/overpy/oracle
//   node tools/overpy/audit_param_names.cjs
//
// Member-call args carry a leading receiver argument upstream; member params
// are compared against both `args.slice(1)` and `args`, because a few member
// entries (for example `.toArray`) declare no receiver arg.
//
// A manifest entry is exempt when it records why its names cannot track
// upstream: `catalogLink: "special-lowering"` (the binder parses the call
// itself), `keywordArgs: false` (no keywords are accepted at all), or
// `unbounded` (a variadic form the fixed upstream arg list does not model).
// Any other divergence, an `alternateNames` spelling beyond upstream, or a
// manifest entry with no upstream counterpart fails the audit.
const fs = require("fs");
const path = require("path");
const { loadFuncKw } = require("./oracle_func_kw.cjs");

const manifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "../../crates/opy-rs/src/manifest/data/manifest.json"), "utf8"),
);

loadFuncKw((funcKw) => {
  const divergent = [];
  const unmapped = [];
  const exempt = [];
  let checked = 0;
  for (const fn of manifest.functions) {
    const member = fn.kind.startsWith("member");
    const recorded =
      fn.catalogLink === "special-lowering" || fn.keywordArgs === false || fn.unbounded === true;
    const keys = [fn.id, fn.catalogId, `__${fn.id}__`, `__${fn.catalogId}__`]
      .filter(Boolean)
      .map((key) => (member ? `.${key}` : key));
    const upstream = keys.map((key) => funcKw[key]).find(Boolean);
    if (upstream === undefined) {
      (recorded ? exempt : unmapped).push(fn.id);
      continue;
    }
    const args = (upstream.args ?? []).map((arg) => arg.name);
    const params = (fn.params ?? []).map((param) => param.name);
    const alternate = (fn.params ?? []).flatMap((param) => param.alternateNames ?? []);
    if (recorded || alternate.length) {
      // A recorded reason waives name parity. `alternateNames` on an
      // otherwise non-exempt entry accepts spellings upstream rejects.
      (recorded ? exempt : divergent).push(
        recorded ? fn.id : [fn.id, params, args, alternate],
      );
      continue;
    }
    checked += 1;
    const alignments = member ? [args.slice(1), args] : [args];
    if (!alignments.some((a) => JSON.stringify(a) === JSON.stringify(params))) {
      divergent.push([fn.id, params, args, []]);
    }
  }
  for (const [id, params, args, alternate] of divergent) {
    console.log(`DIVERGENT ${id}: manifest=${JSON.stringify(params)} upstream=${JSON.stringify(args)}${alternate.length ? ` alternateNames=${JSON.stringify(alternate)}` : ""}`);
  }
  for (const id of unmapped) console.log(`UNMAPPED ${id}: no upstream funcKw entry`);
  console.log(
    `checked ${checked}, exempt ${exempt.length}, divergent ${divergent.length}, unmapped ${unmapped.length}`,
  );
  process.exit(divergent.length + unmapped.length ? 1 : 0);
});

// Audits manifest function parameter names against the pinned OverPy 9.7.10
// `funcKw` tables, the same tables `parseArgs` uses to bind `name=` keyword
// arguments. Every manifest param name must match the upstream arg name at
// its position, so upstream keyword spellings are accepted and others are
// rejected.
//
//   pnpm install --dir tools/overpy/oracle
//   node tools/overpy/audit_param_names.cjs
//
// Member calls pass the receiver to `parseArgs` ahead of the call args, so
// member params are compared against `args.slice(1)`. Entries that model
// that receiver slot as a param (for example `.toArray`) carry a recorded
// exemption instead.
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
    const bases = [fn.id, fn.catalogId].filter(Boolean);
    const keys = [...bases, ...bases.map((base) => `__${base}__`)].map((key) =>
      member ? `.${key}` : key,
    );
    const upstream = keys
      .map((key) => (Object.hasOwn(funcKw, key) ? funcKw[key] : undefined))
      .find(Boolean);
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
    const expected = member ? args.slice(1) : args;
    if (JSON.stringify(expected) !== JSON.stringify(params)) {
      divergent.push([fn.id, params, args, []]);
    }
  }
  let report = "";
  for (const [id, params, args, alternate] of divergent) {
    report += `DIVERGENT ${id}: manifest=${JSON.stringify(params)} upstream=${JSON.stringify(args)}${alternate.length ? ` alternateNames=${JSON.stringify(alternate)}` : ""}\n`;
  }
  for (const id of unmapped) report += `UNMAPPED ${id}: no upstream funcKw entry\n`;
  report += `checked ${checked}, exempt ${exempt.length}, divergent ${divergent.length}, unmapped ${unmapped.length}\n`;
  fs.writeSync(1, report);
  process.exit(divergent.length + unmapped.length ? 1 : 0);
});

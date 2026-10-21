// Member calls pass the receiver to `parseArgs` ahead of the call args, so
// member params are compared against `args.slice(1)`. Entries that model
// that receiver slot as a param (for example `.toArray`) carry a recorded
// exemption instead.
const fs = require("fs");
const path = require("path");
const { loadFuncKw } = require("./oracle_func_kw.cjs");

const manifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "../../crates/opy-rs/src/manifest/data/manifest.json"), "utf8"),
);

const EXEMPT = {
  async: "no upstream funcKw entry; the binder parses `async(subroutine, behavior)` itself",
  chase: "upstream funcKw lists `rateOrDuration`; the call is spelled `rate=` or `duration=`",
  raycast: "upstream binds raycast keywords without checking names (`foo=` compiles); `include`/`exclude` are accepted aliases",
  toArray: "`__enumType__` models the receiver's enum slot, not a call argument",
  format: "variadic template call; upstream models it as (string, value)",
};

loadFuncKw((funcKw) => {
  const divergent = [];
  const unmapped = [];
  const exempt = [];
  let checked = 0;
  for (const fn of manifest.functions) {
    const member = fn.kind.startsWith("member");
    const recorded = Object.hasOwn(EXEMPT, fn.id);
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

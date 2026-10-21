const fs = require("fs");
const path = require("path");
const { loadFuncKw } = require("./oracle_func_kw.cjs");

const excluded = new Set([
  "__for__", "__setGlobalVariableAtIndex__",
  "__setPlayerVariableAtIndex__", "__assignTo__", "__equals__", "__inequals__",
  "__greaterThan__", "__greaterThanOrEquals__", "__lessThan__", "__lessThanOrEquals__",
  "hudHeader", "hudSubheader", "hudSubtext",
  "__localizedString__",
]);
const manifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "../../crates/opy-rs/src/manifest/data/manifest.json"), "utf8"),
);
const catalogId = new Map([
  ...manifest.functions.filter((entry) => entry.catalogId).map((entry) => [entry.id, entry.catalogId]),
  ["concat", "appendToArray"],
  ["exclude", "removeFromArray"],
]);
const flagOf = {
  canReplace0ByFalse: "ZERO_BY_FALSE",
  canReplace1ByTrue: "ONE_BY_TRUE",
  canReplace0ByNull: "ZERO_BY_NULL",
  canReplaceNullVectorByNull: "NULL_VECTOR_BY_NULL",
};

loadFuncKw((funcKw) => {
  const rows = [];
  for (const [name, info] of Object.entries(funcKw)) {
    if (excluded.has(name)) continue;
    const bare = name.replace(/^\./, "").replace(/^__(.*)__$/, "$1");
    const canonical = catalogId.get(bare) ?? bare;
    (info.args ?? []).forEach((arg, index) => {
      const flags = Object.keys(flagOf).filter((key) => arg[key]).map((key) => flagOf[key]);
      if (flags.length) rows.push([canonical, index, flags]);
    });
  }
  rows.sort((a, b) => a[0].localeCompare(b[0]) || a[1] - b[1]);
  const out = path.join(__dirname, "upstream-literal-flags.json");
  fs.writeFileSync(out, `[\n${rows.map((row) => JSON.stringify(row)).join(",\n")}\n]\n`);
  process.exit(0);
});

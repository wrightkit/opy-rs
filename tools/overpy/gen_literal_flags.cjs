// Records tools/overpy/upstream-literal-flags.json from the pinned OverPy 9.7.10
// argument tables (`canReplace0ByFalse`,
// `canReplace1ByTrue`, `canReplace0ByNull`, `canReplaceNullVectorByNull`).
//
//   pnpm install --dir tools/overpy/oracle
//   node tools/overpy/gen_literal_flags.cjs
//
// Comparison operators, `__for__`, the indexed
// variable calls, `__assignTo__` and the HUD macros are excluded: the compiler applies OverPy's
// rules for those on the typed actions and rule conditions directly.
const fs = require("fs");
const path = require("path");
const { createRequire } = require("module");

const oracle = path.join(__dirname, "oracle");
const pkg = path.dirname(
  createRequire(path.join(oracle, "package.json")).resolve("overpy/package.json"),
);
const scratch = fs.mkdtempSync(path.join(oracle, ".flags-"));
fs.cpSync(pkg, scratch, { recursive: true });
const entry = path.join(scratch, "overpy.js");
fs.writeFileSync(
  entry,
  fs.readFileSync(entry, "utf8").replace("var funcKw;", "var funcKw; globalThis.__funcKw = () => funcKw;"),
);
require(entry);

const excluded = new Set([
  "__for__", "__setGlobalVariableAtIndex__",
  "__setPlayerVariableAtIndex__", "__assignTo__", "__equals__", "__inequals__",
  "__greaterThan__", "__greaterThanOrEquals__", "__lessThan__", "__lessThanOrEquals__",
  // Macros expand to `hudText` before this pass runs.
  "hudHeader", "hudSubheader", "hudSubtext",
  // Localized strings lower to Custom String without their own catalog entry.
  "__localizedString__",
]);
// OverPy spells some functions differently from the catalog; the manifest
// records where an OverPy name resolves to another catalog id.
const manifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "../../crates/opy-rs/src/manifest/data/manifest.json"), "utf8"),
);
const catalogId = new Map([
  ...manifest.functions.filter((entry) => entry.catalogId).map((entry) => [entry.id, entry.catalogId]),
  // Lowered specially onto the catalog's array values.
  ["concat", "appendToArray"],
  ["exclude", "removeFromArray"],
]);
const flagOf = {
  canReplace0ByFalse: "ZERO_BY_FALSE",
  canReplace1ByTrue: "ONE_BY_TRUE",
  canReplace0ByNull: "ZERO_BY_NULL",
  canReplaceNullVectorByNull: "NULL_VECTOR_BY_NULL",
};

setTimeout(() => {
  const rows = [];
  for (const [name, info] of Object.entries(globalThis.__funcKw())) {
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
  fs.rmSync(scratch, { recursive: true });
  process.exit(0);
}, 3000);

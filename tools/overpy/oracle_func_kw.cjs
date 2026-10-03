// Loads the pinned OverPy oracle's `funcKw` argument tables — the tables
// `parseArgs` uses to bind `name=` keyword arguments — and hands them to
// `callback`.
//
// `overpy.js` keeps `funcKw` in a module-local `var` and populates it after
// require returns, so the loader copies the package to a scratch directory,
// hooks the declaration to export a getter, and defers the callback until
// the module's deferred initialization has completed.
const fs = require("fs");
const path = require("path");
const { createRequire } = require("module");

const oracle = path.join(__dirname, "oracle");

function loadFuncKw(callback) {
  const pkg = path.dirname(
    createRequire(path.join(oracle, "package.json")).resolve("overpy/package.json"),
  );
  const scratch = fs.mkdtempSync(path.join(oracle, ".funckw-"));
  try {
    fs.cpSync(pkg, scratch, { recursive: true });
    const entry = path.join(scratch, "overpy.js");
    fs.writeFileSync(
      entry,
      fs.readFileSync(entry, "utf8").replace(
        "var funcKw;",
        "var funcKw; globalThis.__funcKw = () => funcKw;",
      ),
    );
    require(entry);
    setTimeout(() => {
      const funcKw = globalThis.__funcKw();
      fs.rmSync(scratch, { recursive: true });
      callback(funcKw);
    }, 3000);
  } catch (error) {
    fs.rmSync(scratch, { recursive: true });
    throw error;
  }
}

module.exports = { loadFuncKw };

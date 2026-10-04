// Loads the pinned OverPy oracle's `funcKw` argument tables — the tables
// `parseArgs` uses to bind `name=` keyword arguments — and hands them to
// `callback`.
//
// `overpy.js` keeps `funcKw` in a module-local `var` and populates it during
// deferred initialization, so the loader copies the package to a scratch
// directory, hooks the declaration to export a getter, and waits on the
// module's exported `readyPromise`.
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
    const patched = fs
      .readFileSync(entry, "utf8")
      .replace("var funcKw;", "var funcKw; globalThis.__funcKw = () => funcKw;");
    if (!patched.includes("__funcKw")) {
      throw new Error("funcKw hook anchor not found in pinned overpy.js");
    }
    fs.writeFileSync(entry, patched);
    require(entry)
      .readyPromise.then(() => {
        const funcKw = globalThis.__funcKw();
        fs.rmSync(scratch, { recursive: true });
        callback(funcKw);
      })
      .catch((error) => {
        fs.rmSync(scratch, { recursive: true, force: true });
        throw error;
      });
  } catch (error) {
    fs.rmSync(scratch, { recursive: true, force: true });
    throw error;
  }
}

module.exports = { loadFuncKw };

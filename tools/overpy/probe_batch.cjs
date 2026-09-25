// Compile probe programs with the pinned OverPy in one process.
// Usage: node probe_batch.cjs <probes.json> <references.json>
const fs = require("node:fs");
const path = require("node:path");

const overpy = require(path.join(__dirname, "oracle", "node_modules", "overpy"));

async function main() {
  const [probesPath, referencesPath] = process.argv.slice(2);
  const probes = JSON.parse(fs.readFileSync(probesPath, "utf8"));
  await overpy.readyPromise;
  const references = {};
  for (const probe of probes) {
    try {
      const compiled = await overpy.compile(probe.source, "en-US", process.cwd(), "probe.opy");
      references[probe.id] = { ok: true, workshop: compiled.result };
    } catch (error) {
      references[probe.id] = { ok: false, error: String(error).split("\n")[0] };
    }
  }
  fs.writeFileSync(referencesPath, JSON.stringify(references));
}

main().then(() => process.exit(0), (error) => {
  console.error(error);
  process.exit(2);
});

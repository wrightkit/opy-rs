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
      const text = String(error).split("\n")[0];
      if (!text.startsWith("OpyError:")) {
        throw new Error(`probe ${probe.id} failed outside source rejection: ${text}`);
      }
      references[probe.id] = { ok: false, error: text };
    }
  }
  fs.writeFileSync(referencesPath, JSON.stringify(references));
}

main().then(() => process.exit(0), (error) => {
  console.error(error);
  process.exit(2);
});

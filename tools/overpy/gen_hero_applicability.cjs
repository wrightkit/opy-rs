// Records crates/opy-rs/src/compiler/data/hero_applicability.json from the
// pinned OverPy 9.7.10 customGameSettingsSchema hero merge: the post-load
// `computeCustomGameSettingsSchema` task expands
// `heroes.values.__generalAndEachHero__` + `__eachHero__` (filtered by each
// key's include/exclude hero lists) into every hero's `values` block, and
// `heroes.values.general` gains `__generalAndEachHero__` +
// `__generalButNotEachHero__`. `compileCustomGameSettingsDict` then looks each
// authored hero key up in that merged per-hero set and writes keys it cannot
// find back verbatim, so set membership is the applicability contract.
//
//   pnpm install --dir tools/overpy/oracle
//   node tools/overpy/gen_hero_applicability.cjs
//
// The artifact records each merged key's applying hero set in the smaller of
// two spellings: `only` (the heroes the key applies to) or `except` (the
// heroes it does not). Keys applying to every hero sit in `all`. Team-level
// pseudo entries (`general`, `enabledHeroes`, `disabledHeroes`) are excluded;
// the general key set is emitted under `generalKeys` for auditing only.

const fs = require("fs");
const path = require("path");

const overpy = require(path.join(__dirname, "oracle", "node_modules", "overpy"));

const OUT = path.join(
  __dirname,
  "..",
  "..",
  "crates",
  "opy-rs",
  "src",
  "compiler",
  "data",
  "hero_applicability.json",
);

overpy.readyPromise.then(() => {
  const heroKw = overpy.heroKw;
  const values = overpy.customGameSettingsSchema.heroes.values;
  const heroes = Object.keys(heroKw);
  const union = new Set();
  for (const hero of heroes) {
    for (const key of Object.keys(values[hero]?.values ?? {})) {
      union.add(key);
    }
  }
  const all = [];
  const only = {};
  const except = {};
  for (const key of [...union].sort()) {
    const have = heroes.filter((hero) => key in (values[hero]?.values ?? {}));
    if (have.length === heroes.length) {
      all.push(key);
    } else if (have.length <= heroes.length - have.length) {
      only[key] = have;
    } else {
      except[key] = heroes.filter((hero) => !(key in (values[hero]?.values ?? {})));
    }
  }
  const artifact = {
    schemaVersion: 1,
    pinned: "overpy@9.7.10",
    derivation:
      "heroes.values merge (__generalAndEachHero__ + filtered __eachHero__ + per-hero values) computed by the pinned compiler's own post-load task",
    generalKeys: Object.keys(values.general?.values ?? {}).sort(),
    all,
    only,
    except,
  };
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, JSON.stringify(artifact, null, 1) + "\n");
  console.log(
    `wrote ${OUT}: ${all.length} universal, ${Object.keys(only).length} only, ${Object.keys(except).length} except`,
  );
});

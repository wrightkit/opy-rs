#!/usr/bin/env node
const fs = require("fs");
const path = require("path");

const bundle = path.join(
  __dirname,
  "oracle/node_modules/.pnpm/overpy@9.7.10/node_modules/overpy/overpy.js",
);
const out = path.join(
  __dirname,
  "../../crates/opy-rs/src/compiler/data/workshop_tokens.json",
);
const source = fs.readFileSync(bundle, "utf8");

const tokens = {};
for (const key of ["__on__", "__off__"]) {
  const match = source.match(new RegExp(`"${key}":\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`token ${key} not found in pinned bundle`);
  const entries = {};
  for (const locale of match[1].matchAll(/"([a-z]{2}-[A-Z]{2})":\s*"((?:\\.|[^"])*)"/g)) {
    // The bundle spells non-ASCII bytes as `\xNN`, which JSON does not read.
    entries[locale[1]] = JSON.parse(
      `"${locale[2].replace(/\\x([0-9a-fA-F]{2})/g, "\\u00$1")}"`,
    );
  }
  if (!entries["en-US"]) throw new Error(`token ${key} has no en-US entry`);
  tokens[key] = entries;
}

fs.writeFileSync(
  out,
  JSON.stringify(
    {
      schemaVersion: 1,
      pinned: "overpy@9.7.10",
      derivation:
        "customGameSettingsKw __on__/__off__ locale maps extracted from the pinned compiler bundle; settings.workshop bool members emit these tokens upstream",
      tokens,
    },
    null,
    " ",
  ) + "\n",
);
console.log(`wrote ${out}`);

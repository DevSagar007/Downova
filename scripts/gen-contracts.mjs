#!/usr/bin/env node
// Generates TypeScript types from the canonical JSON Schema and validates contract examples.
//   node scripts/gen-contracts.mjs          -> write packages/contracts/src/generated/*
//   node scripts/gen-contracts.mjs --check  -> fail if generated output is stale or examples disagree
import { readFile, writeFile, readdir, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "json-schema-to-typescript";
import Ajv2020 from "ajv/dist/2020.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const contractsDir = path.join(root, "packages", "contracts");
const schemaPath = path.join(contractsDir, "schema", "service-api.v1.schema.json");
const outPath = path.join(contractsDir, "src", "generated", "service-api.v1.ts");
const checkOnly = process.argv.includes("--check");

const BANNER = `/* eslint-disable */
// GENERATED FILE — DO NOT EDIT.
// Source: packages/contracts/schema/service-api.v1.schema.json
// Regenerate with: pnpm contracts:gen
`;

const schema = JSON.parse(await readFile(schemaPath, "utf8"));
const failures = [];

// 1. Generate TypeScript for every $defs entry.
let generated = await compile(schema, "ServiceApiV1", {
  bannerComment: BANNER,
  unreachableDefinitions: true,
  additionalProperties: false,
  strictIndexSignatures: true,
  format: true,
  cwd: path.dirname(schemaPath),
});
generated = generated.replace(/\r\n/g, "\n");

if (checkOnly) {
  const current = existsSync(outPath)
    ? (await readFile(outPath, "utf8")).replace(/\r\n/g, "\n")
    : "";
  if (current !== generated) {
    failures.push(`${path.relative(root, outPath)} is stale; run \`pnpm contracts:gen\`.`);
  }
} else {
  await mkdir(path.dirname(outPath), { recursive: true });
  await writeFile(outPath, generated, "utf8");
  console.log(`wrote ${path.relative(root, outPath)}`);
}

// 2. Validate examples: valid ones must pass, invalid ones must fail.
const ajv = new Ajv2020({ allErrors: true, strict: true, allowUnionTypes: true });
ajv.addSchema(schema);
const validatorFor = (def) => {
  if (!schema.$defs?.[def]) return null;
  return ajv.getSchema(`${schema.$id}#/$defs/${def}`);
};

async function checkExamples(dir, expectValid) {
  if (!existsSync(dir)) return 0;
  let count = 0;
  for (const name of (await readdir(dir)).filter((n) => n.endsWith(".json")).sort()) {
    const rel = path.relative(root, path.join(dir, name));
    const doc = JSON.parse(await readFile(path.join(dir, name), "utf8"));
    const validate = validatorFor(doc.$contract);
    if (!validate) {
      failures.push(`${rel}: unknown $contract "${doc.$contract}"`);
      continue;
    }
    const ok = validate(doc.value);
    if (ok !== expectValid) {
      const why = ok ? "passed but must be rejected" : ajv.errorsText(validate.errors);
      failures.push(`${rel}: ${why}`);
    }
    count += 1;
  }
  return count;
}

const validCount = await checkExamples(path.join(contractsDir, "examples"), true);
const invalidCount = await checkExamples(path.join(contractsDir, "examples", "invalid"), false);

if (failures.length > 0) {
  for (const f of failures) console.error(`contracts: FAIL ${f}`);
  process.exit(1);
}
console.log(
  `contracts: OK (${checkOnly ? "generated types up to date, " : ""}${validCount} valid and ${invalidCount} invalid examples behave as expected)`,
);

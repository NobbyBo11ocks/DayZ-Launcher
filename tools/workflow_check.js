#!/usr/bin/env node
// Parses every GitHub Actions workflow and fails if one is malformed.
//
// An invalid workflow does not fail loudly. GitHub cannot read its `name:`, so the run
// is listed under the file path, it ends in 0 s, and the only explanation is "this run
// likely failed because of a workflow file issue" — with no log to open. Two tagged
// releases went unpublished that way before anyone looked at why the updater was still
// serving an older version (D-228).
//
// Also checks for duplicated step names, which is what a botched edit looks like when
// the YAML still happens to parse.
//
// Usage: node tools/workflow_check.js
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dir = path.join(root, ".github", "workflows");

let failed = 0;
const files = readdirSync(dir).filter((f) => f.endsWith(".yml") || f.endsWith(".yaml"));
if (files.length === 0) {
  console.error("workflow_check: no workflows found in .github/workflows");
  process.exitCode = 1;
}

for (const file of files) {
  const rel = path.join(".github", "workflows", file);
  let doc;
  try {
    doc = parse(readFileSync(path.join(dir, file), "utf8"));
  } catch (e) {
    console.error(`FAIL ${rel}: ${e.message.split("\n")[0]}`);
    failed++;
    continue;
  }
  if (!doc || typeof doc !== "object") {
    console.error(`FAIL ${rel}: not a mapping`);
    failed++;
    continue;
  }
  if (!doc.name) {
    console.error(`FAIL ${rel}: no top-level \`name\`, so the run shows as a file path`);
    failed++;
    continue;
  }
  if (!doc.jobs || Object.keys(doc.jobs).length === 0) {
    console.error(`FAIL ${rel}: no jobs`);
    failed++;
    continue;
  }
  let steps = 0;
  for (const [jobName, job] of Object.entries(doc.jobs)) {
    const names = (job.steps ?? []).map((s) => s?.name).filter(Boolean);
    steps += (job.steps ?? []).length;
    const dupes = names.filter((n, i) => names.indexOf(n) !== i);
    if (dupes.length > 0) {
      console.error(`FAIL ${rel}: job "${jobName}" repeats a step name: ${[...new Set(dupes)].join(", ")}`);
      failed++;
    }
  }
  if (failed === 0) console.log(`ok   ${rel}: ${doc.name}, ${Object.keys(doc.jobs).length} job(s), ${steps} steps`);
}

if (failed > 0) {
  console.error(`\n${failed} workflow problem(s) — GitHub would refuse to run these and say almost nothing about why`);
  process.exitCode = 1;
}

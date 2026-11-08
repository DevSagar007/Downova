#!/usr/bin/env node
// Publishes a failed step's log as an annotation (readable without signing in).
//
//   node scripts/annotate-failed-log.mjs <log file> <title>
import fs from "node:fs";
import { logTailAnnotation } from "./lib/annotations.mjs";

const [file, title = "build failed"] = process.argv.slice(2);
if (!file || !fs.existsSync(file)) {
  console.log(`::error title=${title.replace(/[,:]/g, " ")}::no log at ${file ?? "(none)"}`);
  process.exit(0);
}
console.log(logTailAnnotation(title, fs.readFileSync(file, "utf8")));

#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2]?.trim();
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Usage: node scripts/bump-cli-version.mjs <semver>");
  process.exit(1);
}

const jsonFiles = [
  "packages/cli-npm/package.json",
  "packages/cli-npm/platform/darwin-arm64/package.json",
  "packages/cli-npm/platform/win32-x64/package.json",
];

function writeJson(path, data) {
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`);
}

for (const path of jsonFiles) {
  const pkg = JSON.parse(readFileSync(path, "utf8"));
  pkg.version = version;
  if (pkg.name === "@vibe-plus/cli") {
    pkg.optionalDependencies = {
      "@vibe-plus/cli-darwin-arm64": version,
      "@vibe-plus/cli-win32-x64": version,
    };
  }
  writeJson(path, pkg);
}

const cargoToml = readFileSync("Cargo.toml", "utf8");
writeFileSync(
  "Cargo.toml",
  cargoToml.replace(/(\[workspace\.package\][\s\S]*?\nversion = ")[^"]+("\n)/, `$1${version}$2`),
);

execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], { stdio: "inherit" });
execFileSync("bun", ["install", "--lockfile-only"], { stdio: "inherit" });

console.log(`Bumped CLI/Gateway release files to ${version}.`);
console.log("Next:");
console.log(
  `  git add Cargo.toml Cargo.lock bun.lock packages/cli-npm/package.json packages/cli-npm/platform/*/package.json`,
);
console.log(`  git commit -m "chore: release cli ${version}"`);
console.log(`  git tag v${version}`);
console.log(`  git push origin main v${version}`);

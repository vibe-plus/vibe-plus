#!/usr/bin/env node
// Wrapper: resolves the correct platform binary and execs it.

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";

const require = createRequire(import.meta.url);

function getPlatformPackage() {
  const { platform, arch } = process;
  const map = {
    "darwin-arm64": "@vibe-cli/darwin-arm64",
    "darwin-x64": "@vibe-cli/darwin-x64",
    "linux-x64": "@vibe-cli/linux-x64",
    "linux-arm64": "@vibe-cli/linux-arm64",
    "win32-x64": "@vibe-cli/win32-x64",
  };
  const key = `${platform}-${arch}`;
  const pkg = map[key];
  if (!pkg) {
    console.error(`vibe-cli: unsupported platform ${key}`);
    process.exit(1);
  }
  return pkg;
}

function findBinary() {
  const pkg = getPlatformPackage();
  try {
    const pkgJson = require.resolve(`${pkg}/package.json`);
    const dir = path.dirname(pkgJson);
    const ext = process.platform === "win32" ? ".exe" : "";
    return path.join(dir, "bin", `vibe${ext}`);
  } catch {
    console.error(`vibe-cli: platform package ${pkg} is not installed.`);
    console.error(`  Try: npm install -g vibe-cli`);
    process.exit(1);
  }
}

const binary = findBinary();
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
process.exit(result.status ?? 1);

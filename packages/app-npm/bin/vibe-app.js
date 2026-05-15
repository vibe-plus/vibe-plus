#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";

const require = createRequire(import.meta.url);

const packageMap = {
  "darwin-arm64": "@vibe-plus/app-darwin-arm64",
  "win32-x64": "@vibe-plus/app-win32-x64",
};

function findBinary() {
  const platform = process.env.VIBE_APP_PLATFORM || process.platform;
  const arch = process.env.VIBE_APP_ARCH || process.arch;
  const key = `${platform}-${arch}`;
  const pkg = packageMap[key];

  if (!pkg) {
    console.error(`vibe-app: unsupported platform ${key}`);
    console.error(`  Supported: ${Object.keys(packageMap).join(", ")}`);
    process.exit(1);
  }

  try {
    const pkgJson = require.resolve(`${pkg}/package.json`);
    const dir = path.dirname(pkgJson);
    const ext = platform === "win32" ? ".exe" : "";
    return path.join(dir, "bin", `vibe-app${ext}`);
  } catch {
    console.error(`vibe-app: platform package ${pkg} is not installed.`);
    console.error(`  Try: npm install -g @vibe-plus/app`);
    process.exit(1);
  }
}

const binary = findBinary();

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false,
});

child.on("error", (error) => {
  console.error(`vibe-app: failed to launch ${binary}`);
  console.error(`  ${error.message}`);
  process.exit(1);
});

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    if (!child.killed) child.kill(signal);
  });
}

const result = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    resolve(signal ? { type: "signal", signal } : { type: "code", code: code ?? 1 });
  });
});

if (result.type === "signal") {
  process.exit({ SIGHUP: 129, SIGINT: 130, SIGTERM: 143 }[result.signal] ?? 1);
}
process.exit(result.code);

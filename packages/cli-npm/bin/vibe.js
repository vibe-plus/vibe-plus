#!/usr/bin/env node
// Wrapper: resolves the correct platform binary and execs it.

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const packageMap = {
  "darwin-arm64": "@vibe-plus/cli-darwin-arm64",
  "win32-x64": "@vibe-plus/cli-win32-x64",
};

function getPlatformPackage() {
  const platform = process.env.VIBE_CLI_PLATFORM || process.platform;
  const arch = process.env.VIBE_CLI_ARCH || process.arch;
  const key = `${platform}-${arch}`;
  const pkg = process.env.VIBE_CLI_PLATFORM_PACKAGE || packageMap[key];
  if (!pkg) {
    console.error(`vibe: unsupported platform ${key}`);
    console.error(`  Supported platforms: ${Object.keys(packageMap).join(", ")}`);
    if (platform === "linux") {
      console.error(
        "  Linux builds are not published; use macOS Apple Silicon or Windows x64, or build from source.",
      );
    } else if (platform === "darwin" && arch === "x64") {
      console.error(
        "  Intel Mac builds are not published; use Apple Silicon or build from source.",
      );
    } else if (platform === "win32" && arch === "arm64") {
      console.error(
        "  Windows ARM64 builds are not published; use Windows x64 or build from source.",
      );
    }
    process.exit(1);
  }
  return pkg;
}

function getRuntimePlatform() {
  return process.env.VIBE_CLI_PLATFORM || process.platform;
}

function findBinary() {
  const pkg = getPlatformPackage();
  try {
    const pkgJson = require.resolve(`${pkg}/package.json`);
    const dir = path.dirname(pkgJson);
    const ext = getRuntimePlatform() === "win32" ? ".exe" : "";
    return path.join(dir, "bin", `vibe${ext}`);
  } catch {
    const manager = detectPackageManager();
    const installCommand =
      manager === "bun" ? "bun install -g @vibe-plus/cli" : "npm install -g @vibe-plus/cli";
    console.error(`vibe: platform package ${pkg} is not installed.`);
    console.error(`  Try: ${installCommand}`);
    console.error(`  If you used npm with --no-optional, reinstall without that flag.`);
    process.exit(1);
  }
}

function detectPackageManager() {
  const userAgent = process.env.npm_config_user_agent || "";
  if (/\bbun\//.test(userAgent)) {
    return "bun";
  }

  const execPath = process.env.npm_execpath || "";
  if (execPath.includes("bun")) {
    return "bun";
  }

  const argv0 = process.env.npm_node_execpath || process.argv0 || "";
  if (argv0.includes("bun")) {
    return "bun";
  }

  const wrapperDir = path.dirname(fileURLToPath(import.meta.url));
  const bunInstall = process.env.BUN_INSTALL || "";
  if (bunInstall && wrapperDir.startsWith(path.resolve(bunInstall))) {
    return "bun";
  }

  if (wrapperDir.includes(".bun/install/global") || wrapperDir.includes(".bun\\install\\global")) {
    return "bun";
  }

  return "npm";
}

const binary = findBinary();
const packageManager = detectPackageManager();
const env = { ...process.env };
if (packageManager === "bun") {
  env.VIBE_MANAGED_BY_BUN = "1";
} else {
  env.VIBE_MANAGED_BY_NPM = "1";
}

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
  env,
  windowsHide: false,
});

child.on("error", (error) => {
  console.error(`vibe: failed to launch ${binary}`);
  console.error(`  ${error.message}`);
  process.exit(1);
});

function forwardSignal(signal) {
  if (child.killed) {
    return;
  }
  try {
    child.kill(signal);
  } catch {
    // Ignore races where the child has already exited.
  }
}

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => forwardSignal(signal));
}

const result = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    if (signal) {
      resolve({ type: "signal", signal });
    } else {
      resolve({ type: "code", code: code ?? 1 });
    }
  });
});

if (result.type === "signal") {
  const signalExitCodes = {
    SIGHUP: 129,
    SIGINT: 130,
    SIGTERM: 143,
  };
  process.exit(signalExitCodes[result.signal] ?? 1);
}

process.exit(result.code);

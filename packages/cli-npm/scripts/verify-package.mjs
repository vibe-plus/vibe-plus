#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(root, "..", "..");
const requireBinaries = process.argv.includes("--require-binaries");
const platforms = [
  ["darwin-arm64", "@vibe-plus/cli-darwin-arm64", "darwin", "arm64", "vibe"],
  ["win32-x64", "@vibe-plus/cli-win32-x64", "win32", "x64", "vibe.exe"],
];

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd || root,
    env: { ...process.env, ...options.env },
    encoding: "utf8",
    stdio: options.stdio || "pipe",
  });
  if (result.status !== 0) {
    throw new Error(
      [
        `${command} ${args.join(" ")} failed with exit code ${result.status}`,
        result.stdout,
        result.stderr,
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }
  return result.stdout;
}

function runAllowFailure(command, args, options = {}) {
  return spawnSync(command, args, {
    cwd: options.cwd || root,
    env: { ...process.env, ...options.env },
    encoding: "utf8",
    stdio: "pipe",
  });
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function packList(cwd) {
  const output = run("npm", ["pack", "--dry-run", "--json"], { cwd });
  return JSON.parse(output)[0]
    .files.map((file) => file.path)
    .sort();
}

async function main() {
  const wrapper = await readJson(path.join(root, "package.json"));
  assert(wrapper.name === "@vibe-plus/cli", "wrapper package name changed");
  assert(wrapper.type === "module", "wrapper package must mark bin/vibe.js as ESM");
  const vibeBin = wrapper.bin?.vibe?.replace(/^\.\//, "");
  assert(vibeBin === "bin/vibe.js", "wrapper bin must expose vibe");
  assert(wrapper.files?.includes("bin/"), "wrapper package must publish bin/");
  assert(wrapper.files?.includes("README.md"), "wrapper package must publish README.md");
  assert(
    wrapper.scripts?.verify === "node ./scripts/verify-package.mjs",
    "wrapper verify script missing",
  );
  assert(wrapper.publishConfig?.access === "public", "wrapper package must publish publicly");

  for (const [, packageName] of platforms) {
    assert(
      wrapper.optionalDependencies?.[packageName] === wrapper.version,
      `missing optional dependency ${packageName}@${wrapper.version}`,
    );
  }

  const releaseWorkflow = await readFile(
    path.join(repoRoot, ".github", "workflows", "release.yml"),
    "utf8",
  );
  for (const [directory, packageName] of platforms) {
    assert(
      releaseWorkflow.includes(`npm_plat: ${directory}`),
      `release workflow missing npm_plat: ${directory}`,
    );
    assert(
      releaseWorkflow.includes(`"packages/cli-npm/platform/${directory}/package.json"`),
      `release workflow does not stamp ${directory}`,
    );
    assert(
      releaseWorkflow.includes(`"${packageName}"`),
      `release workflow missing optional dependency ${packageName}`,
    );
  }

  const wrapperFiles = packList(root);
  assert(wrapperFiles.includes("bin/vibe.js"), "wrapper tarball missing bin/vibe.js");
  assert(wrapperFiles.includes("README.md"), "wrapper tarball missing README.md");
  assert(
    !wrapperFiles.some((file) => file.startsWith("scripts/")),
    "wrapper tarball must not include scripts/",
  );
  assert(
    !wrapperFiles.some((file) => file.startsWith("platform/")),
    "wrapper tarball must not include platform/",
  );

  for (const [directory, packageName, os, cpu, binaryName] of platforms) {
    const packageDir = path.join(root, "platform", directory);
    const pkg = await readJson(path.join(packageDir, "package.json"));
    assert(pkg.name === packageName, `${directory} package name mismatch`);
    assert(pkg.version === wrapper.version, `${directory} version mismatch`);
    assert(pkg.os?.length === 1 && pkg.os[0] === os, `${directory} os mismatch`);
    assert(pkg.cpu?.length === 1 && pkg.cpu[0] === cpu, `${directory} cpu mismatch`);
    assert(pkg.files?.includes("bin/"), `${directory} package must publish bin/`);
    assert(pkg.publishConfig?.access === "public", `${directory} package must publish publicly`);

    const binaryPath = path.join(packageDir, "bin", binaryName);
    if (existsSync(binaryPath)) {
      const files = packList(packageDir);
      assert(files.includes(`bin/${binaryName}`), `${directory} tarball missing ${binaryName}`);
    } else if (requireBinaries) {
      throw new Error(`${directory} missing bin/${binaryName}`);
    }
  }

  const tmp = mkdtempSync(path.join(tmpdir(), "vibe-npm-wrapper-"));
  const testPackageDir = path.join(tmp, "platform");
  const testBinDir = path.join(testPackageDir, "bin");
  const testBinary = path.join(testBinDir, "vibe");
  try {
    run("mkdir", ["-p", testBinDir]);
    writeFileSync(
      testBinary,
      '#!/usr/bin/env sh\nif [ -n "$VIBE_MANAGED_BY_BUN" ]; then echo vibe-verify-bun; elif [ -n "$VIBE_MANAGED_BY_NPM" ]; then echo vibe-verify-npm; else echo vibe-verify-unmanaged; fi\n',
    );
    run("chmod", ["+x", testBinary]);
    writeFileSync(
      path.join(testPackageDir, "package.json"),
      JSON.stringify({ name: "@vibe-plus/cli-darwin-arm64", version: wrapper.version }, null, 2),
    );
    const npmOutput = run(process.execPath, ["./bin/vibe.js", "--version"], {
      env: {
        VIBE_CLI_PLATFORM: "darwin",
        VIBE_CLI_ARCH: "arm64",
        VIBE_CLI_PLATFORM_PACKAGE: testPackageDir,
      },
      stdio: "pipe",
    });
    assert(npmOutput.includes("vibe-verify-npm"), "wrapper did not mark npm-managed installs");

    const bunOutput = run(process.execPath, ["./bin/vibe.js", "--version"], {
      env: {
        npm_config_user_agent: "bun/1.3.0 npm/? node/?",
        VIBE_CLI_PLATFORM: "darwin",
        VIBE_CLI_ARCH: "arm64",
        VIBE_CLI_PLATFORM_PACKAGE: testPackageDir,
      },
      stdio: "pipe",
    });
    assert(bunOutput.includes("vibe-verify-bun"), "wrapper did not mark bun-managed installs");

    const customBunHomeOutput = run(process.execPath, ["./bin/vibe.js", "--version"], {
      env: {
        BUN_INSTALL: root,
        VIBE_CLI_PLATFORM: "darwin",
        VIBE_CLI_ARCH: "arm64",
        VIBE_CLI_PLATFORM_PACKAGE: testPackageDir,
      },
      stdio: "pipe",
    });
    assert(
      customBunHomeOutput.includes("vibe-verify-bun"),
      "wrapper did not detect custom BUN_INSTALL installs",
    );
  } finally {
    rmSync(tmp, { force: true, recursive: true });
  }

  const winTmp = mkdtempSync(path.join(tmpdir(), "vibe-npm-win-x64-"));
  const winPackageDir = path.join(winTmp, "platform");
  const winBinDir = path.join(winPackageDir, "bin");
  const winBinary = path.join(winBinDir, "vibe.exe");
  try {
    run("mkdir", ["-p", winBinDir]);
    writeFileSync(winBinary, "#!/usr/bin/env sh\necho vibe-verify-win-x64\n");
    run("chmod", ["+x", winBinary]);
    writeFileSync(
      path.join(winPackageDir, "package.json"),
      JSON.stringify({ name: "@vibe-plus/cli-win32-x64", version: wrapper.version }, null, 2),
    );
    const winOutput = run(process.execPath, ["./bin/vibe.js", "--version"], {
      env: {
        VIBE_CLI_PLATFORM: "win32",
        VIBE_CLI_ARCH: "x64",
        VIBE_CLI_PLATFORM_PACKAGE: winPackageDir,
      },
      stdio: "pipe",
    });
    assert(winOutput.includes("vibe-verify-win-x64"), "wrapper did not resolve win32-x64 binary");
  } finally {
    rmSync(winTmp, { force: true, recursive: true });
  }

  const unsupportedLinux = runAllowFailure(process.execPath, ["./bin/vibe.js", "--version"], {
    env: {
      VIBE_CLI_PLATFORM: "linux",
      VIBE_CLI_ARCH: "x64",
    },
  });
  assert(unsupportedLinux.status !== 0, "unsupported Linux platform should fail");
  assert(
    unsupportedLinux.stderr.includes("Linux builds are not published"),
    "unsupported Linux message should be specific",
  );

  const unsupportedIntelMac = runAllowFailure(process.execPath, ["./bin/vibe.js", "--version"], {
    env: {
      VIBE_CLI_PLATFORM: "darwin",
      VIBE_CLI_ARCH: "x64",
    },
  });
  assert(unsupportedIntelMac.status !== 0, "unsupported Intel Mac platform should fail");
  assert(
    unsupportedIntelMac.stderr.includes("Intel Mac builds are not published"),
    "unsupported Intel Mac message should be specific",
  );

  const unsupportedWindowsArm = runAllowFailure(process.execPath, ["./bin/vibe.js", "--version"], {
    env: {
      VIBE_CLI_PLATFORM: "win32",
      VIBE_CLI_ARCH: "arm64",
    },
  });
  assert(unsupportedWindowsArm.status !== 0, "unsupported Windows ARM64 platform should fail");
  assert(
    unsupportedWindowsArm.stderr.includes("Windows ARM64 builds are not published"),
    "unsupported Windows ARM64 message should be specific",
  );

  console.log("npm package verification passed");
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});

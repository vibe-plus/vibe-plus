#!/usr/bin/env node
import { intro, outro, select, spinner, note, cancel, isCancel } from "@clack/prompts";
import { spawnSync } from "node:child_process";
import { platform, arch } from "node:process";

function detectPackageManager() {
  const ua = process.env.npm_config_user_agent ?? "";
  if (/\bbun\//.test(ua) || (process.env.npm_execpath ?? "").includes("bun")) return "bun";
  return "npm";
}

function platformKey() {
  const p = process.env.VIBEPLUS_PLATFORM ?? platform;
  const a = process.env.VIBEPLUS_ARCH ?? arch;
  return `${p}-${a}`;
}

const APP_RELEASE_URL = "https://github.com/vibe-plus/vibe-plus/releases/latest";

async function installCli() {
  const pm = detectPackageManager();
  const pkg = "@vibe-plus/cli";
  const cmd = pm === "bun" ? ["bun", ["add", "-g", pkg]] : ["npm", ["install", "-g", pkg]];

  const s = spinner();
  s.start(`Installing ${pkg} via ${pm}…`);
  const result = spawnSync(cmd[0], cmd[1], { stdio: "pipe", encoding: "utf8" });
  if (result.status !== 0) {
    s.stop("Installation failed.");
    console.error(result.stderr || result.stdout);
    process.exit(1);
  }
  s.stop(`${pkg} installed.`);

  note(
    [
      "  vibe start              start the proxy on port 15917",
      "  vibe provider add       add your first API provider",
      "  vibe takeover claude    redirect Claude Code through vibe+",
      "  vibe ui                 open the dashboard in your browser",
    ].join("\n"),
    "Next steps",
  );
}

function showAppInfo() {
  const key = platformKey();
  note(
    [
      `Platform detected: ${key}`,
      "",
      "The native App is not yet released.",
      "Install the CLI for now — it includes a built-in dashboard:",
      "",
      "  npm install -g @vibe-plus/cli",
      "  vibe start && vibe ui",
      "",
      `Watch for releases: ${APP_RELEASE_URL}`,
    ].join("\n"),
    "App coming soon",
  );
}

async function main() {
  console.log("");
  intro("Welcome to vibe+  —  the unified local AI gateway");

  const choice = await select({
    message: "How would you like to install vibe+?",
    options: [
      {
        value: "cli",
        label: "CLI  (recommended)",
        hint: "lightweight binary, runs as a local proxy",
      },
      {
        value: "app",
        label: "App  (coming soon)",
        hint: "native desktop app for macOS / Windows / Linux",
      },
    ],
  });

  if (isCancel(choice)) {
    cancel("Cancelled.");
    process.exit(0);
  }

  if (choice === "cli") {
    await installCli();
  } else {
    showAppInfo();
  }

  outro("Done! Questions? → https://github.com/vibe-plus/vibe-plus");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

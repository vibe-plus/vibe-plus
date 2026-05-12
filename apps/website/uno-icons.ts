import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import type { IconifyJSON } from "@iconify/types";

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

const LOBE_ICON_NAMES = [
  "anthropic",
  "claude",
  "claude-color",
  "claudecode",
  "claudecode-color",
  "codex",
  "codex-color",
  "gemini",
  "gemini-color",
  "geminicli",
  "geminicli-color",
  "google",
  "google-color",
  "openai",
] as const;

function parseSvgIcon(raw: string) {
  const viewBox = raw.match(/\sviewBox=["']([^"']+)["']/)?.[1] ?? "0 0 24 24";
  const [, , width = "24", height = "24"] = viewBox.split(/\s+/);
  const body = raw
    .replace(/<svg\b[^>]*>/i, "")
    .replace(/<\/svg>\s*$/i, "")
    .replace(/<title\b[^>]*>[\s\S]*?<\/title>/gi, "")
    .replace(/<desc\b[^>]*>[\s\S]*?<\/desc>/gi, "")
    .trim();

  return {
    body,
    width: Number(width) || 24,
    height: Number(height) || 24,
  };
}

function svgFileCollection(prefix: string, files: Record<string, string>): IconifyJSON {
  const icons: IconifyJSON["icons"] = {};

  for (const [name, path] of Object.entries(files)) {
    icons[name] = parseSvgIcon(readFileSync(path, "utf8"));
  }

  return {
    prefix,
    icons,
  };
}

function lobeIconPath(name: string): string {
  return require.resolve(`@lobehub/icons-static-svg/icons/${name}.svg`);
}

export function lobeIconsCollection(): IconifyJSON {
  return svgFileCollection(
    "lobe",
    Object.fromEntries(LOBE_ICON_NAMES.map((name) => [name, lobeIconPath(name)])),
  );
}

export function vibePlusIconsCollection(): IconifyJSON {
  const logoPath = join(here, "src/assets/brand/vibe-plus-logo.svg");
  const fallbackPath = join(here, "src/assets/brand/vibe-plus-icon-soft-mint.svg");

  return svgFileCollection("vp", {
    logo: existsSync(logoPath) ? logoPath : fallbackPath,
  });
}

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
  // Third-party provider brands (auto-matched by smart modal / provider-logo)
  "deepseek",
  "deepseek-color",
  "qwen",
  "qwen-color",
  "moonshot",
  "kimi",
  "kimi-color",
  "groq",
  "openrouter",
  "mistral",
  "mistral-color",
  "fireworks",
  "fireworks-color",
  "grok",
  "together",
  "together-color",
  "replicate",
  "zhipu",
  "zhipu-color",
  "azure",
  "azure-color",
  "bedrock",
  "bedrock-color",
  "baichuan",
  "baichuan-color",
  "chatglm",
  "chatglm-color",
  "cloudflare",
  "cloudflare-color",
  "cohere",
  "cohere-color",
  "doubao",
  "doubao-color",
  "huggingface",
  "huggingface-color",
  "hunyuan",
  "hunyuan-color",
  "minimax",
  "minimax-color",
  "nvidia",
  "nvidia-color",
  "ollama",
  "perplexity",
  "perplexity-color",
  "spark",
  "spark-color",
  "stepfun",
  "stepfun-color",
  "volcengine",
  "volcengine-color",
  "wenxin",
  "wenxin-color",
  "xai",
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
  const logoPath = join(here, "src/dashboard/assets/brand/vibe-plus-logo.svg");
  const fallbackPath = join(here, "src/dashboard/assets/brand/vibe-plus-icon-soft-mint.svg");

  return svgFileCollection("vp", {
    logo: existsSync(logoPath) ? logoPath : fallbackPath,
  });
}

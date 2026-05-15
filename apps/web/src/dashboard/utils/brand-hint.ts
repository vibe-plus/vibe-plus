const BRAND_EXACT_KEYS = [
  "deepseek",
  "moonshot",
  "openrouter",
  "perplexity",
  "fireworks",
  "volcengine",
  "huggingface",
  "cloudflare",
  "chatglm",
  "baichuan",
  "replicate",
  "together",
  "stepfun",
  "minimax",
  "mistral",
  "bedrock",
  "cohere",
  "doubao",
  "hunyuan",
  "nvidia",
  "ollama",
  "spark",
  "wenxin",
  "zhipu",
  "gemini",
  "google",
  "claude",
  "openai",
  "anthropic",
  "azure",
  "groq",
  "grok",
  "xai",
  "qwen",
  "kimi",
] as const;

const BRAND_ICON_KEYS = new Set<string>(BRAND_EXACT_KEYS);

export function brandHintFromHost(host: string | null | undefined): string | null {
  if (!host) return null;
  const lower = host
    .trim()
    .toLowerCase()
    .replace(/^www\./, "");
  const parts = lower.split(".").filter(Boolean);
  for (const part of parts) {
    if (BRAND_ICON_KEYS.has(part)) return part;
  }
  for (const key of BRAND_EXACT_KEYS) {
    if (lower.includes(key)) return key;
  }
  return null;
}

import type { Provider, ProviderKind } from "../api/client.ts";
import { PORT } from "../api/client.ts";

/** 本地 CLI / IDE 插件连到 vibe 网关时使用的「工具」维度（与供应商 kind 不同）。 */
export type ClientToolId = "codex" | "claude-code" | "opencode";

export interface ClientToolInfo {
  id: ClientToolId;
  label: string;
  shortLabel: string;
  icon: string;
  /** 网关上的路径前缀，例如 `/codex/v1` */
  pathPrefix: string;
  /** 该工具请求会匹配到的上游供应商 kind（用于快速对照「有没有配对人」） */
  consumesKinds: readonly ProviderKind[];
  /** 一句话说明环境变量或 takeover 指向 */
  setupHint: string;
}

export const CLIENT_TOOLS: readonly ClientToolInfo[] = [
  {
    id: "codex",
    label: "Codex CLI",
    shortLabel: "Codex",
    icon: "🤖",
    pathPrefix: "/codex/v1",
    consumesKinds: ["openai-responses"],
    /** 优先路径：OAuth 走网关密钥池，CLI 只连本机 */
    setupHint: "Codex：网关前缀 `/codex/v1`（OAuth/密钥池）；CLI 指向本机端口即可。",
  },
  {
    id: "claude-code",
    label: "Claude Code",
    shortLabel: "Claude Code",
    icon: "🔮",
    /** 与 `vibe takeover claude` 一致：BASE_URL 指向 /claude（SDK 再请求 /v1/messages） */
    pathPrefix: "/claude",
    consumesKinds: ["anthropic"],
    setupHint: "Claude：`ANTHROPIC_BASE_URL` → `…/claude`（与 takeover 一致）。",
  },
  {
    id: "opencode",
    label: "OpenCode",
    shortLabel: "OpenCode",
    icon: "📦",
    pathPrefix: "/opencode/v1",
    consumesKinds: ["openai-chat", "openai-responses"],
    setupHint: "OpenCode：`baseURL` → `…/opencode/v1`。",
  },
];

/** 供无障碍文案与路由说明使用（与 `CLIENT_TOOLS` 中 codex 项一致）。 */
export function getCodexClientTool(): ClientToolInfo {
  const tool = CLIENT_TOOLS.find((x) => x.id === "codex");
  if (!tool) throw new Error("CLIENT_TOOLS 缺少 codex 项");
  return tool;
}

/** 上游 kind 可被 Codex CLI 使用的网关路径前缀（如 `/codex/v1`）路由到。 */
export function providerServesCodexCliRoute(p: Provider): boolean {
  return getCodexClientTool().consumesKinds.includes(p.kind);
}

export function defaultProxyOrigin(port: number = PORT): string {
  return `http://127.0.0.1:${port}`;
}

export function toolProxyExample(tool: ClientToolInfo, port: number = PORT): string {
  return `${defaultProxyOrigin(port)}${tool.pathPrefix}`;
}

/** 与该工具相关的供应商（按 kind 匹配）；用于统计开关状态。 */
export function providersForTool(providers: readonly Provider[], tool: ClientToolInfo): Provider[] {
  const set = new Set(tool.consumesKinds);
  return providers.filter((p) => set.has(p.kind));
}

export function toolProviderStats(providers: readonly Provider[], tool: ClientToolInfo) {
  const relevant = providersForTool(providers, tool);
  const enabled = relevant.filter((p) => p.enabled);
  return {
    relevant,
    total: relevant.length,
    enabledCount: enabled.length,
  };
}

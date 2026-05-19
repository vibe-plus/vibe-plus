import type { Provider, ProviderKind } from "../api/client.ts";
import { PORT } from "../api/client.ts";
import { providerHasKind } from "./provider-protocols.ts";

/** Tool dimension used when local CLI / IDE plugins connect to the vibe gateway; distinct from provider kind. */
export type ClientToolId = "codex" | "claude-code" | "opencode";

export type ProtocolSupportMode = "native" | "bridged" | "unsupported";

export interface ProtocolSupportInfo {
  mode: ProtocolSupportMode;
  label: string;
  detail: string;
  order: number;
}

export interface ClientToolInfo {
  id: ClientToolId;
  label: string;
  shortLabel: string;
  icon: string;
  /** Path prefix on the gateway, such as `/codex/v1`. */
  pathPrefix: string;
  /** Upstream provider kind matched by this tool, used to quickly check whether a counterpart is configured. */
  consumesKinds: readonly ProviderKind[];
  /** One-line description of environment variables or takeover target. */
  setupHint: string;
  /** Maturity; experimental entries tell the UI this entry still needs polish. */
  maturity?: "stable" | "experimental";
  maturityLabel?: string;
}

export const CLIENT_TOOLS: readonly ClientToolInfo[] = [
  {
    id: "codex",
    label: "Codex CLI",
    shortLabel: "Codex",
    icon: "i-[lucide--terminal]",
    pathPrefix: "/codex/v1",
    consumesKinds: ["openai-responses"],
    /** Preferred path: OAuth uses the gateway credential pool; CLI only connects locally. */
    setupHint: "/codex/v1",
  },
  {
    id: "claude-code",
    label: "Claude Code",
    shortLabel: "Claude",
    icon: "i-[lucide--sparkles]",
    /** Same as `vibe takeover claude`: BASE_URL points to /claude, and the SDK then requests /v1/messages. */
    pathPrefix: "/claude",
    consumesKinds: ["anthropic"],
    setupHint: "Experimental · ANTHROPIC_BASE_URL -> /claude",
    maturity: "experimental",
    maturityLabel: "Experimental",
  },
  {
    id: "opencode",
    label: "OpenCode",
    shortLabel: "OpenCode",
    icon: "i-[lucide--package]",
    pathPrefix: "/opencode/v1",
    consumesKinds: ["openai-chat", "openai-responses"],
    setupHint: "baseURL -> /opencode/v1",
  },
];

/** Used for accessibility text and route descriptions, matching the codex entry in `CLIENT_TOOLS`. */
export function getCodexClientTool(): ClientToolInfo {
  const tool = CLIENT_TOOLS.find((x) => x.id === "codex");
  if (!tool) throw new Error("CLIENT_TOOLS is missing the codex entry");
  return tool;
}

/** Upstream kinds routable through gateway path prefixes usable by Codex CLI, such as `/codex/v1`. */
export function providerServesCodexCliRoute(p: Provider): boolean {
  return providerHasKind(p, "openai-responses");
}

export function getToolProtocolSupport(
  provider: Pick<Provider, "kind">,
  tool: ClientToolInfo,
): ProtocolSupportInfo {
  if (!tool.consumesKinds.some((kind) => providerHasKind(provider, kind))) {
    return {
      mode: "unsupported",
      label: "none",
      detail: "none",
      order: 99,
    };
  }

  if (tool.id === "codex") {
    if (providerHasKind(provider, "openai-responses")) {
      return {
        mode: "native",
        label: "native",
        detail: "responses",
        order: 0,
      };
    }
    if (
      providerHasKind(provider, "openai-chat") &&
      !providerHasKind(provider, "openai-responses")
    ) {
      return {
        mode: "bridged",
        label: "bridge",
        detail: "responses->chat",
        order: 1,
      };
    }
  }

  if (tool.id === "opencode") {
    if (providerHasKind(provider, "openai-chat")) {
      return {
        mode: "native",
        label: "native",
        detail: "chat",
        order: 0,
      };
    }
    if (providerHasKind(provider, "openai-responses")) {
      return {
        mode: "native",
        label: "native",
        detail: "responses",
        order: 0,
      };
    }
  }

  if (tool.id === "claude-code" && providerHasKind(provider, "anthropic")) {
    return {
      mode: "native",
      label: "native",
      detail: "anthropic",
      order: 0,
    };
  }

  return {
    mode: "native",
    label: "native",
    detail: "native",
    order: 0,
  };
}

export function defaultProxyOrigin(port: number = PORT): string {
  return `http://127.0.0.1:${port}`;
}

export function toolProxyExample(tool: ClientToolInfo, port: number = PORT): string {
  return `${defaultProxyOrigin(port)}${tool.pathPrefix}`;
}

/** Providers related to this tool, matched by kind; used to compute toggle state. */
export function providersForTool(providers: readonly Provider[], tool: ClientToolInfo): Provider[] {
  return providers.filter((p) => tool.consumesKinds.some((kind) => providerHasKind(p, kind)));
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

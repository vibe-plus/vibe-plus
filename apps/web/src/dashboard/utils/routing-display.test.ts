import { describe, expect, test } from "vite-plus/test";
import { brandHintFromHost } from "./brand-hint.ts";
import {
  formatUnknownProviderId,
  isUnknownProviderName,
  resolveProviderLabel,
} from "./provider-display.ts";
import {
  faviconUrlForHost,
  frameworkIconFromBaseUrl,
  hostFromUrlOrHost,
} from "./provider-visual.ts";
import { protocolLabel, protocolLabelsForProvider } from "./protocol-label.ts";
import {
  logMatchesWorkspaceView,
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
} from "./workspace-view.ts";

describe("routing/display pure helpers", () => {
  test("detects brand hints from host segments and fuzzy host names", () => {
    expect(brandHintFromHost("www.api.deepseek.com")).toBe("deepseek");
    expect(brandHintFromHost("gateway-openrouter.internal")).toBe("openrouter");
    expect(brandHintFromHost("unknown.example.com")).toBeNull();
  });

  test("formats missing provider labels without leaking UUID noise", () => {
    const uuid = "550e8400-e29b-41d4-a716-446655440000";
    expect(formatUnknownProviderId(uuid)).toBe("unknown");
    expect(formatUnknownProviderId(" deleted-provider ")).toBe("deleted-provider");
    expect(isUnknownProviderName("Unknown provider 123")).toBe(true);
    expect(resolveProviderLabel("p1", "fallback", new Map([["p1", "Mapped"]]))).toBe("Mapped");
    expect(resolveProviderLabel(uuid, "unknown provider", new Map())).toBe("unknown");
  });

  test("parses visual provider hints from urls and hostnames", () => {
    expect(frameworkIconFromBaseUrl("https://relay.example.com/new-api/v1")).toBe(
      "i-[lucide--layers]",
    );
    expect(frameworkIconFromBaseUrl("https://sub2api.example.com/v1")).toBe("i-[lucide--shuffle]");
    expect(hostFromUrlOrHost(" https://www.example.com:8443/v1 ")).toBe("www.example.com");
    expect(hostFromUrlOrHost("www.example.com:8443/v1")).toBe("example.com");
    expect(faviconUrlForHost("www.example.com")).toBe(
      "https://www.google.com/s2/favicons?domain=example.com&sz=64",
    );
  });

  test("maps protocol slugs and de-duplicates provider protocol labels", () => {
    expect(protocolLabel("anthropic")).toBe("Messages");
    expect(protocolLabel("openai-compat")).toBe("Chat");
    expect(protocolLabel("unknown-kind")).toBe("unknown-kind");
    expect(
      protocolLabelsForProvider({
        kind: "openai-chat",
        protocols: [{ kind: "openai-chat" }, { kind: "openai-responses" }, { kind: "openai-chat" }],
      } as any),
    ).toEqual(["Chat", "Responses"]);
  });

  test("matches providers and logs to workspace views by provider, route, app, and wire fallback", () => {
    const codexProvider = { id: "p-codex", kind: "openai-responses" } as any;
    const claudeProvider = { id: "p-claude", kind: "anthropic" } as any;
    const providerById = new Map([
      ["p-codex", codexProvider],
      ["p-claude", claudeProvider],
    ]);

    expect(workspaceViewFromQuery(["claude"])).toBe("claude");
    expect(workspaceViewFromQuery("bad")).toBe("overview");
    expect(providerMatchesWorkspaceView(codexProvider, "codex")).toBe(true);
    expect(providerMatchesWorkspaceView(claudeProvider, "codex")).toBe(false);
    expect(
      logMatchesWorkspaceView({ provider_id: "p-claude" } as any, "claude", providerById),
    ).toBe(true);
    expect(
      logMatchesWorkspaceView({ route_prefix: "/codex/v1" } as any, "codex", providerById),
    ).toBe(true);
    expect(logMatchesWorkspaceView({ wire: "anthropic" } as any, "claude", providerById)).toBe(
      true,
    );
    expect(logMatchesWorkspaceView({ app: "Codex CLI" } as any, "codex", providerById)).toBe(true);
  });
});

import type { Provider } from "../api/client.ts";
import { providerHasKind } from "./provider-protocols.ts";

export type WorkspaceView = "overview" | "codex" | "claude";

export const WORKSPACE_VIEWS: { id: WorkspaceView; label: string; icon: string }[] = [
  { id: "overview", label: "All", icon: "layout-dashboard" },
  { id: "codex", label: "Codex", icon: "codex" },
  { id: "claude", label: "Claude", icon: "claude" },
];

export function workspaceViewFromQuery(value: unknown): WorkspaceView {
  const v = Array.isArray(value) ? value[0] : value;
  if (v === "codex" || v === "claude") return v;
  return "overview";
}

export function providerMatchesWorkspaceView(provider: Provider, view: WorkspaceView): boolean {
  if (view === "overview") return true;
  if (view === "claude") return providerHasKind(provider, "anthropic");
  return providerHasKind(provider, "openai-responses");
}

export function routePrefixMatchesWorkspaceView(
  routePrefix: string | null | undefined,
  view: WorkspaceView,
): boolean {
  if (view === "overview") return true;
  const prefix = (routePrefix ?? "").toLowerCase();
  if (view === "claude") return prefix.includes("claude");
  return prefix.includes("codex") || prefix.includes("opencode");
}

export function appNameMatchesWorkspaceView(
  app: string | null | undefined,
  view: WorkspaceView,
): boolean {
  if (view === "overview") return true;
  const value = (app ?? "").toLowerCase();
  if (view === "claude") return value.includes("claude");
  return value.includes("codex") || value.includes("opencode");
}

export function providerKindMatchesWorkspaceView(
  kind: string | null | undefined,
  view: WorkspaceView,
): boolean {
  if (view === "overview") return true;
  if (view === "claude") return kind === "anthropic";
  return kind === "openai-responses" || kind === "openai-chat";
}

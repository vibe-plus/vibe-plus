import type { Provider, RequestLog } from "../api/client.ts";

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
  if (view === "claude") return provider.kind === "anthropic";
  return provider.kind === "openai-responses" || provider.kind === "openai-chat";
}

export function logMatchesWorkspaceView(
  log: RequestLog,
  view: WorkspaceView,
  providerById: ReadonlyMap<string, Provider>,
): boolean {
  if (view === "overview") return true;
  const provider = log.provider_id ? providerById.get(log.provider_id) : null;
  if (provider) return providerMatchesWorkspaceView(provider, view);
  const route = (log.route_prefix ?? "").toLowerCase();
  const wire = (log.wire ?? "").toLowerCase();
  const app = (log.app ?? "").toLowerCase();
  if (view === "codex") {
    return route.includes("codex") || app.includes("codex") || wire.includes("responses");
  }
  return route.includes("claude") || app.includes("claude") || wire.includes("anthropic");
}

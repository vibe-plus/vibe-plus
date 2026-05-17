import type { Provider } from "../api/client.ts";

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

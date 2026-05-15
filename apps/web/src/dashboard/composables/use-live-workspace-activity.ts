import { computed, ref } from "vue";
import { api, type Provider } from "../api/client.ts";
import { providerMatchesWorkspaceView, type WorkspaceView } from "../utils/workspace-view.ts";
import { useWs } from "./useProxy.ts";

type ActivityCounts = Record<WorkspaceView, number>;

const zeroCounts = (): ActivityCounts => ({
  overview: 0,
  codex: 0,
  claude: 0,
});

/**
 * Derive active request counts for each sidebar view from in-flight WS requests and provider lists.
 */
export function useLiveWorkspaceActivity() {
  const providersById = ref<Map<string, Provider>>(new Map());
  /** request_id -> provider_id */
  const activeRequestToProvider = ref<Record<string, string>>({});

  async function refreshProviders(): Promise<void> {
    try {
      const rows = await api.providers.list();
      const next = new Map<string, Provider>();
      for (const p of rows) next.set(p.id, p);
      providersById.value = next;
    } catch {
      /* Keep the existing mapping to avoid flicker */
    }
  }

  void refreshProviders();

  useWs((ev: unknown) => {
    const e = ev as {
      type?: string;
      id?: string;
      request_id?: string;
      provider_id?: string | null;
      providers?: Provider[];
      rolling_hours?: number;
    };

    if (e.type === "providers-overview-changed" && Array.isArray(e.providers)) {
      const next = new Map(providersById.value);
      for (const p of e.providers as Provider[]) next.set(p.id, p);
      providersById.value = next;
      return;
    }

    if (e.type === "request-started" && e.id && e.provider_id) {
      activeRequestToProvider.value = {
        ...activeRequestToProvider.value,
        [e.id]: e.provider_id,
      };
      return;
    }

    if (e.type === "request-updated" && e.request_id && e.provider_id) {
      activeRequestToProvider.value = {
        ...activeRequestToProvider.value,
        [e.request_id]: e.provider_id,
      };
      return;
    }

    if (e.type === "log-appended" && e.id) {
      const { [e.id]: _, ...rest } = activeRequestToProvider.value;
      activeRequestToProvider.value = rest;
    }
  });

  const activeCounts = computed<ActivityCounts>(() => {
    const out = zeroCounts();
    const pmap = providersById.value;
    for (const providerId of Object.values(activeRequestToProvider.value)) {
      const p = pmap.get(providerId);
      out.overview += 1;
      if (p && providerMatchesWorkspaceView(p, "codex")) out.codex += 1;
      if (p && providerMatchesWorkspaceView(p, "claude")) out.claude += 1;
    }
    return out;
  });

  return { activeCounts };
}

import { ref, type Ref } from "vue";
import {
  api,
  type CredentialPlanSnapshot,
  type Provider,
  type ProviderCodexPlanItem,
} from "../../../api/client.ts";

const CODEX_PLAN_AUTO_REFRESH_COOLDOWN_MS = 15 * 60 * 1000;
const CODEX_PLAN_STALE_AFTER_MS = 30 * 60 * 1000;

export function isOfficialCodexProvider(p: Provider): boolean {
  if (p.kind !== "openai-responses") return false;
  const u = p.base_url.toLowerCase();
  return u.includes("chatgpt.com") && u.includes("backend-api") && u.includes("codex");
}

export function useProviderCodexPlans(
  providers: Ref<Provider[]>,
  deps: {
    loadCreds: (providerId: string) => Promise<void>;
    refreshSinglePool: (providerId: string) => Promise<void>;
  },
) {
  const planSnapByCred = ref<Record<string, CredentialPlanSnapshot | null>>({});
  const codexPlanRowsByProvider = ref<Record<string, ProviderCodexPlanItem[]>>({});
  const codexRefreshNote = ref<Record<string, string>>({});
  const codexPlanRefreshing = ref<Record<string, boolean>>({});
  const codexPlanAutoRefreshAttemptAt = ref<Record<string, number>>({});

  async function loadCodexPlanRowsForProvider(providerId: string) {
    const p = providers.value.find((x) => x.id === providerId);
    if (!p || !isOfficialCodexProvider(p)) return;
    try {
      codexPlanRowsByProvider.value = {
        ...codexPlanRowsByProvider.value,
        [providerId]: await api.providers.codexPlan(providerId),
      };
    } catch {
      codexPlanRowsByProvider.value = { ...codexPlanRowsByProvider.value, [providerId]: [] };
    }
  }

  function isCodexPlanSnapshotStale(snap: CredentialPlanSnapshot | null | undefined): boolean {
    if (!snap?.captured_at) return true;
    return Date.now() - snap.captured_at * 1000 > CODEX_PLAN_STALE_AFTER_MS;
  }

  function shouldAutoRefreshCodexPlan(providerId: string): boolean {
    if (codexPlanRefreshing.value[providerId]) return false;
    const lastAttemptAt = codexPlanAutoRefreshAttemptAt.value[providerId] ?? 0;
    if (Date.now() - lastAttemptAt < CODEX_PLAN_AUTO_REFRESH_COOLDOWN_MS) return false;
    const rows = codexPlanRowsByProvider.value[providerId] ?? [];
    return rows.some((row) => isCodexPlanSnapshotStale(row.plan));
  }

  async function refreshCodexPlanFromChatgpt(
    providerId: string,
    opts?: { silent?: boolean; reloadCreds?: boolean },
  ) {
    if (codexPlanRefreshing.value[providerId]) return;
    if (!opts?.silent) {
      codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: "" };
    }
    if (opts?.silent) {
      codexPlanAutoRefreshAttemptAt.value = {
        ...codexPlanAutoRefreshAttemptAt.value,
        [providerId]: Date.now(),
      };
    }
    codexPlanRefreshing.value = { ...codexPlanRefreshing.value, [providerId]: true };
    try {
      const r = await api.providers.refreshCodexPlan(providerId);
      const errPart = r.errors.length ? r.errors.join("; ") : "";
      if (!opts?.silent) {
        if (r.attempted === 0) {
          codexRefreshNote.value = {
            ...codexRefreshNote.value,
            [providerId]: "oauth.credentials:empty",
          };
        } else {
          codexRefreshNote.value = {
            ...codexRefreshNote.value,
            [providerId]: errPart
              ? `updated ${r.ok}/${r.attempted} · ${errPart}`
              : `updated ${r.ok}/${r.attempted}`,
          };
        }
      } else if (errPart || (r.attempted > 0 && r.ok === 0)) {
        codexRefreshNote.value = {
          ...codexRefreshNote.value,
          [providerId]: errPart || `plan:sync_failed ${r.ok}/${r.attempted}`,
        };
      } else {
        codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: "" };
      }
      await loadCodexPlanRowsForProvider(providerId);
      if (opts?.reloadCreds ?? true) await deps.loadCreds(providerId);
      await deps.refreshSinglePool(providerId);
    } catch (e) {
      codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: String(e) };
    } finally {
      codexPlanRefreshing.value = { ...codexPlanRefreshing.value, [providerId]: false };
    }
  }

  async function runCodexWhamBackgroundRefresh() {
    const targets = providers.value.filter(
      (p) => isOfficialCodexProvider(p) && shouldAutoRefreshCodexPlan(p.id),
    );
    for (const p of targets) {
      await refreshCodexPlanFromChatgpt(p.id, { silent: true, reloadCreds: false });
      await new Promise((res) => setTimeout(res, 400));
    }
  }

  function resetCodexPlans() {
    codexPlanRowsByProvider.value = {};
    planSnapByCred.value = {};
  }

  function applyCodexPlanRows(providerId: string, rows: ProviderCodexPlanItem[]) {
    codexPlanRowsByProvider.value = {
      ...codexPlanRowsByProvider.value,
      [providerId]: rows,
    };
    const nextSnaps = { ...planSnapByCred.value };
    for (const row of rows) nextSnaps[row.credential_id] = row.plan;
    planSnapByCred.value = nextSnaps;
  }

  return {
    planSnapByCred,
    codexPlanRowsByProvider,
    codexRefreshNote,
    codexPlanRefreshing,
    loadCodexPlanRowsForProvider,
    refreshCodexPlanFromChatgpt,
    runCodexWhamBackgroundRefresh,
    resetCodexPlans,
    applyCodexPlanRows,
  };
}

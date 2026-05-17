<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from "vue";
import {
  api,
  type Provider,
  type ProviderInput,
  type ProviderHealthSummary,
  type ProviderAuthPoolSummary,
  type CredentialPoolStatus,
  type Credential,
  type CredentialInput,
  type ProvidersOverview,
  type RequestRuntimeStats,
  type UpstreamGroupInfo,
  isProviderHealthSummary,
} from "../../api/client.ts";
import {
  CLIENT_TOOLS,
  getCodexClientTool,
  type ClientToolId,
  type ClientToolInfo,
} from "../../utils/client-tools.ts";
import type { LiveRequestMetric, ProviderSectionView, ProviderTabOption } from "./types.ts";
import { useRoute, useRouter } from "vue-router";
import { resolvePageAccent } from "../../utils/page-accent.ts";
import { displayProviderName } from "../../utils/providers-display.ts";
import { hintsFromAuthJsonTokens } from "../../utils/codex-oauth-hints.ts";
import VpIcon from "../../components/vp-icon.vue";
import UiButton from "../../components/ui/button.vue";
import UiCard from "../../components/ui/card.vue";
import UiSkeleton from "../../components/ui/skeleton.vue";
import ProviderSections from "./components/ProviderSections.vue";
import ProviderSmartModal from "./components/provider-smart-modal.vue";
import ProviderImportModal from "./components/provider-import-modal.vue";
import CredentialFormModal from "./components/CredentialFormModal.vue";
import { requestWsSnapshot, useWs } from "../../composables/useProxy.ts";
import { workspaceViewFromQuery, type WorkspaceView } from "../../utils/workspace-view.ts";
import { buildProviderSections } from "./utils/provider-sections.ts";
import {
  isOfficialCodexProvider,
  useProviderCodexPlans,
} from "./composables/useProviderCodexPlans.ts";

const providers = ref<Provider[]>([]);
const healthMap = ref<Record<string, ProviderHealthSummary>>({});
/** `GET /_vp/pools` — credential-level circuit/rate-limit summary, loaded in parallel with the list. */
const poolByProviderId = ref<Record<string, ProviderAuthPoolSummary>>({});
const route = useRoute();
const router = useRouter();
const pageAccent = computed(() => resolvePageAccent(route.name));
const workspaceView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const codexRouteTool = computed(() => getCodexClientTool());
/** Hours for `GET /_vp/providers/:id/health?hours=` — gateway `request_logs` rollup only (not Codex plan windows). */
const GATEWAY_ROLLING_STAT_HOURS = 24;
const loading = ref(true);
const error = ref("");

const activeRequestProviderIds = ref<Record<string, string>>({});
const activeAttemptCredentials = ref<Record<string, { providerId: string; credentialId: string }>>(
  {},
);
const liveRequestMetrics = ref<Record<string, LiveRequestMetric>>({});
const highlightedProviderId = ref<string | null>(null);
/** Inline enable/disable debounce state (PUT /_vp/providers/:id). */
const toggleBusy = ref<Record<string, boolean>>({});
/** Per-provider manual circuit reset busy state (POST /_vp/providers/:id/circuit/reset). */
const circuitResetBusy = ref<Record<string, boolean>>({});
/** Per-provider protocol probe busy state (POST /_vp/providers/:id/probe). */
const speedtestBusy = ref<Record<string, boolean>>({});
/** Per-provider vendor auto-detect busy state. */
const detectVendorBusy = ref<Record<string, boolean>>({});
/** Per-provider remote model refresh busy state. */
const modelRefreshBusy = ref<Record<string, boolean>>({});
const credModelRefreshBusy = ref<Record<string, boolean>>({});
const credBalanceRefreshBusy = ref<Record<string, boolean>>({});
/** Per-credential enable/disable busy state (PUT /_vp/credentials/:id). */
const credToggleBusy = ref<Record<string, boolean>>({});
const activeProviderTab = ref<"common" | ClientToolId>("common");
let providersOverviewStreamRequestId: string | null = null;
let providersOverviewFallbackTimer: ReturnType<typeof setTimeout> | null = null;

// Provider form
const showForm = ref(false);
const editTarget = ref<Provider | null>(null);

// Import modal
const showImportModal = ref(false);

const editProviderLive = computed(() => {
  if (!editTarget.value) return null;
  return providers.value.find((x) => x.id === editTarget.value?.id) ?? editTarget.value;
});

const editProviderModelCount = computed(() => editProviderLive.value?.remote_models?.length ?? 0);
const editProviderSpeedLabel = computed(() => {
  const result = editProviderLive.value?.last_speedtest;
  if (!result) return "Not tested";
  if (result.error) return result.error;
  return result.latency_ms == null ? "Tested" : `${result.latency_ms}ms`;
});

// Credential management (list loads by default; see load())
const credsByProvider = ref<Record<string, Credential[]>>({});
const loadingCreds = ref<Record<string, boolean>>({});
const showCredForm = ref(false);
const editCred = ref<Credential | null>(null);
// Upstream login UI
const credLoginPassword = ref("");
const credLoginBusy = ref(false);
const credLoginNote = ref<string | null>(null);
const credGroups = ref<UpstreamGroupInfo[]>([]);
const credGroupsBusy = ref(false);
const credProviderId = ref("");
const emptyCredForm = (): CredentialInput => ({
  label: "",
  auth_ref: null,
  plan_type: null,
  notes: null,
  enabled: true,
  priority: 100,
  oauth_access_token: null,
  oauth_refresh_token: null,
  oauth_expires_at: null,
  oauth_cached_email: null,
  oauth_cached_subject: null,
  oauth_cached_plan_slug: null,
  upstream_vendor: null,
  upstream_username: null,
  upstream_group: null,
  price_multiplier: 1.0,
});
const credForm = ref<CredentialInput>(emptyCredForm());
// Credential form auth mode: "apikey" or "oauth"
const credAuthMode = ref<"apikey" | "oauth">("apikey");
// auth.json paste / file (client-side parse; mirrors vibe-core `parse_codex_auth_json`)
const authJsonPaste = ref("");
const authJsonPasteErr = ref("");
const authJsonDragActive = ref(false);
const authJsonFileInputRef = ref<HTMLInputElement | null>(null);

const credProviderKind = computed(() => {
  const p = providers.value.find((x) => x.id === credProviderId.value);
  return p?.kind ?? null;
});

function resetAuthJsonImportUi() {
  authJsonPaste.value = "";
  authJsonPasteErr.value = "";
  authJsonDragActive.value = false;
}

type OauthTriple = { access: string; refresh: string | null; exp: number | null };

function extractOauthTriple(v: Record<string, unknown>): OauthTriple | null {
  const tokens = v.tokens;
  if (!tokens || typeof tokens !== "object") return null;
  const t = tokens as Record<string, unknown>;
  const access = t.access_token;
  if (typeof access !== "string" || !access.trim()) return null;
  const rr = t.refresh_token;
  const refresh = typeof rr === "string" && rr.trim() ? rr : null;
  let exp: number | null = null;
  if (typeof t.expires_at === "number") exp = t.expires_at;
  else if (typeof t.expiry === "number") exp = t.expiry;
  return { access: access.trim(), refresh, exp };
}

function fillOauthFromTriple(triple: OauthTriple, rawDoc?: Record<string, unknown>) {
  credForm.value.oauth_access_token = triple.access;
  credForm.value.oauth_refresh_token = triple.refresh;
  credForm.value.oauth_expires_at = triple.exp ?? jwtExp(triple.access);
  credForm.value.auth_ref = null;
  credAuthMode.value = "oauth";
  if (rawDoc) {
    const h = hintsFromAuthJsonTokens(rawDoc.tokens);
    credForm.value.oauth_cached_email = h.oauth_cached_email;
    credForm.value.oauth_cached_subject = h.oauth_cached_subject;
    credForm.value.oauth_cached_plan_slug = h.oauth_cached_plan_slug;
  } else {
    credForm.value.oauth_cached_email = null;
    credForm.value.oauth_cached_subject = null;
    credForm.value.oauth_cached_plan_slug = null;
  }
}

function applyAuthJsonText(raw: string, clearPaste: boolean) {
  authJsonPasteErr.value = "";
  try {
    const v = JSON.parse(raw) as Record<string, unknown>;
    const mode = typeof v.auth_mode === "string" ? v.auth_mode : "";
    const openaiKey = v.OPENAI_API_KEY;
    const useLiteralKey =
      typeof openaiKey === "string" && openaiKey.trim() && openaiKey !== "PROXY_MANAGED";

    if (mode === "chatgpt") {
      const triple = extractOauthTriple(v);
      if (!triple) {
        authJsonPasteErr.value = "ChatGPT OAuth requires tokens.access_token in JSON.";
        return;
      }
      fillOauthFromTriple(triple, v);
      if (clearPaste) authJsonPaste.value = "";
      return;
    }

    if (mode === "apikey" || mode === "") {
      if (useLiteralKey) {
        credForm.value.auth_ref = `literal:${String(openaiKey).trim()}`;
        credForm.value.oauth_access_token = null;
        credForm.value.oauth_refresh_token = null;
        credForm.value.oauth_expires_at = null;
        credForm.value.oauth_cached_email = null;
        credForm.value.oauth_cached_subject = null;
        credForm.value.oauth_cached_plan_slug = null;
        credAuthMode.value = "apikey";
        if (clearPaste) authJsonPaste.value = "";
        return;
      }
      const triple = extractOauthTriple(v);
      if (triple) {
        fillOauthFromTriple(triple, v);
        if (clearPaste) authJsonPaste.value = "";
        return;
      }
      authJsonPasteErr.value =
        'Unrecognized JSON: need OPENAI_API_KEY, or tokens.access_token, or auth_mode "chatgpt".';
      return;
    }

    authJsonPasteErr.value = `Unknown auth_mode "${mode}".`;
  } catch (e: unknown) {
    authJsonPasteErr.value = e instanceof Error ? e.message : String(e);
  }
}

function parseAuthJsonPaste() {
  applyAuthJsonText(authJsonPaste.value, true);
}

function triggerAuthJsonFilePick() {
  authJsonFileInputRef.value?.click();
}

function onAuthJsonFileChange(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  readAuthJsonFile(file);
}

function readAuthJsonFile(file: File) {
  authJsonPasteErr.value = "";
  const reader = new FileReader();
  reader.onload = () => {
    const text = typeof reader.result === "string" ? reader.result : "";
    authJsonPaste.value = text;
    applyAuthJsonText(text, false);
  };
  reader.onerror = () => {
    authJsonPasteErr.value = "Could not read file.";
  };
  reader.readAsText(file, "UTF-8");
}

function onAuthJsonDragOver(ev: DragEvent) {
  ev.preventDefault();
  authJsonDragActive.value = true;
}

function onAuthJsonDragLeave(ev: DragEvent) {
  ev.preventDefault();
  const el = ev.currentTarget as HTMLElement | null;
  if (el && ev.relatedTarget instanceof Node && el.contains(ev.relatedTarget)) return;
  authJsonDragActive.value = false;
}

function onAuthJsonDrop(ev: DragEvent) {
  ev.preventDefault();
  authJsonDragActive.value = false;
  const file = ev.dataTransfer?.files?.[0];
  if (!file) {
    authJsonPasteErr.value = "Drop a single .json file.";
    return;
  }
  readAuthJsonFile(file);
}

/** Extract exp (Unix seconds) from a JWT access token without a library. */
function jwtExp(token: string | null | undefined): number | null {
  if (!token) return null;
  const parts = token.split(".");
  if (parts.length < 2) return null;
  try {
    const payload = atob(parts[1].replace(/-/g, "+").replace(/_/g, "/"));
    const obj = JSON.parse(payload) as Record<string, unknown>;
    const exp = obj["exp"];
    return typeof exp === "number" ? exp : null;
  } catch {
    return null;
  }
}

/** Try merging local Codex / Claude: add credentials or refresh auth_ref for existing upstreams; idempotent. */
async function mergeLocalToolsFromDisk() {
  try {
    await api.providers.importLocal(["codex", "claude"]);
  } catch {
    /* Gateway is not running or ~/.codex / ~/.claude is absent */
  }
}

function applyProvidersOverview(overview: ProvidersOverview): boolean {
  providers.value = overview.providers;

  const map: Record<string, ProviderHealthSummary> = {};
  for (const body of overview.health) {
    if (!isProviderHealthSummary(body)) {
      error.value = "gateway_api:mismatch health.cumulative missing; restart rebuilt vibe binary";
      healthMap.value = {};
      poolByProviderId.value = {};
      return false;
    }
    map[body.cumulative.provider_id] = body;
  }
  healthMap.value = map;

  const poolMap: Record<string, ProviderAuthPoolSummary> = {};
  for (const pool of overview.pools) {
    poolMap[pool.provider_id] = pool;
  }
  poolByProviderId.value = poolMap;

  const nextRows: Record<string, ProviderCodexPlanItem[]> = {};
  const nextSnaps: Record<string, CredentialPlanSnapshot | null> = {};
  for (const p of providers.value) {
    const rows = isOfficialCodexProvider(p) ? (overview.codex_plans[p.id] ?? []) : [];
    nextRows[p.id] = rows;
    for (const row of rows) {
      nextSnaps[row.credential_id] = row.plan;
    }
  }
  codexPlanRowsByProvider.value = nextRows;
  planSnapByCred.value = nextSnaps;

  const nextCreds: Record<string, Credential[]> = {};
  const nextLoading: Record<string, boolean> = {};
  for (const p of providers.value) {
    nextCreds[p.id] = overview.credentials[p.id] ?? [];
    nextLoading[p.id] = false;
  }
  credsByProvider.value = nextCreds;
  loadingCreds.value = nextLoading;
  return true;
}

async function load() {
  loading.value = true;
  try {
    error.value = "";
    if (providersOverviewFallbackTimer) clearTimeout(providersOverviewFallbackTimer);
    // Request snapshot first to avoid serial latency with local import; gateway emits stream-started only after build_providers_overview finishes.
    const requestId = requestWsSnapshot("providers-overview", {
      hours: GATEWAY_ROLLING_STAT_HOURS,
    });
    providersOverviewStreamRequestId = requestId;
    providersOverviewFallbackTimer = window.setTimeout(() => {
      if (providersOverviewStreamRequestId !== requestId || !loading.value) return;
      void loadProvidersOverviewViaHttpFallback(requestId);
    }, 3000);
    void mergeLocalToolsFromDisk();
  } catch (e) {
    error.value = String(e);
  }
}

async function loadProvidersOverviewViaHttpFallback(requestId: string) {
  try {
    const overview = await api.providers.overview(GATEWAY_ROLLING_STAT_HOURS);
    if (providersOverviewStreamRequestId !== requestId) return;
    error.value = "";
    if (!applyProvidersOverview(overview)) return;
    void runCodexWhamBackgroundRefresh();
  } catch (e) {
    if (providersOverviewStreamRequestId === requestId) error.value = String(e);
  } finally {
    if (providersOverviewStreamRequestId === requestId) loading.value = false;
  }
}

async function loadCreds(providerId: string) {
  loadingCreds.value[providerId] = true;
  try {
    credsByProvider.value[providerId] = await api.credentials.list(providerId);
  } catch {
    credsByProvider.value[providerId] = [];
  } finally {
    loadingCreds.value[providerId] = false;
  }
}

async function refreshSinglePool(providerId: string) {
  try {
    const pool = await api.providers.pool(providerId, GATEWAY_ROLLING_STAT_HOURS);
    poolByProviderId.value = { ...poolByProviderId.value, [providerId]: pool };
  } catch {
    /* Preserve previous pool snapshot */
  }
}

const {
  planSnapByCred,
  codexPlanRowsByProvider,
  codexRefreshNote,
  codexPlanRefreshing,
  refreshCodexPlanFromChatgpt,
  runCodexWhamBackgroundRefresh,
  resetCodexPlans,
  applyCodexPlanRows,
} = useProviderCodexPlans(providers, { loadCreds, refreshSinglePool });

function timestampExpired(ts: number | null | undefined): boolean {
  return typeof ts === "number" && ts > 0 && ts <= Math.floor(Date.now() / 1000);
}

function poolNeedsStaleStatusRefresh(pool: ProviderAuthPoolSummary): boolean {
  if (pool.provider_circuit_open && (pool.provider_circuit_open_remaining_secs ?? 1) <= 0) {
    return true;
  }
  return pool.credentials.some((row) => {
    if (row.circuit_open && (row.circuit_open_remaining_secs ?? 1) <= 0) return true;
    if (row.is_rate_limited) {
      return timestampExpired(row.rl_requests_reset_at) || timestampExpired(row.rl_tokens_reset_at);
    }
    return false;
  });
}

async function refreshStalePoolStatuses() {
  const targets = Object.values(poolByProviderId.value)
    .filter(poolNeedsStaleStatusRefresh)
    .map((pool) => pool.provider_id);
  if (!targets.length) return;
  await Promise.all(targets.map((providerId) => refreshSinglePool(providerId)));
}

async function autoProbeStalePools() {
  const providerIds = Object.values(poolByProviderId.value)
    .filter(
      (pool) =>
        pool.provider_circuit_open ||
        pool.credentials.some((row) => row.circuit_open || row.is_rate_limited),
    )
    .map((pool) => pool.provider_id);
  if (!providerIds.length) return;
  await Promise.all(
    providerIds.map(async (providerId) => {
      const pool = poolByProviderId.value[providerId];
      if (!pool) return;
      if (pool.provider_circuit_open && (pool.provider_circuit_open_remaining_secs ?? 1) <= 0) {
        await refreshSinglePool(providerId);
      }
      const needsSpeedtest = providers.value.some(
        (p) => p.id === providerId && p.enabled && !speedtestBusy.value[providerId],
      );
      if (needsSpeedtest && !pool.provider_circuit_open) {
        await speedtestProvider(providerId);
      }
    }),
  );
}

async function reloadProviderCreds(providerId: string) {
  await Promise.all([loadCreds(providerId), refreshSinglePool(providerId)]);
}

function replaceProviderInList(updated: Provider) {
  const ix = providers.value.findIndex((x) => x.id === updated.id);
  if (ix < 0) {
    providers.value = [...providers.value, updated];
    return;
  }
  providers.value[ix] = updated;
  providers.value = [...providers.value];
}

async function refreshProviderModels(providerId: string) {
  if (modelRefreshBusy.value[providerId]) return;
  modelRefreshBusy.value = { ...modelRefreshBusy.value, [providerId]: true };
  try {
    const updated = await api.providers.refreshModels(providerId);
    replaceProviderInList(updated);
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [providerId]: _, ...rest } = modelRefreshBusy.value;
    modelRefreshBusy.value = rest;
  }
}

async function refreshCredentialModels(credentialId: string) {
  if (credModelRefreshBusy.value[credentialId]) return;
  credModelRefreshBusy.value = { ...credModelRefreshBusy.value, [credentialId]: true };
  try {
    const updated = await api.credentials.refreshModels(credentialId);
    const pid = updated.provider_id;
    const list = credsByProvider.value[pid];
    if (list) {
      const ix = list.findIndex((c) => c.id === credentialId);
      if (ix >= 0) {
        list[ix] = updated;
        credsByProvider.value = { ...credsByProvider.value, [pid]: [...list] };
      }
    }
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [credentialId]: _, ...rest } = credModelRefreshBusy.value;
    credModelRefreshBusy.value = rest;
  }
}

async function refreshCredentialBalance(credentialId: string) {
  if (credBalanceRefreshBusy.value[credentialId]) return;
  credBalanceRefreshBusy.value = { ...credBalanceRefreshBusy.value, [credentialId]: true };
  try {
    const updated = await api.credentials.refreshBalance(credentialId);
    const pid = updated.provider_id;
    const list = credsByProvider.value[pid];
    if (list) {
      const ix = list.findIndex((c) => c.id === credentialId);
      if (ix >= 0) {
        list[ix] = updated;
        credsByProvider.value = { ...credsByProvider.value, [pid]: [...list] };
      }
    }
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [credentialId]: _, ...rest } = credBalanceRefreshBusy.value;
    credBalanceRefreshBusy.value = rest;
  }
}

async function detectVendor(providerId: string) {
  if (detectVendorBusy.value[providerId]) return;
  detectVendorBusy.value = { ...detectVendorBusy.value, [providerId]: true };
  try {
    const result = await api.providers.detectVendor(providerId);
    // Reload credentials for this provider so vendor badges appear
    const creds = await api.credentials.list(providerId);
    credsByProvider.value = { ...credsByProvider.value, [providerId]: creds };
    // Auto-refresh balance for all creds that now have a vendor
    const vendored = creds.filter((c) => c.upstream_vendor);
    await Promise.all(vendored.map((c) => refreshCredentialBalance(c.id)));
    if (result.upstream_vendor) {
      error.value = "";
    } else {
      error.value = `未能识别 ${providerId} 的供应商类型（无 NewAPI/Sub2API 特征）`;
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [providerId]: _, ...rest } = detectVendorBusy.value;
    detectVendorBusy.value = rest;
  }
}

async function speedtestProvider(providerId: string) {
  if (speedtestBusy.value[providerId]) return;
  speedtestBusy.value = { ...speedtestBusy.value, [providerId]: true };
  try {
    const updated = await api.providers.probe(providerId);
    replaceProviderInList(updated);
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [providerId]: _, ...rest } = speedtestBusy.value;
    speedtestBusy.value = rest;
  }
}

async function speedtestProviders(providerIds: string[]) {
  await Promise.all(providerIds.map((providerId) => speedtestProvider(providerId)));
}

async function refreshProviderModelsForProviders(providerIds: string[]) {
  await Promise.all(providerIds.map((providerId) => refreshProviderModels(providerId)));
}

function poolCred(providerId: string, credentialId: string): CredentialPoolStatus | undefined {
  return poolByProviderId.value[providerId]?.credentials.find(
    (x) => x.credential_id === credentialId,
  );
}

function formatCooldown(seconds: number | bigint | null | undefined): string {
  if (seconds == null) return "";
  const total = Number(seconds);
  if (!Number.isFinite(total) || total <= 0) return "0s";
  const mins = Math.floor(total / 60);
  const secs = total % 60;
  if (mins <= 0) return `${secs}s`;
  if (secs === 0) return `${mins}m`;
  return `${mins}m ${secs}s`;
}

function codexCliRouteAriaLabel(provider: Provider): string {
  const t = codexRouteTool.value;
  return `codex.route ${t.pathPrefix} -> ${displayProviderName(provider.name)} (${provider.kind})`;
}

const PROVIDER_TAB_OPTIONS: ProviderTabOption[] = [
  {
    id: "common",
    label: "Common",
    shortLabel: "all",
    icon: "i-lucide-compass",
    description: "",
  },
  ...CLIENT_TOOLS.map((tool) => ({
    id: tool.id,
    label: tool.label,
    shortLabel: tool.shortLabel,
    icon: tool.icon,
    description: tool.setupHint,
  })),
];

function providerGroupName(provider: Provider): string {
  const trimmed = provider.group_name?.trim();
  if (trimmed) return trimmed;
  return "Ungrouped";
}

function providerGroupKey(provider: Provider): string {
  return providerGroupName(provider).toLowerCase();
}

function providerIdsFromSection(section: ProviderSectionView): string[] {
  return section.providers.map((card) => card.provider.id);
}

function sectionSpeedtestBusy(section: ProviderSectionView): boolean {
  return providerIdsFromSection(section).some((providerId) => !!speedtestBusy.value[providerId]);
}

function sectionModelRefreshBusy(section: ProviderSectionView): boolean {
  return providerIdsFromSection(section).some((providerId) => !!modelRefreshBusy.value[providerId]);
}

const providerTabs = computed(() => PROVIDER_TAB_OPTIONS);
const activeToolTab = computed<ClientToolInfo | null>(() => {
  if (activeProviderTab.value === "common") return null;
  return CLIENT_TOOLS.find((tool) => tool.id === activeProviderTab.value) ?? null;
});

const providerSections = computed<ProviderSectionView[]>(() =>
  buildProviderSections({
    providers: providers.value,
    selectedTool: activeToolTab.value,
    healthMap: healthMap.value,
    poolByProviderId: poolByProviderId.value,
    activeRequestCountsByProvider: activeRequestCountsByProvider.value,
    liveTokensPerSecByProvider: liveTokensPerSecByProvider.value,
    liveRequestMetrics: liveRequestMetrics.value,
  }),
);
const providerRollingStatById = computed(() => {
  const map = new Map<string, NonNullable<ProviderHealthSummary["rolling"]>>();
  for (const [providerId, health] of Object.entries(healthMap.value)) {
    if (health.rolling) map.set(providerId, health.rolling);
  }
  return map;
});
const activeRequestCountsByProvider = computed(() => {
  const counts = new Map<string, number>();
  for (const providerId of Object.values(activeRequestProviderIds.value)) {
    counts.set(providerId, (counts.get(providerId) ?? 0) + 1);
  }
  return counts;
});
const activeCredentialCountsByProvider = computed(() => {
  const byProvider: Record<string, Record<string, number>> = {};
  for (const attempt of Object.values(activeAttemptCredentials.value)) {
    const current = byProvider[attempt.providerId] ?? {};
    current[attempt.credentialId] = (current[attempt.credentialId] ?? 0) + 1;
    byProvider[attempt.providerId] = current;
  }
  return byProvider;
});
const liveTokensPerSecByProvider = computed(() => {
  const totals = new Map<string, number>();
  for (const metric of Object.values(liveRequestMetrics.value)) {
    const tps = metric.active_request_tokens_per_sec;
    if (!Number.isFinite(tps ?? NaN) || !metric.provider_id) continue;
    totals.set(metric.provider_id, (totals.get(metric.provider_id) ?? 0) + (tps ?? 0));
  }
  return totals;
});

function targetProviderIdFromRoute(): string | null {
  const raw = route.query.provider;
  if (Array.isArray(raw)) return raw[0] ?? null;
  return raw ?? null;
}

function escapeProviderDomIdSegment(value: string): string {
  const cssApi = globalThis.CSS;
  if (cssApi && typeof cssApi.escape === "function") return cssApi.escape(value);
  return value.replace(/[^a-zA-Z0-9_-]/g, "$&");
}

async function scrollToTargetProvider() {
  const providerId = targetProviderIdFromRoute();
  if (!providerId) return;
  await nextTick();
  const el = document.getElementById(`provider-${escapeProviderDomIdSegment(providerId)}`);
  if (!el) return;
  highlightedProviderId.value = providerId;
  el.scrollIntoView({ block: "center", behavior: "smooth" });
  window.setTimeout(() => {
    if (highlightedProviderId.value === providerId) highlightedProviderId.value = null;
  }, 2200);
}

async function loadAndScrollToTargetProvider() {
  try {
    await load();
    await scrollToTargetProvider();
  } catch (e) {
    error.value = String(e);
  }
}

function startAdd() {
  editTarget.value = null;
  showForm.value = true;
}
function startEdit(p: Provider) {
  editTarget.value = p;
  showForm.value = true;
}

async function save(payload: ProviderInput, credentialAuthRef: string | null = null) {
  const providerPayload: ProviderInput = { ...payload, auth_ref: null };
  try {
    let providerId: string;
    if (editTarget.value) {
      providerId = editTarget.value.id;
      await api.providers.update(providerId, providerPayload);
    } else {
      const created = await api.providers.create(providerPayload);
      providerId = created.id;
      api.providers.refreshModels(providerId).catch(() => {});
    }
    if (credentialAuthRef?.trim()) {
      await api.credentials.create(providerId, {
        ...emptyCredForm(),
        label: "API Key",
        auth_ref: normalizeAuthRef(credentialAuthRef.trim()),
        notes: "Created from provider wizard paste",
      });
    }
    showForm.value = false;
    await load();
  } catch (e) {
    error.value = String(e);
  }
}

async function toggleProviderEnabled(p: Provider) {
  if (toggleBusy.value[p.id]) return;
  toggleBusy.value = { ...toggleBusy.value, [p.id]: true };
  const next = !p.enabled;
  try {
    await api.providers.update(p.id, {
      name: p.name,
      group_name: p.group_name ?? null,
      kind: p.kind,
      base_url: p.base_url,
      auth_ref: null,
      enabled: next,
      priority: p.priority,
      supports_websocket: p.supports_websocket ?? null,
      passthrough_mode: p.passthrough_mode ?? true,
      model_aliases: [...p.model_aliases],
    });
    const ix = providers.value.findIndex((x) => x.id === p.id);
    if (ix >= 0) {
      providers.value[ix] = { ...providers.value[ix], enabled: next };
      providers.value = [...providers.value];
    }
    // Bind toggle to circuit state: clear provider/credential circuit breaks on re-enable to avoid enabled-but-blocked state.
    if (next) {
      await api.providers.resetCircuit(p.id);
      const fresh = await api.providers.health(p.id, GATEWAY_ROLLING_STAT_HOURS);
      healthMap.value = { ...healthMap.value, [p.id]: fresh };
      await refreshSinglePool(p.id);
    }
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [p.id]: _, ...rest } = toggleBusy.value;
    toggleBusy.value = rest;
  }
}

async function resetProviderCircuit(providerId: string) {
  if (circuitResetBusy.value[providerId]) return;
  circuitResetBusy.value = { ...circuitResetBusy.value, [providerId]: true };
  try {
    await api.providers.resetCircuit(providerId);
    const fresh = await api.providers.health(providerId, GATEWAY_ROLLING_STAT_HOURS);
    healthMap.value = { ...healthMap.value, [providerId]: fresh };
    await refreshSinglePool(providerId);
    error.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [providerId]: _, ...rest } = circuitResetBusy.value;
    circuitResetBusy.value = rest;
  }
}

async function remove(id: string) {
  if (!confirm("Remove this provider?")) return;
  try {
    await api.providers.delete(id);
    await load();
  } catch (e) {
    error.value = String(e);
  }
}

function viewProviderLogs(providerId: string) {
  void router.push({ path: "/ui/monitor", query: { ...route.query, provider_id: providerId } });
}

// Credential actions
function startAddCred(providerId: string) {
  credForm.value = emptyCredForm();
  credAuthMode.value = "apikey";
  editCred.value = null;
  credProviderId.value = providerId;
  credLoginPassword.value = "";
  credLoginNote.value = null;
  credGroups.value = [];
  resetAuthJsonImportUi();
  showCredForm.value = true;
}

function startEditCred(cred: Credential) {
  const isOAuth = !!cred.oauth_access_token || cred.oauth_has_refresh;
  credAuthMode.value = isOAuth ? "oauth" : "apikey";
  resetAuthJsonImportUi();
  credForm.value = {
    label: cred.label,
    auth_ref: cred.auth_ref,
    plan_type: cred.plan_type,
    notes: cred.notes,
    enabled: cred.enabled,
    priority: cred.priority,
    oauth_access_token: cred.oauth_access_token,
    oauth_refresh_token: null, // write-only: never returned from server
    oauth_expires_at: cred.oauth_expires_at,
    upstream_vendor: cred.upstream_vendor ?? null,
    upstream_username: cred.upstream_username ?? null,
    upstream_group: cred.upstream_group ?? null,
    price_multiplier: cred.price_multiplier ?? 1.0,
  };
  editCred.value = cred;
  credProviderId.value = cred.provider_id;
  showCredForm.value = true;
}

/** Known auth_ref schemes; treat other raw strings as literal fallback to avoid storing pasted sk-* keys as unresolved auth_refs. */
const KNOWN_AUTH_REF_SCHEMES = ["literal:", "env:", "keyring:", "passthrough"];

function normalizeAuthRef(raw: string | null): string | null {
  if (!raw) return raw;
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (KNOWN_AUTH_REF_SCHEMES.some((s) => trimmed === s || trimmed.startsWith(s))) {
    return trimmed;
  }
  return `literal:${trimmed}`;
}

async function doCredLogin() {
  if (!editCred.value) return;
  const username = credForm.value.upstream_username?.trim();
  const password = credLoginPassword.value.trim();
  if (!username || !password) return;
  credLoginBusy.value = true;
  credLoginNote.value = null;
  try {
    const res = await api.credentials.login(editCred.value.id, { username, password });
    credLoginNote.value = res.ok ? "登录成功" : (res.note ?? "登录失败");
    if (res.ok) credLoginPassword.value = "";
  } catch (e) {
    credLoginNote.value = String(e);
  } finally {
    credLoginBusy.value = false;
  }
}

async function fetchCredGroups() {
  if (!editCred.value) return;
  credGroupsBusy.value = true;
  try {
    credGroups.value = await api.credentials.groups(editCred.value.id);
  } catch {
    credGroups.value = [];
  } finally {
    credGroupsBusy.value = false;
  }
}

async function saveCred() {
  try {
    const payload = {
      ...credForm.value,
      auth_ref:
        credAuthMode.value === "apikey"
          ? normalizeAuthRef(credForm.value.auth_ref)
          : credForm.value.auth_ref,
    };
    if (editCred.value) {
      await api.credentials.update(editCred.value.id, payload);
    } else {
      await api.credentials.create(credProviderId.value, payload);
    }
    showCredForm.value = false;
    await loadCreds(credProviderId.value);
    await refreshSinglePool(credProviderId.value);
  } catch (e) {
    error.value = String(e);
  }
}

async function removeCred(cred: Credential) {
  if (!confirm(`Remove credential "${cred.label}"?`)) return;
  try {
    await api.credentials.delete(cred.id);
    await loadCreds(cred.provider_id);
    await refreshSinglePool(cred.provider_id);
  } catch (e) {
    error.value = String(e);
  }
}

async function toggleCredentialEnabled(cred: Credential) {
  if (credToggleBusy.value[cred.id]) return;
  credToggleBusy.value = { ...credToggleBusy.value, [cred.id]: true };
  const nextEnabled = !cred.enabled;
  try {
    await api.credentials.update(cred.id, {
      label: cred.label,
      auth_ref: cred.auth_ref,
      plan_type: cred.plan_type,
      notes: cred.notes,
      enabled: nextEnabled,
      priority: cred.priority,
      oauth_access_token: cred.oauth_access_token,
      oauth_refresh_token: null,
      oauth_expires_at: cred.oauth_expires_at,
      oauth_cached_email: cred.oauth_account_email ?? null,
      oauth_cached_subject: cred.oauth_account_subject ?? null,
      oauth_cached_plan_slug: cred.oauth_chatgpt_plan_slug ?? null,
    });
    await loadCreds(cred.provider_id);
    await refreshSinglePool(cred.provider_id);
  } catch (e) {
    error.value = String(e);
  } finally {
    const { [cred.id]: _, ...rest } = credToggleBusy.value;
    credToggleBusy.value = rest;
  }
}

function circuitBadge(state: string, remainingSecs?: number | bigint | null) {
  if (state === "closed") {
    return { label: "ok", detail: "", cls: "bg-emerald-50 text-emerald-800 border-emerald-200" };
  }
  if (state === "half-open") {
    return {
      label: "half-open",
      detail: "probing",
      cls: "bg-amber-50 text-amber-900 border-amber-200",
    };
  }
  return {
    label: "open",
    detail: remainingSecs != null ? `cd ${formatCooldown(remainingSecs)}` : "cooling down",
    cls: "bg-red-50 text-red-800 border-red-200",
  };
}

watch(showForm, async (open) => {
  if (!open) return;
  if (editTarget.value) await reloadProviderCreds(editTarget.value.id);
});

onMounted(() => {
  void loadAndScrollToTargetProvider();
});
onUnmounted(() => {
  if (providersOverviewFallbackTimer) {
    clearTimeout(providersOverviewFallbackTimer);
    providersOverviewFallbackTimer = null;
  }
});

watch(
  () => route.query.provider,
  () => {
    void scrollToTargetProvider().catch((e) => {
      error.value = String(e);
    });
  },
);

useWs((ev: unknown) => {
  const e = ev as
    | {
        type?: string;
        request_id?: string;
        rolling_hours?: number;
        attempt_id?: string;
        provider_id?: string | null;
        credential_id?: string | null;
        request_id?: string;
        id?: string;
        providers?: Provider[];
        health?: ProviderHealthSummary[];
        pools?: ProviderAuthPoolSummary[];
        credentials?: Record<string, Credential[]>;
        codex_plans?: Record<string, ProviderCodexPlanItem[]>;
      }
    | ({ type?: string } & RequestRuntimeStats);
  if (e.rolling_hours != null && e.rolling_hours !== GATEWAY_ROLLING_STAT_HOURS) return;
  if (
    e.type?.startsWith("providers-overview-") &&
    e.request_id &&
    providersOverviewStreamRequestId &&
    e.request_id !== providersOverviewStreamRequestId
  ) {
    return;
  }
  if (e.type === "providers-overview-stream-started") {
    loading.value = true;
    error.value = "";
    providers.value = [];
    healthMap.value = {};
    poolByProviderId.value = {};
    credsByProvider.value = {};
    loadingCreds.value = {};
    resetCodexPlans();
    return;
  }
  if (e.type === "providers-overview-providers-chunk" && e.providers) {
    providers.value = e.providers;
    loadingCreds.value = Object.fromEntries(e.providers.map((provider) => [provider.id, true]));
    return;
  }
  if (e.type === "providers-overview-health-chunk" && e.health) {
    const map: Record<string, ProviderHealthSummary> = {};
    for (const body of e.health) {
      if (!isProviderHealthSummary(body)) continue;
      map[body.cumulative.provider_id] = body;
    }
    healthMap.value = map;
    return;
  }
  if (e.type === "providers-overview-pools-chunk" && e.pools) {
    poolByProviderId.value = Object.fromEntries(e.pools.map((pool) => [pool.provider_id, pool]));
    return;
  }
  if (e.type === "providers-overview-credentials-chunk" && e.provider_id && e.credentials) {
    credsByProvider.value = { ...credsByProvider.value, [e.provider_id]: e.credentials };
    loadingCreds.value = { ...loadingCreds.value, [e.provider_id]: false };
    return;
  }
  if (e.type === "providers-overview-codex-plans-chunk" && e.provider_id) {
    const rows = (e as { codex_plans?: ProviderCodexPlanItem[] }).codex_plans ?? [];
    applyCodexPlanRows(e.provider_id, rows);
    return;
  }
  if (e.type === "providers-overview-stream-ended") {
    if (providersOverviewFallbackTimer) {
      clearTimeout(providersOverviewFallbackTimer);
      providersOverviewFallbackTimer = null;
    }
    loadingCreds.value = Object.fromEntries(
      providers.value.map((provider) => [provider.id, false]),
    );
    loading.value = false;
    void runCodexWhamBackgroundRefresh();
    return;
  }
  if (e.type === "providers-overview-changed") {
    const overview = e as ProvidersOverview;
    error.value = "";
    applyProvidersOverview(overview);
    return;
  }
  if (e.type === "upstream-attempt-started" && e.attempt_id && e.provider_id) {
    if (e.credential_id) {
      activeAttemptCredentials.value = {
        ...activeAttemptCredentials.value,
        [e.attempt_id]: { providerId: e.provider_id, credentialId: e.credential_id },
      };
    }
    return;
  }
  if (e.type === "request-updated" && e.request_id && e.provider_id) {
    activeRequestProviderIds.value = {
      ...activeRequestProviderIds.value,
      [e.request_id]: e.provider_id,
    };
    liveRequestMetrics.value = {
      ...liveRequestMetrics.value,
      [e.request_id]: {
        request_id: e.request_id,
        provider_id: e.provider_id,
        upstream_first_byte_ms: e.upstream_first_byte_ms,
        active_request_tokens_per_sec: e.active_request_tokens_per_sec,
        active_upstream_decode_tps: e.active_upstream_decode_tps,
        active_downstream_emit_tps: e.active_downstream_emit_tps,
        updated_at: e.updated_at,
      },
    };
    return;
  }
  if (e.type === "upstream-attempt-finished" && e.attempt_id) {
    const { [e.attempt_id]: _, ...rest } = activeAttemptCredentials.value;
    activeAttemptCredentials.value = rest;
    return;
  }
  if (e.type === "log-appended" && e.id) {
    const { [e.id]: _, ...reqRest } = activeRequestProviderIds.value;
    activeRequestProviderIds.value = reqRest;
    const { [e.id]: __, ...metricRest } = liveRequestMetrics.value;
    liveRequestMetrics.value = metricRest;
  }
});
</script>

<template>
  <div class="mx-auto w-full max-w-[1040px]">
    <UiCard class="relative mb-4 overflow-hidden">
      <div
        class="relative z-10 flex flex-col gap-3 p-4 sm:flex-row sm:items-start sm:justify-between"
      >
        <div class="min-w-0 flex-1">
          <span :class="['text-xs uppercase', pageAccent.kicker]">Gateway</span>
          <h1 :class="['text-2xl font-bold tracking-tight', pageAccent.heading]">Providers</h1>
        </div>
        <div class="flex w-full shrink-0 flex-wrap items-center justify-end gap-2 sm:w-auto">
          <UiButton
            type="button"
            variant="outline"
            class="min-h-11 sm:min-h-9"
            title="Local import"
            aria-label="Local import"
            @click="showImportModal = true"
          >
            <VpIcon name="folder-input" size-class="size-4 shrink-0" />
            <span class="hidden sm:inline">Import</span>
          </UiButton>
          <UiButton
            type="button"
            class="min-h-11 min-w-11 sm:min-h-9"
            aria-label="provider:add"
            title="provider:add"
            @click="startAdd"
          >
            <VpIcon name="plus" size-class="size-4 shrink-0 text-white" />
            <span class="sr-only">provider:add</span>
          </UiButton>
        </div>
      </div>
    </UiCard>

    <div
      v-if="error"
      class="mb-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
    >
      {{ error }}
    </div>

    <div v-if="loading" class="space-y-4">
      <UiSkeleton class="h-28 w-full" />
      <UiSkeleton class="h-36 w-full" />
      <UiSkeleton class="h-36 w-full" />
    </div>
    <div
      v-else-if="providers.length === 0"
      class="rounded-xl border border-dashed border-border bg-card py-12 text-center font-mono text-sm text-muted-foreground"
      title="empty"
      aria-label="empty"
    >
      ∅
    </div>
    <ProviderSections
      v-else
      :sections="providerSections"
      :health-map="healthMap"
      :creds-by-provider="credsByProvider"
      :loading-creds="loadingCreds"
      :toggle-busy="toggleBusy"
      :circuit-reset-busy="circuitResetBusy"
      :speedtest-busy="speedtestBusy"
      :model-refresh-busy="modelRefreshBusy"
      :cred-model-refresh-busy="credModelRefreshBusy"
      :cred-balance-refresh-busy="credBalanceRefreshBusy"
      :cred-toggle-busy="credToggleBusy"
      :pool-by-provider-id="poolByProviderId"
      :plan-snap-by-cred="planSnapByCred"
      :active-credential-counts-by-provider="activeCredentialCountsByProvider"
      :active-request-counts-by-provider="activeRequestCountsByProvider"
      :live-tokens-per-sec-by-provider="liveTokensPerSecByProvider"
      :provider-rolling-stat-by-id="providerRollingStatById"
      :detect-vendor-busy="detectVendorBusy"
      :highlighted-provider-id="highlightedProviderId"
      @speedtest-providers="speedtestProviders"
      @refresh-provider-models-for-providers="refreshProviderModelsForProviders"
      @sync-creds="reloadProviderCreds"
      @detect-vendor="detectVendor"
      @speedtest-provider="speedtestProvider"
      @refresh-models="refreshProviderModels"
      @refresh-cred-models="refreshCredentialModels"
      @refresh-cred-balance="refreshCredentialBalance"
      @toggle-provider="toggleProviderEnabled"
      @reset-circuit="resetProviderCircuit"
      @edit-provider="startEdit"
      @delete-provider="remove"
      @add-cred="startAddCred"
      @toggle-cred="toggleCredentialEnabled"
      @edit-cred="startEditCred"
      @delete-cred="removeCred"
      @view-logs="viewProviderLogs"
    />

    <ProviderSmartModal
      :open="showForm"
      :edit-target="editTarget"
      :creds="editTarget ? (credsByProvider[editTarget.id] ?? []) : []"
      :loading-creds="!!(editTarget && loadingCreds[editTarget.id])"
      :cred-toggle-busy="credToggleBusy"
      :model-refresh-busy="!!(editTarget && modelRefreshBusy[editTarget.id])"
      :speed-label="editProviderSpeedLabel"
      @close="showForm = false"
      @save="(form, credKey) => save(form, credKey)"
      @refresh-models="editTarget && refreshProviderModels(editTarget.id)"
      @add-credential="editTarget && startAddCred(editTarget.id)"
      @reload-creds="editTarget && reloadProviderCreds(editTarget.id)"
      @edit-credential="startEditCred($event)"
      @remove-credential="removeCred($event)"
      @toggle-credential="toggleCredentialEnabled($event)"
    />

    <ProviderImportModal
      :open="showImportModal"
      :view="workspaceView"
      @close="showImportModal = false"
      @imported="load()"
    />

    <CredentialFormModal
      :open="showCredForm"
      :edit-cred="editCred"
      :edit-target="editTarget"
      :cred-form="credForm"
      :cred-auth-mode="credAuthMode"
      :auth-json-paste="authJsonPaste"
      :auth-json-paste-err="authJsonPasteErr"
      :auth-json-drag-active="authJsonDragActive"
      :auth-json-file-input-ref="authJsonFileInputRef"
      :cred-login-password="credLoginPassword"
      :cred-login-busy="credLoginBusy"
      :cred-login-note="credLoginNote"
      :cred-groups="credGroups"
      :cred-groups-busy="credGroupsBusy"
      @close="showCredForm = false"
      @save="saveCred"
      @update:cred-form="credForm = $event"
      @update:cred-auth-mode="credAuthMode = $event"
      @update:auth-json-paste="authJsonPaste = $event"
      @update:auth-json-file-input-ref="authJsonFileInputRef = $event"
      @update:cred-login-password="credLoginPassword = $event"
      @parse-auth-json-paste="parseAuthJsonPaste"
      @trigger-auth-json-file-pick="triggerAuthJsonFilePick"
      @auth-json-file-change="onAuthJsonFileChange"
      @auth-json-drag-over="onAuthJsonDragOver"
      @auth-json-drag-leave="onAuthJsonDragLeave"
      @auth-json-drop="onAuthJsonDrop"
      @refresh-provider-models="refreshProviderModels"
      @do-cred-login="doCredLogin"
      @fetch-cred-groups="fetchCredGroups"
    />
  </div>
</template>

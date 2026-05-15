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
  type CredentialPlanSnapshot,
  type ProviderCodexPlanItem,
  type ProvidersOverview,
  type RequestRuntimeStats,
  isProviderHealthSummary,
} from "../api/client.ts";
import {
  CLIENT_TOOLS,
  getCodexClientTool,
  getToolProtocolSupport,
  type ClientToolId,
  type ClientToolInfo,
  type ProtocolSupportInfo,
} from "../utils/client-tools.ts";
import { useRoute, useRouter } from "vue-router";
import { resolvePageAccent } from "../utils/page-accent.ts";
import { displayProviderName } from "../utils/providers-display.ts";
import { hintsFromAuthJsonTokens } from "../utils/codex-oauth-hints.ts";
import VpIcon from "../components/vp-icon.vue";
import ProviderCard from "../components/provider-card.vue";
import ProviderSmartModal from "../components/provider-smart-modal.vue";
import ProviderImportModal from "../components/provider-import-modal.vue";
import { requestWsSnapshot, useWs } from "../composables/useProxy.ts";
import { useIntakeFlow, INTAKE_FLOW_IMPORTED_EVENT } from "../composables/use-intake-flow.ts";
import { workspaceViewFromQuery, type WorkspaceView } from "../utils/workspace-view.ts";

const intakeFlow = useIntakeFlow();

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
const CODEX_PLAN_AUTO_REFRESH_COOLDOWN_MS = 15 * 60 * 1000;
const CODEX_PLAN_STALE_AFTER_MS = 30 * 60 * 1000;
const planSnapByCred = ref<Record<string, CredentialPlanSnapshot | null>>({});
/** Latest ChatGPT `wham/usage` or header snapshot per credential on official Codex providers. */
const codexPlanRowsByProvider = ref<Record<string, ProviderCodexPlanItem[]>>({});
const codexRefreshNote = ref<Record<string, string>>({});
/** True while POST …/codex-plan/refresh is in flight for that provider. */
const codexPlanRefreshing = ref<Record<string, boolean>>({});
const codexPlanAutoRefreshAttemptAt = ref<Record<string, number>>({});
const loading = ref(true);
const error = ref("");
type LiveRequestMetric = {
  request_id: string;
  provider_id: string;
  upstream_first_byte_ms: number | null;
  active_request_tokens_per_sec: number | null;
  active_upstream_decode_tps: number | null;
  active_downstream_emit_tps: number | null;
  updated_at: number;
};

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

function isOfficialCodexProvider(p: Provider): boolean {
  if (p.kind !== "openai-responses") return false;
  const u = p.base_url.toLowerCase();
  return u.includes("chatgpt.com") && u.includes("backend-api") && u.includes("codex");
}

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
    if (opts?.reloadCreds ?? true) await loadCreds(providerId);
    await refreshSinglePool(providerId);
  } catch (e) {
    codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: String(e) };
  } finally {
    codexPlanRefreshing.value = { ...codexPlanRefreshing.value, [providerId]: false };
  }
}

/** After list load: refresh missing/stale ChatGPT plan snapshots, sequentially and with cooldown. */
async function runCodexWhamBackgroundRefresh() {
  const targets = providers.value.filter(
    (p) => isOfficialCodexProvider(p) && shouldAutoRefreshCodexPlan(p.id),
  );
  for (const p of targets) {
    await refreshCodexPlanFromChatgpt(p.id, { silent: true, reloadCreds: false });
    await new Promise((res) => setTimeout(res, 400));
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

type ProviderGroupKey = "native" | "bridged" | "other";

interface ProviderTabOption {
  id: "common" | ClientToolId;
  label: string;
  shortLabel: string;
  icon: string;
  description: string;
}

interface ProviderCardProtocolBadge {
  toolId: ClientToolId;
  toolLabel: string;
  toolIcon: string;
  support: ProtocolSupportInfo;
}

interface ProviderCardView {
  provider: Provider;
  title: string;
  badges: ProviderCardProtocolBadge[];
  primarySupport: ProtocolSupportInfo | null;
  group: ProviderGroupKey;
  qualityScore: number;
  sortReason: string;
  sortKey: string;
}

interface ProviderSectionView {
  key: string;
  title: string;
  description: string;
  summary: ProviderSectionSummary;
  providers: ProviderCardView[];
}

interface ProviderSectionSummary {
  totalEndpoints: number;
  enabledEndpoints: number;
  nativeEndpoints: number;
  bridgedEndpoints: number;
  availableCredentials: number;
  enabledCredentials: number;
  blockedCredentials: number;
  activeRequests: number;
  fastestLatencyMs: number | null;
  remoteModels: number;
  testedEndpoints: number;
  directEndpoints: number;
  wsEndpoints: number;
  passthroughEndpoints: number;
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

function latencyCandidatesForProvider(provider: Provider): number[] {
  const health = healthMap.value[provider.id];
  const values = [
    providerLiveFirstByteMs(provider.id),
    provider.last_speedtest?.latency_ms ?? null,
    health?.rolling?.avg_latency_ms ?? null,
    health?.cumulative.avg_latency_ms ?? null,
  ];
  return values.filter(
    (value): value is number => typeof value === "number" && Number.isFinite(value),
  );
}

function summarizeProviderSection(cards: ProviderCardView[]): ProviderSectionSummary {
  let enabledEndpoints = 0;
  let nativeEndpoints = 0;
  let bridgedEndpoints = 0;
  let availableCredentials = 0;
  let enabledCredentials = 0;
  let blockedCredentials = 0;
  let activeRequests = 0;
  let remoteModels = 0;
  let testedEndpoints = 0;
  let directEndpoints = 0;
  let wsEndpoints = 0;
  let passthroughEndpoints = 0;
  const latencies: number[] = [];

  for (const card of cards) {
    const provider = card.provider;
    const pool = poolByProviderId.value[provider.id];
    if (provider.enabled) enabledEndpoints += 1;
    if (card.group === "native") nativeEndpoints += 1;
    if (card.group === "bridged") bridgedEndpoints += 1;
    if (provider.passthrough_mode) passthroughEndpoints += 1;
    if (provider.supports_websocket === true) wsEndpoints += 1;
    if (provider.last_speedtest) testedEndpoints += 1;
    if (!provider.base_url.includes("127.0.0.1") && !provider.base_url.includes("localhost")) {
      directEndpoints += 1;
    }
    remoteModels += provider.remote_models?.length ?? 0;
    activeRequests += activeRequestCountsByProvider.value.get(provider.id) ?? 0;
    availableCredentials += pool?.available_credentials ?? 0;
    enabledCredentials += pool?.enabled_credentials ?? 0;
    blockedCredentials +=
      (pool?.rate_limited_credentials ?? 0) + (pool?.open_circuit_credentials ?? 0);
    latencies.push(...latencyCandidatesForProvider(provider));
  }

  return {
    totalEndpoints: cards.length,
    enabledEndpoints,
    nativeEndpoints,
    bridgedEndpoints,
    availableCredentials,
    enabledCredentials,
    blockedCredentials,
    activeRequests,
    fastestLatencyMs: latencies.length ? Math.min(...latencies) : null,
    remoteModels,
    testedEndpoints,
    directEndpoints,
    wsEndpoints,
    passthroughEndpoints,
  };
}

function providerSectionDescription(summary: ProviderSectionSummary): string {
  const pieces = [
    summary.nativeEndpoints ? `${summary.nativeEndpoints} native` : "",
    summary.bridgedEndpoints ? `${summary.bridgedEndpoints} bridge` : "",
    summary.availableCredentials
      ? `${summary.availableCredentials}/${summary.enabledCredentials} cred`
      : "no cred",
    summary.fastestLatencyMs == null
      ? "not tested"
      : `${Math.round(summary.fastestLatencyMs)}ms best`,
    summary.remoteModels ? `${summary.remoteModels} models` : "",
  ].filter(Boolean);
  return pieces.join(" · ");
}

function providerCardBadges(provider: Provider): ProviderCardProtocolBadge[] {
  return CLIENT_TOOLS.filter((tool) => tool.consumesKinds.includes(provider.kind)).map((tool) => ({
    toolId: tool.id,
    toolLabel: tool.shortLabel,
    toolIcon: tool.icon,
    support: getToolProtocolSupport(provider, tool),
  }));
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value));
}

function providerLiveFirstByteMs(providerId: string): number | null {
  const values = Object.values(liveRequestMetrics.value)
    .filter((metric) => metric.provider_id === providerId)
    .map((metric) => metric.upstream_first_byte_ms)
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (!values.length) return null;
  return Math.min(...values);
}

function providerCompositeScore(provider: Provider): { score: number; reason: string } {
  const health = healthMap.value[provider.id];
  const rolling = health?.rolling ?? null;
  const pool = poolByProviderId.value[provider.id];
  const activeRequests = activeRequestCountsByProvider.value.get(provider.id) ?? 0;
  const liveTps = liveTokensPerSecByProvider.value.get(provider.id) ?? 0;
  const rollingTps = rolling?.decode_output_tokens_per_sec || rolling?.output_tokens_per_sec || 0;
  const tps = liveTps || rollingTps;
  const successRate =
    rolling && rolling.requests > 0
      ? rolling.success_rate
      : (health?.cumulative.success_rate ?? (provider.enabled ? 1 : 0));
  const latencyMs =
    providerLiveFirstByteMs(provider.id) ??
    provider.last_speedtest?.latency_ms ??
    rolling?.avg_latency_ms ??
    health?.cumulative.avg_latency_ms ??
    null;
  const availableCreds = pool?.available_credentials ?? 0;
  const enabledCreds = pool?.enabled_credentials ?? 0;
  const circuitOpen = pool?.provider_circuit_open || health?.cumulative.circuit_state === "open";
  const rateLimited = pool?.rate_limited_credentials ?? 0;
  const openCreds = pool?.open_circuit_credentials ?? 0;
  const priorityScore = Math.max(0, 240 - provider.priority);
  const latencyScore = latencyMs == null ? 120 : 260 * (1 - clamp01(latencyMs / 5000));
  const speedScore = Math.min(360, tps * 10);
  const score =
    activeRequests * 5000 +
    (provider.enabled ? 650 : -1600) +
    (circuitOpen ? -1200 : 250) +
    availableCreds * 180 +
    Math.min(180, enabledCreds * 40) -
    rateLimited * 120 -
    openCreds * 160 +
    successRate * 900 +
    latencyScore +
    speedScore +
    priorityScore;
  const reasonParts = [
    activeRequests ? `live x${activeRequests}` : "",
    `${Math.round(successRate * 100)}% ok`,
    latencyMs == null ? "" : `${Math.round(latencyMs)}ms first`,
    tps ? `${tps.toFixed(1)} tok/s` : "",
    availableCreds ? `${availableCreds} cred` : "no cred",
  ].filter(Boolean);
  return { score, reason: reasonParts.join(" · ") };
}

function rankProviderCard(
  provider: Provider,
  selectedTool: ClientToolInfo | null,
): ProviderCardView {
  const badges = providerCardBadges(provider);
  const title = displayProviderName(provider.name);
  const primarySupport = selectedTool ? getToolProtocolSupport(provider, selectedTool) : null;
  const firstUsefulSupport =
    primarySupport ??
    badges.map((badge) => badge.support).sort((a, b) => a.order - b.order)[0] ??
    null;

  let group: ProviderGroupKey = "other";
  if (primarySupport) {
    group =
      primarySupport.mode === "native"
        ? "native"
        : primarySupport.mode === "bridged"
          ? "bridged"
          : "other";
  } else {
    const hasNative = badges.some((badge) => badge.support.mode === "native");
    const hasBridged = badges.some((badge) => badge.support.mode === "bridged");
    group = hasNative ? "native" : hasBridged ? "bridged" : "other";
  }
  const quality = providerCompositeScore(provider);
  const normalizedTitle = title.toLocaleLowerCase("zh-Hans-CN");

  return {
    provider,
    title,
    badges,
    primarySupport: firstUsefulSupport,
    group,
    qualityScore: quality.score,
    sortReason: quality.reason,
    sortKey: `${provider.enabled ? "0" : "1"}:${normalizedTitle}:${provider.id}`,
  };
}

const activeToolTab = computed<ClientToolInfo | null>(() => {
  if (workspaceView.value === "codex")
    return CLIENT_TOOLS.find((tool) => tool.id === "codex") ?? null;
  if (workspaceView.value === "claude")
    return CLIENT_TOOLS.find((tool) => tool.id === "claude-code") ?? null;
  if (activeProviderTab.value === "common") return null;
  return CLIENT_TOOLS.find((tool) => tool.id === activeProviderTab.value) ?? null;
});

const providerTabs = computed(() => PROVIDER_TAB_OPTIONS);

const providerSections = computed<ProviderSectionView[]>(() => {
  const selectedTool = activeToolTab.value;
  const cards = providers.value
    .map((provider) => rankProviderCard(provider, selectedTool))
    .filter((card) => {
      if (!selectedTool) return true;
      return selectedTool.consumesKinds.includes(card.provider.kind);
    })
    .sort((a, b) => a.sortKey.localeCompare(b.sortKey));

  const grouped = new Map<string, ProviderSectionView>();
  for (const card of cards) {
    const key = providerGroupKey(card.provider);
    const title = providerGroupName(card.provider);
    const section =
      grouped.get(key) ??
      ({
        key,
        title,
        description: "",
        summary: summarizeProviderSection([]),
        providers: [],
      } satisfies ProviderSectionView);
    section.providers.push(card);
    grouped.set(key, section);
  }

  return [...grouped.values()]
    .map((section) => {
      const providers = section.providers.sort((a, b) => a.sortKey.localeCompare(b.sortKey));
      const summary = summarizeProviderSection(providers);
      return {
        ...section,
        providers,
        summary,
        description: providerSectionDescription(summary),
      };
    })
    .sort((a, b) => a.title.localeCompare(b.title, "zh-Hans-CN"));
});
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

async function save(payload: ProviderInput) {
  try {
    if (editTarget.value) {
      await api.providers.update(editTarget.value.id, payload);
    } else {
      const created = await api.providers.create(payload);
      // Fire-and-forget: auto-refresh model list after creation so cards show model counts immediately
      api.providers.refreshModels(created.id).catch(() => {});
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
      auth_ref: p.auth_ref,
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

function onIntakeImported(ev?: Event) {
  const detail = (ev as CustomEvent<{ providerIds?: string[] }> | undefined)?.detail;
  const providerIds = detail?.providerIds ?? [];
  void load().then(async () => {
    if (!providerIds.length) return;
    await nextTick();
    highlightedProviderId.value = providerIds[0] ?? null;
    const el = document.querySelector(
      `[data-provider-id="${providerIds[0]}"]`,
    ) as HTMLElement | null;
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
    window.setTimeout(() => {
      if (highlightedProviderId.value === providerIds[0]) highlightedProviderId.value = null;
    }, 2200);
  });
}

watch(showForm, async (open) => {
  if (!open) return;
  if (editTarget.value) await reloadProviderCreds(editTarget.value.id);
});

onMounted(() => {
  void loadAndScrollToTargetProvider();
  window.addEventListener(INTAKE_FLOW_IMPORTED_EVENT, onIntakeImported);
  intakeFlow.bindShyClipboardOnFocus();
  void intakeFlow.tryShyClipboard();
});
onUnmounted(() => {
  if (providersOverviewFallbackTimer) {
    clearTimeout(providersOverviewFallbackTimer);
    providersOverviewFallbackTimer = null;
  }
  window.removeEventListener(INTAKE_FLOW_IMPORTED_EVENT, onIntakeImported);
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
    codexPlanRowsByProvider.value = {};
    planSnapByCred.value = {};
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
    codexPlanRowsByProvider.value = {
      ...codexPlanRowsByProvider.value,
      [e.provider_id]: rows,
    };
    const nextSnaps = { ...planSnapByCred.value };
    for (const row of rows) nextSnaps[row.credential_id] = row.plan;
    planSnapByCred.value = nextSnaps;
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
    <div class="relative mb-4 rounded-xl border border-slate-200/90 bg-vp-surface shadow-sm">
      <div
        class="relative z-10 flex flex-col gap-3 p-4 sm:flex-row sm:items-start sm:justify-between"
      >
        <div class="min-w-0 flex-1">
          <span :class="['text-xs uppercase', pageAccent.kicker]">Gateway</span>
          <h1 :class="['text-2xl font-bold tracking-tight', pageAccent.heading]">Providers</h1>
        </div>
        <div class="flex w-full shrink-0 flex-wrap items-center justify-end gap-2 sm:w-auto">
          <button
            type="button"
            class="btn-ghost flex min-h-11 items-center justify-center gap-2 px-3 py-2 text-sm rounded-lg border border-vp-border/80 sm:py-1.5"
            title="本地导入"
            aria-label="本地导入"
            @click="showImportModal = true"
          >
            <VpIcon name="folder-input" size-class="size-4 shrink-0" />
            <span class="hidden sm:inline">导入</span>
          </button>
          <button
            type="button"
            :class="[
              'flex min-h-11 min-w-11 items-center justify-center gap-2 px-3 py-2 sm:py-1.5 rounded-lg text-sm font-medium',
              pageAccent.btnPrimary,
            ]"
            aria-label="provider:add"
            title="provider:add"
            @click="startAdd"
          >
            <VpIcon name="plus" size-class="size-4 shrink-0 text-white" />
            <span class="sr-only">provider:add</span>
          </button>
        </div>
      </div>
    </div>

    <div
      v-if="error"
      class="mb-4 text-sm text-red-700 bg-red-50 border border-red-200 rounded-lg px-4 py-2"
    >
      {{ error }}
    </div>

    <div v-if="loading" class="text-slate-500 text-sm">...</div>
    <div
      v-else-if="providers.length === 0"
      class="font-mono text-slate-500 text-sm py-12 text-center"
      title="empty"
      aria-label="empty"
    >
      ∅
    </div>
    <div v-else class="space-y-3">
      <div v-for="section in providerSections" :key="section.key" class="space-y-2.5">
        <div class="rounded-lg border border-slate-200 bg-white px-3 py-2 shadow-sm">
          <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
            <div class="min-w-0 flex-1">
              <div class="flex min-w-0 flex-wrap items-center gap-2">
                <span :class="['i-lucide-layers-3', 'size-4 text-slate-500']" aria-hidden="true" />
                <h2 class="text-sm font-semibold text-slate-900">{{ section.title }}</h2>
                <span
                  class="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-[11px] text-slate-600"
                >
                  {{ section.summary.enabledEndpoints }}/{{ section.summary.totalEndpoints }}
                  endpoints
                </span>
                <span
                  v-if="section.summary.activeRequests"
                  class="rounded-full border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-[11px] text-emerald-700"
                >
                  live {{ section.summary.activeRequests }}
                </span>
                <span
                  v-if="section.summary.blockedCredentials"
                  class="rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-[11px] text-amber-800"
                >
                  {{ section.summary.blockedCredentials }} blocked creds
                </span>
              </div>
              <div
                class="mt-2 grid grid-cols-2 gap-1.5 text-[11px] text-slate-500 sm:grid-cols-3 lg:grid-cols-4"
              >
                <span class="rounded-md bg-slate-50 px-2 py-1"
                  >{{ section.summary.availableCredentials }}/{{
                    section.summary.enabledCredentials
                  }}
                  credentials</span
                >
                <span class="rounded-md bg-slate-50 px-2 py-1">{{
                  section.summary.fastestLatencyMs == null
                    ? "no speed"
                    : `${Math.round(section.summary.fastestLatencyMs)}ms best`
                }}</span>
                <span class="rounded-md bg-slate-50 px-2 py-1"
                  >{{ section.summary.remoteModels }} models</span
                >
                <span class="rounded-md bg-slate-50 px-2 py-1"
                  >{{ section.summary.nativeEndpoints }} native /
                  {{ section.summary.bridgedEndpoints }} bridge</span
                >
              </div>
            </div>
            <div class="flex shrink-0 flex-wrap items-center gap-2">
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-md border border-slate-200 bg-white px-2 py-1 text-[11px] font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50"
                :disabled="sectionSpeedtestBusy(section)"
                @click="speedtestProviders(providerIdsFromSection(section))"
              >
                <VpIcon
                  name="activity"
                  size-class="size-3.5"
                  :spin="sectionSpeedtestBusy(section)"
                />
                Test group
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2 py-1 text-[11px] font-medium text-emerald-900 hover:bg-emerald-100 disabled:opacity-50"
                :disabled="sectionModelRefreshBusy(section)"
                @click="refreshProviderModelsForProviders(providerIdsFromSection(section))"
              >
                <VpIcon
                  name="book-open"
                  size-class="size-3.5"
                  :spin="sectionModelRefreshBusy(section)"
                />
                Refresh models
              </button>
            </div>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-2 xl:grid-cols-2">
          <div
            v-for="card in section.providers"
            :key="card.provider.id"
            :data-provider-id="card.provider.id"
          >
            <ProviderCard
              :id="`provider-${card.provider.id}`"
              :card="card"
              :health="healthMap[card.provider.id]"
              :creds="credsByProvider[card.provider.id] ?? []"
              :loading-creds="!!loadingCreds[card.provider.id]"
              :toggle-provider-busy="!!toggleBusy[card.provider.id]"
              :circuit-reset-busy="!!circuitResetBusy[card.provider.id]"
              :speedtest-busy="!!speedtestBusy[card.provider.id]"
              :model-refresh-busy="!!modelRefreshBusy[card.provider.id]"
              :cred-model-refresh-busy="credModelRefreshBusy"
              :cred-balance-refresh-busy="credBalanceRefreshBusy"
              :cred-toggle-busy="credToggleBusy"
              :pool-rows="poolByProviderId[card.provider.id]?.credentials ?? []"
              :plan-snap-by-cred="planSnapByCred"
              :active-credential-counts="activeCredentialCountsByProvider[card.provider.id] ?? {}"
              :active-request-count="activeRequestCountsByProvider.get(card.provider.id) ?? 0"
              :tokens-per-sec="
                liveTokensPerSecByProvider.get(card.provider.id) ||
                providerRollingStatById.get(card.provider.id)?.decode_output_tokens_per_sec ||
                providerRollingStatById.get(card.provider.id)?.output_tokens_per_sec
              "
              :class="[
                highlightedProviderId === card.provider.id
                  ? 'ring-2 ring-sky-300 ring-offset-2 ring-offset-vp-bg'
                  : '',
              ]"
              @sync-creds="reloadProviderCreds($event)"
              @speedtest-provider="speedtestProvider($event)"
              @refresh-models="refreshProviderModels($event)"
              @refresh-cred-models="refreshCredentialModels($event)"
              @refresh-cred-balance="refreshCredentialBalance($event)"
              @toggle-provider="toggleProviderEnabled($event)"
              @reset-circuit="resetProviderCircuit($event)"
              @edit-provider="startEdit($event)"
              @delete-provider="remove($event)"
              @add-cred="startAddCred($event)"
              @toggle-cred="toggleCredentialEnabled($event)"
              @edit-cred="startEditCred($event)"
              @delete-cred="removeCred($event)"
              @view-logs="viewProviderLogs($event)"
            />
          </div>
        </div>
      </div>
    </div>

    <ProviderSmartModal
      :open="showForm"
      :edit-target="editTarget"
      :creds="editTarget ? (credsByProvider[editTarget.id] ?? []) : []"
      :loading-creds="!!(editTarget && loadingCreds[editTarget.id])"
      :cred-toggle-busy="credToggleBusy"
      :model-refresh-busy="!!(editTarget && modelRefreshBusy[editTarget.id])"
      :speed-label="editProviderSpeedLabel"
      @close="showForm = false"
      @save="save($event)"
      @refresh-models="editTarget && refreshProviderModels(editTarget.id)"
      @add-credential="editTarget && startAddCred(editTarget.id)"
      @reload-creds="editTarget && reloadProviderCreds(editTarget.id)"
      @edit-credential="startEditCred($event)"
      @remove-credential="removeCred($event)"
      @toggle-credential="toggleCredentialEnabled($event)"
    />

    <ProviderImportModal
      :open="showImportModal"
      @close="showImportModal = false"
      @imported="load()"
    />

    <Teleport to="body">
      <div
        v-if="showCredForm"
        class="vp-modal-backdrop z-[110] overflow-y-auto py-6"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cred-form-title"
        @click.self="showCredForm = false"
      >
        <div class="vp-modal-panel max-w-lg flex flex-col my-auto" @click.stop>
          <div class="vp-modal-header">
            <span
              class="grid size-10 shrink-0 place-items-center rounded-xl bg-violet-100 text-violet-700 ring-1 ring-violet-200"
              aria-hidden="true"
            >
              <VpIcon name="key" size-class="size-5" />
            </span>
            <div class="min-w-0 flex-1">
              <h2 id="cred-form-title" class="font-semibold text-lg text-vp-text">
                credential.{{ editCred ? "edit" : "add" }}
              </h2>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="close"
              title="close"
              @click="showCredForm = false"
            >
              <VpIcon name="x" size-class="size-5" />
            </button>
          </div>
          <div class="px-6 py-4 space-y-3 overflow-y-auto max-h-[min(36rem,72vh)]">
            <label class="block">
              <span class="sr-only">label</span>
              <input
                v-model="credForm.label"
                placeholder="label"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900 focus:outline-none focus:border-violet-500"
              />
            </label>

            <!-- Auth mode selector -->
            <div class="flex gap-2">
              <button
                @click="credAuthMode = 'apikey'"
                :class="
                  credAuthMode === 'apikey'
                    ? 'bg-violet-600 text-white'
                    : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
                "
                class="flex-1 py-1.5 text-xs rounded-md transition-colors"
              >
                auth_ref
              </button>
              <button
                @click="credAuthMode = 'oauth'"
                :class="
                  credAuthMode === 'oauth'
                    ? 'bg-violet-600 text-white'
                    : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
                "
                class="flex-1 py-1.5 text-xs rounded-md transition-colors"
              >
                OAuth
              </button>
            </div>

            <!-- API Key mode -->
            <template v-if="credAuthMode === 'apikey'">
              <label class="block">
                <span class="sr-only">auth_ref</span>
                <input
                  v-model="credForm.auth_ref"
                  placeholder="sk-… paste directly (advanced: env:MY_KEY / keyring:name)"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
                />
                <p class="mt-1 text-[11px] text-vp-muted font-mono">
                  Raw sk-/ck-/dk-* values are automatically wrapped with
                  <code>literal:</code> before storing in SQLite.
                </p>
              </label>
            </template>

            <!-- OAuth mode -->
            <template v-else>
              <input
                ref="authJsonFileInputRef"
                type="file"
                accept=".json,application/json"
                class="sr-only"
                @change="onAuthJsonFileChange"
              />
              <!-- Paste or drop Codex auth*.json (same shape as local import) -->
              <div
                class="rounded-lg border border-dashed border-violet-200 p-3 space-y-2 bg-violet-50/80 transition-colors"
                :class="
                  authJsonDragActive
                    ? 'border-violet-500 ring-2 ring-violet-400/50 bg-violet-100'
                    : 'border-violet-200'
                "
                @dragover="onAuthJsonDragOver"
                @dragleave="onAuthJsonDragLeave"
                @drop="onAuthJsonDrop"
              >
                <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-2">
                  <p class="text-xs text-slate-800 font-medium">
                    <code class="font-mono text-slate-600">auth*.json</code>
                  </p>
                  <button
                    type="button"
                    class="shrink-0 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-white border border-slate-200 hover:bg-slate-50 text-slate-800 transition-colors w-full sm:w-auto"
                    aria-label="file:pick"
                    title="file:pick"
                    @click="triggerAuthJsonFilePick"
                  >
                    <VpIcon name="folder-input" size-class="size-4" />
                    <span class="sr-only">file:pick</span>
                  </button>
                </div>
                <textarea
                  v-model="authJsonPaste"
                  rows="5"
                  placeholder='{"auth_mode":"chatgpt","tokens":{"access_token":"eyJ…","refresh_token":"…"}}'
                  class="w-full min-h-[7rem] bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900 resize-y"
                />
                <p v-if="authJsonPasteErr" class="text-xs text-red-600">{{ authJsonPasteErr }}</p>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-if="editTarget"
                    type="button"
                    class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-900 hover:bg-emerald-100"
                    @click="refreshProviderModels(editTarget.id)"
                  >
                    <VpIcon name="refresh-cw" size-class="size-3.5" />
                    Fetch remote models
                  </button>
                  <button
                    type="button"
                    :disabled="!authJsonPaste.trim()"
                    class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-violet-600 hover:bg-violet-700 text-white disabled:opacity-40 transition-colors"
                    aria-label="json:parse"
                    @click="parseAuthJsonPaste"
                  >
                    <VpIcon name="zap" size-class="size-4 text-white" />
                    <span class="sr-only">parse</span>
                  </button>
                </div>
              </div>
              <label class="block">
                <span class="sr-only">access_token</span>
                <input
                  v-model="credForm.oauth_access_token"
                  placeholder="eyJhbGciOiJSUzI1NiJ9…"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                />
              </label>
              <label class="block">
                <span class="sr-only">refresh_token</span>
                <input
                  v-model="credForm.oauth_refresh_token"
                  placeholder="refresh_token"
                  type="password"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                />
              </label>
              <p class="font-mono text-xs text-slate-600">
                exp
                {{
                  credForm.oauth_expires_at
                    ? new Date(credForm.oauth_expires_at * 1000).toLocaleString()
                    : "unknown"
                }}
              </p>
            </template>

            <label class="block">
              <span class="sr-only">plan_type</span>
              <input
                v-model="credForm.plan_type"
                placeholder="claude-pro · codex-plus · codex-pro · payg · …"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="block">
              <span class="sr-only">priority</span>
              <input
                v-model.number="credForm.priority"
                type="number"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="block">
              <span class="sr-only">notes</span>
              <input
                v-model="credForm.notes"
                placeholder="notes"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input v-model="credForm.enabled" type="checkbox" class="rounded" />
              <span class="sr-only">enabled</span>
            </label>
          </div>
          <div
            class="flex gap-3 px-6 py-4 border-t border-vp-border justify-end bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
          >
            <button
              type="button"
              class="btn-ghost flex items-center gap-2 !px-3"
              aria-label="cancel"
              @click="showCredForm = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span class="sr-only">cancel</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors"
              aria-label="credential:save"
              @click="saveCred"
            >
              <VpIcon name="check" size-class="size-4 text-white" />
              <span class="sr-only">save</span>
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

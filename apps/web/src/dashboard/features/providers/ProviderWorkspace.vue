<script setup lang="ts">
import { ref, computed, onMounted, watch, nextTick } from "vue";
import { useI18n } from "vue-i18n";
import {
  api,
  type CredentialPlanSnapshot,
  type Provider,
  type ProviderCodexPlanItem,
  type ProviderInput,
  type ProviderHealthSummary,
  type ProviderAuthPoolSummary,
  type Credential,
  type CredentialInput,
  type ProvidersOverview,
  type UpstreamGroupInfo,
  isProviderHealthSummary,
} from "../../api/client.ts";
import { CLIENT_TOOLS, type ClientToolId, type ClientToolInfo } from "../../utils/client-tools.ts";
import { formatApiError } from "../../utils/api-error.ts";
import type { ProviderSectionView } from "./types.ts";
import { useRoute, useRouter } from "vue-router";
import { hintsFromAuthJsonTokens } from "../../utils/codex-oauth-hints.ts";
import VpIcon from "../../components/vp-icon.vue";
import UiButton from "../../components/ui/button.vue";
import UiSkeleton from "../../components/ui/skeleton.vue";
import ProviderSections from "./components/ProviderSections.vue";
import ProviderSmartModal from "./components/provider-smart-modal.vue";
import ProviderImportModal from "./components/provider-import-modal.vue";
import CredentialFormModal from "./components/CredentialFormModal.vue";

import {
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../../utils/workspace-view.ts";
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
const { t } = useI18n();
const workspaceView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
/** Hours for provider health and quota snapshots (not request body logging). */
const GATEWAY_ROLLING_STAT_HOURS = 24;
const loading = ref(true);
const error = ref("");

const highlightedProviderId = ref<string | null>(null);
/** Inline enable/disable debounce state (PUT /_vp/providers/:id). */
const toggleBusy = ref<Record<string, boolean>>({});
/** Per-provider manual circuit reset busy state (POST /_vp/providers/:id/circuit/reset). */
const circuitResetBusy = ref<Record<string, boolean>>({});
/** Per-provider remote model refresh busy state. */
const modelRefreshBusy = ref<Record<string, boolean>>({});
const credModelRefreshBusy = ref<Record<string, boolean>>({});
const credBalanceRefreshBusy = ref<Record<string, boolean>>({});
/** Per-credential enable/disable busy state (PUT /_vp/credentials/:id). */
const credToggleBusy = ref<Record<string, boolean>>({});
const activeProviderTab = ref<"common" | ClientToolId>("common");

// Provider form
const showForm = ref(false);
const editTarget = ref<Provider | null>(null);

// Import modal
const showImportModal = ref(false);

const editProviderLive = computed(() => {
  if (!editTarget.value) return null;
  return providers.value.find((x) => x.id === editTarget.value?.id) ?? editTarget.value;
});

const editProviderSpeedLabel = computed(() => {
  const result = editProviderLive.value?.last_speedtest;
  if (!result) return t("speed.notTested");
  if (result.error) return result.error;
  return result.latency_ms == null ? t("speed.tested") : `${result.latency_ms}ms`;
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

function toUiError(err: unknown) {
  return formatApiError(err, t);
}

function resetAuthJsonImportUi() {
  authJsonPaste.value = "";
  authJsonPasteErr.value = "";
  authJsonDragActive.value = false;
}

type OauthTriple = {
  access: string;
  refresh: string | null;
  exp: number | null;
};

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
        authJsonPasteErr.value = t("authJson.requiresAccessToken");
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
      authJsonPasteErr.value = t("authJson.unrecognized");
      return;
    }

    authJsonPasteErr.value = t("authJson.unknownMode", { mode });
  } catch (e: unknown) {
    authJsonPasteErr.value = e instanceof Error ? e.message : toUiError(e);
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
    authJsonPasteErr.value = t("authJson.readFileFailed");
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
    authJsonPasteErr.value = t("authJson.dropSingleJson");
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
    try {
      const cached = sessionStorage.getItem("vp-providers-overview-cache");
      if (cached) {
        applyProvidersOverview(JSON.parse(cached) as ProvidersOverview);
        loading.value = false;
      }
    } catch {}
    // The websocket snapshot endpoint was removed with the monitor subsystem.
    // Do not wait for a 3s fallback: Providers first paint must be a direct DB snapshot.
    const overview = await api.providers.overview(GATEWAY_ROLLING_STAT_HOURS);
    if (!applyProvidersOverview(overview)) return;
    try {
      sessionStorage.setItem("vp-providers-overview-cache", JSON.stringify(overview));
    } catch {}
  } catch (e) {
    error.value = toUiError(e);
  } finally {
    loading.value = false;
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

const { planSnapByCred, codexPlanRowsByProvider } = useProviderCodexPlans(providers, {
  loadCreds,
  refreshSinglePool,
});

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
    error.value = toUiError(e);
  } finally {
    const { [providerId]: _, ...rest } = modelRefreshBusy.value;
    modelRefreshBusy.value = rest;
  }
}

async function refreshCredentialModels(credentialId: string) {
  if (credModelRefreshBusy.value[credentialId]) return;
  credModelRefreshBusy.value = {
    ...credModelRefreshBusy.value,
    [credentialId]: true,
  };
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
    error.value = toUiError(e);
  } finally {
    const { [credentialId]: _, ...rest } = credModelRefreshBusy.value;
    credModelRefreshBusy.value = rest;
  }
}

async function refreshCredentialBalance(credentialId: string) {
  if (credBalanceRefreshBusy.value[credentialId]) return;
  credBalanceRefreshBusy.value = {
    ...credBalanceRefreshBusy.value,
    [credentialId]: true,
  };
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
    error.value = toUiError(e);
  } finally {
    const { [credentialId]: _, ...rest } = credBalanceRefreshBusy.value;
    credBalanceRefreshBusy.value = rest;
  }
}

const activeToolTab = computed<ClientToolInfo | null>(() => {
  if (activeProviderTab.value === "common") return null;
  return CLIENT_TOOLS.find((tool) => tool.id === activeProviderTab.value) ?? null;
});

const scopedProviders = computed(() =>
  providers.value.filter((provider) => providerMatchesWorkspaceView(provider, workspaceView.value)),
);

const scopedCredsByProvider = computed(() => {
  const providerIds = new Set(scopedProviders.value.map((provider) => provider.id));
  return Object.fromEntries(
    Object.entries(credsByProvider.value).filter(([providerId]) => providerIds.has(providerId)),
  );
});

const providerSections = computed<ProviderSectionView[]>(() =>
  buildProviderSections({
    providers: scopedProviders.value,
    selectedTool: activeToolTab.value,
    healthMap: healthMap.value,
    poolByProviderId: poolByProviderId.value,
    text: {
      bridge: t("section.bridge"),
      credentialShort: t("section.credentialShort"),
      models: t("section.models"),
      native: t("section.native"),
      noCredential: t("section.noCredential"),
      notTested: t("section.notTested"),
      fastest: (ms) => t("section.fastest", { ms }),
      success: (pct) => t("section.success", { pct }),
      successUnknown: t("section.successUnknown"),
      first: (ms) => t("section.first", { ms }),
      tokensPerSecond: (value) => t("section.tokensPerSecond", { value }),
    },
    fallbackGroupName: t("groups.ungrouped"),
  }),
);
const providerRollingStatById = computed(() => {
  const map = new Map<string, NonNullable<ProviderHealthSummary["rolling"]>>();
  for (const [providerId, health] of Object.entries(healthMap.value)) {
    if (health.rolling) map.set(providerId, health.rolling);
  }
  return map;
});
const activeCredentialCountsByProvider = computed(() => ({}));

function targetProviderIdFromRoute(): string | null {
  const raw = route.query.provider;
  if (Array.isArray(raw)) return raw[0] ?? null;
  return raw ?? null;
}

function providerDomId(providerId: string): string {
  return `provider-${providerId}`;
}

async function clearProviderQuery(providerId: string) {
  if (targetProviderIdFromRoute() !== providerId) return;
  const query = { ...route.query };
  delete query.provider;
  await router.replace({ path: route.path, query, hash: route.hash });
}

async function scrollToTargetProvider() {
  const providerId = targetProviderIdFromRoute();
  if (!providerId) return;
  await nextTick();
  const el = document.getElementById(providerDomId(providerId));
  if (!el) return;
  highlightedProviderId.value = providerId;
  el.scrollIntoView({ block: "center", behavior: "smooth" });
  void clearProviderQuery(providerId);
  window.setTimeout(() => {
    if (highlightedProviderId.value === providerId) highlightedProviderId.value = null;
  }, 2200);
}

async function loadAndScrollToTargetProvider() {
  try {
    await load();
    await scrollToTargetProvider();
  } catch (e) {
    error.value = toUiError(e);
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

async function saveCredentialOnly(providerId: string, credentialAuthRef: string) {
  try {
    await api.credentials.create(providerId, {
      ...emptyCredForm(),
      label: t("credentials.defaultApiKeyLabel"),
      auth_ref: normalizeAuthRef(credentialAuthRef.trim()),
      notes: t("credentials.createdFromWizard"),
    });
    showForm.value = false;
    await load();
  } catch (e) {
    error.value = toUiError(e);
  }
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
        label: t("credentials.defaultApiKeyLabel"),
        auth_ref: normalizeAuthRef(credentialAuthRef.trim()),
        notes: t("credentials.createdFromWizard"),
      });
    }
    showForm.value = false;
    await load();
  } catch (e) {
    error.value = toUiError(e);
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
      avatar_url: p.avatar_url ?? null,
      kind: p.kind,
      base_url: p.base_url,
      host: p.host ?? null,
      protocols: [...(p.protocols ?? [])],
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
    error.value = toUiError(e);
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
    error.value = toUiError(e);
  } finally {
    const { [providerId]: _, ...rest } = circuitResetBusy.value;
    circuitResetBusy.value = rest;
  }
}

async function remove(id: string) {
  if (!confirm(t("confirm.removeProvider"))) return;
  try {
    await api.providers.delete(id);
    await load();
  } catch (e) {
    error.value = toUiError(e);
  }
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
    const res = await api.credentials.login(editCred.value.id, {
      username,
      password,
    });
    credLoginNote.value = res.ok ? t("login.success") : (res.note ?? t("login.failed"));
    if (res.ok) credLoginPassword.value = "";
  } catch (e) {
    credLoginNote.value = toUiError(e);
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
    error.value = toUiError(e);
  }
}

async function removeCred(cred: Credential) {
  if (!confirm(t("confirm.removeCredential", { label: cred.label }))) return;
  try {
    await api.credentials.delete(cred.id);
    await loadCreds(cred.provider_id);
    await refreshSinglePool(cred.provider_id);
  } catch (e) {
    error.value = toUiError(e);
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
    error.value = toUiError(e);
  } finally {
    const { [cred.id]: _, ...rest } = credToggleBusy.value;
    credToggleBusy.value = rest;
  }
}

watch(showForm, async (open) => {
  if (!open) return;
  if (editTarget.value) await reloadProviderCreds(editTarget.value.id);
});

onMounted(() => {
  void loadAndScrollToTargetProvider();
});
watch(
  () => route.query.provider,
  () => {
    if (loading.value) return;
    void scrollToTargetProvider().catch((e) => {
      error.value = toUiError(e);
    });
  },
  { flush: "post" },
);
</script>

<template>
  <div class="mx-auto w-full max-w-[1040px]">
    <div class="mb-4 flex flex-wrap items-center justify-end gap-2">
      <UiButton
        type="button"
        variant="outline"
        class="min-h-11 sm:min-h-9"
        :title="t('actions.localImport')"
        :aria-label="t('actions.localImport')"
        @click="showImportModal = true"
      >
        <VpIcon name="folder-input" size-class="size-4 shrink-0" />
        <span class="hidden sm:inline">{{ t("actions.import") }}</span>
      </UiButton>
      <UiButton
        type="button"
        class="min-h-11 min-w-11 sm:min-h-9"
        :aria-label="t('actions.addProvider')"
        :title="t('actions.addProvider')"
        @click="startAdd"
      >
        <VpIcon name="plus" size-class="size-4 shrink-0 text-white" />
        <span class="sr-only">{{ t("actions.addProvider") }}</span>
      </UiButton>
    </div>

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
      v-else-if="scopedProviders.length === 0"
      class="rounded-xl border border-dashed border-border bg-card py-12 text-center font-mono text-sm text-muted-foreground"
      :title="t('states.empty')"
      :aria-label="t('states.empty')"
    >
      ∅
    </div>
    <ProviderSections
      v-else
      data-testid="providers-complete"
      :data-provider-count="scopedProviders.length"
      :data-credential-count="
        Object.values(scopedCredsByProvider).reduce((sum, rows) => sum + rows.length, 0)
      "
      :sections="providerSections"
      :health-map="healthMap"
      :creds-by-provider="scopedCredsByProvider"
      :loading-creds="loadingCreds"
      :toggle-busy="toggleBusy"
      :circuit-reset-busy="circuitResetBusy"
      :cred-model-refresh-busy="credModelRefreshBusy"
      :cred-balance-refresh-busy="credBalanceRefreshBusy"
      :cred-toggle-busy="credToggleBusy"
      :pool-by-provider-id="poolByProviderId"
      :plan-snap-by-cred="planSnapByCred"
      :active-credential-counts-by-provider="activeCredentialCountsByProvider"
      :provider-rolling-stat-by-id="providerRollingStatById"
      :highlighted-provider-id="highlightedProviderId"
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
    />

    <ProviderSmartModal
      :open="showForm"
      :edit-target="editTarget"
      :existing-providers="providers"
      :creds="editTarget ? (credsByProvider[editTarget.id] ?? []) : []"
      :loading-creds="!!(editTarget && loadingCreds[editTarget.id])"
      :cred-toggle-busy="credToggleBusy"
      :model-refresh-busy="!!(editTarget && modelRefreshBusy[editTarget.id])"
      :speed-label="editProviderSpeedLabel"
      @close="showForm = false"
      @save="(form, credKey) => save(form, credKey)"
      @save-credential-only="saveCredentialOnly"
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

<i18n lang="json">
{
  "en": {
    "actions": {
      "addProvider": "Add provider",
      "import": "Import",
      "localImport": "Local import"
    },
    "errors": {
      "vendorUnknown": "Could not detect provider type for {providerId} (no NewAPI/Sub2API signature)."
    },
    "login": {
      "failed": "Login failed",
      "success": "Login successful"
    },
    "confirm": {
      "removeCredential": "Remove credential \"{label}\"?",
      "removeProvider": "Remove this provider?"
    },
    "credentials": {
      "createdFromWizard": "Created from provider wizard paste",
      "defaultApiKeyLabel": "API Key"
    },
    "page": {
      "kicker": "Gateway",
      "title": "Providers"
    },
    "states": {
      "empty": "empty"
    },
    "authJson": {
      "dropSingleJson": "Drop a single .json file.",
      "readFileFailed": "Could not read file.",
      "requiresAccessToken": "ChatGPT OAuth requires tokens.access_token in JSON.",
      "unknownMode": "Unknown auth_mode \"{mode}\".",
      "unrecognized": "Unrecognized JSON: need OPENAI_API_KEY, or tokens.access_token, or auth_mode \"chatgpt\"."
    },
    "groups": { "all": "all", "common": "Common", "ungrouped": "Ungrouped" },
    "section": {
      "bridge": "bridge",
      "credentialShort": "cred",
      "fastest": "{ms}ms best",
      "first": "{ms}ms first",
      "models": "models",
      "native": "native",
      "noCredential": "no cred",
      "notTested": "not tested",
      "success": "{pct}% ok",
      "successUnknown": "no traffic",
      "tokensPerSecond": "{value} tok/s"
    },
    "speed": { "notTested": "Not tested", "tested": "Tested" }
  },
  "zh-CN": {
    "actions": {
      "addProvider": "添加供应商",
      "import": "导入",
      "localImport": "本地导入"
    },
    "errors": {
      "vendorUnknown": "未能识别 {providerId} 的供应商类型（无 NewAPI/Sub2API 特征）。"
    },
    "login": {
      "failed": "登录失败",
      "success": "登录成功"
    },
    "confirm": {
      "removeCredential": "移除凭证「{label}」？",
      "removeProvider": "移除此供应商？"
    },
    "credentials": {
      "createdFromWizard": "由供应商向导粘贴创建",
      "defaultApiKeyLabel": "API Key"
    },
    "page": {
      "kicker": "网关",
      "title": "供应商"
    },
    "states": {
      "empty": "空状态"
    },
    "authJson": {
      "dropSingleJson": "请只拖入一个 .json 文件。",
      "readFileFailed": "无法读取文件。",
      "requiresAccessToken": "ChatGPT OAuth 需要 JSON 中包含 tokens.access_token。",
      "unknownMode": "未知 auth_mode \"{mode}\"。",
      "unrecognized": "无法识别 JSON：需要 OPENAI_API_KEY、tokens.access_token，或 auth_mode \"chatgpt\"。"
    },
    "groups": { "all": "全部", "common": "通用", "ungrouped": "未分组" },
    "section": {
      "bridge": "桥接",
      "credentialShort": "凭证",
      "fastest": "最快 {ms}ms",
      "first": "首响 {ms}ms",
      "models": "模型",
      "native": "原生",
      "noCredential": "无凭证",
      "notTested": "未测试",
      "success": "成功率 {pct}%",
      "successUnknown": "暂无流量",
      "tokensPerSecond": "{value} tok/s"
    },
    "speed": { "notTested": "未测试", "tested": "已测试" }
  }
}
</i18n>

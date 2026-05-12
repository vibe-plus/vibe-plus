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
  type LocalCandidate,
  isProviderHealthSummary,
} from "../api/client.ts";
import {
  CLIENT_TOOLS,
  getCodexClientTool,
  getToolProtocolSupport,
  providerServesCodexCliRoute,
  type ClientToolId,
  type ClientToolInfo,
  type ProtocolSupportInfo,
} from "../utils/client-tools.ts";
import { useRoute } from "vue-router";
import { resolvePageAccent } from "../utils/page-accent.ts";
import { mapUpstreamUserMessage, displayProviderName } from "../utils/providers-display.ts";
import { hintsFromAuthJsonTokens } from "../utils/codex-oauth-hints.ts";
import VpIcon from "../components/vp-icon.vue";
import ProviderCard from "../components/provider-card.vue";
import { workspaceViewFromQuery, type WorkspaceView } from "../utils/workspace-view.ts";

// ---------------------------------------------------------------------------
// Provider presets
// ---------------------------------------------------------------------------
interface Preset {
  label: string;
  icon: string;
  name: string;
  kind: ProviderInput["kind"];
  base_url: string;
  auth_ref_hint: string;
  priority: number;
  aliases: { alias: string; upstream_model: string }[];
}

const PRESETS: Preset[] = [
  {
    label: "OpenAI / Codex",
    icon: "i-lucide-bot",
    name: "OpenAI",
    kind: "openai-responses",
    base_url: "https://api.openai.com",
    auth_ref_hint: "env:OPENAI_API_KEY  or  keyring:…",
    priority: 10,
    // Align with Codex CLI model slugs; the authoritative set is from GET /codex/v1/models (or upstream) after takeover.
    aliases: [
      { alias: "gpt-5.3-codex", upstream_model: "gpt-5.3-codex" },
      { alias: "gpt-5.4", upstream_model: "gpt-5.4" },
      { alias: "gpt-5.1-codex-max", upstream_model: "gpt-5.1-codex-max" },
      { alias: "gpt-5.1-codex-mini", upstream_model: "gpt-5.1-codex-mini" },
    ],
  },
  {
    label: "Anthropic",
    icon: "i-lucide-sparkles",
    name: "Anthropic",
    kind: "anthropic",
    base_url: "https://api.anthropic.com",
    auth_ref_hint: "env:ANTHROPIC_API_KEY  or  keyring:…",
    priority: 10,
    aliases: [
      { alias: "claude-opus-4-7", upstream_model: "claude-opus-4-7-20251101" },
      { alias: "claude-sonnet-4-5", upstream_model: "claude-sonnet-4-5-20251001" },
      { alias: "claude-haiku-4-5", upstream_model: "claude-haiku-4-5-20251001" },
    ],
  },
  {
    label: "DeepSeek",
    icon: "i-lucide-brain",
    name: "DeepSeek",
    kind: "openai-chat",
    base_url: "https://api.deepseek.com",
    auth_ref_hint: "env:DEEPSEEK_API_KEY",
    priority: 200,
    aliases: [
      { alias: "deepseek-chat", upstream_model: "deepseek-chat" },
      { alias: "deepseek-reasoner", upstream_model: "deepseek-reasoner" },
    ],
  },
  {
    label: "Qwen (Alibaba)",
    icon: "i-lucide-cloud",
    name: "Qwen",
    kind: "openai-chat",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    auth_ref_hint: "env:DASHSCOPE_API_KEY",
    priority: 200,
    aliases: [
      { alias: "qwen-plus", upstream_model: "qwen-plus" },
      { alias: "qwen-turbo", upstream_model: "qwen-turbo" },
      { alias: "qwen-max", upstream_model: "qwen-max" },
    ],
  },
  {
    label: "Moonshot / Kimi",
    icon: "i-lucide-moon",
    name: "Moonshot",
    kind: "openai-chat",
    base_url: "https://api.moonshot.cn/v1",
    auth_ref_hint: "env:MOONSHOT_API_KEY",
    priority: 200,
    aliases: [
      { alias: "moonshot-v1-8k", upstream_model: "moonshot-v1-8k" },
      { alias: "moonshot-v1-32k", upstream_model: "moonshot-v1-32k" },
    ],
  },
  {
    label: "Zhipu / GLM",
    icon: "i-lucide-zap",
    name: "Zhipu",
    kind: "openai-chat",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    auth_ref_hint: "env:ZHIPU_API_KEY",
    priority: 200,
    aliases: [
      { alias: "glm-4-plus", upstream_model: "glm-4-plus" },
      { alias: "glm-4-flash", upstream_model: "glm-4-flash" },
    ],
  },
  {
    label: "Gemini",
    icon: "i-lucide-gem",
    name: "Google Gemini",
    kind: "gemini-native",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
    auth_ref_hint: "env:GEMINI_API_KEY",
    priority: 150,
    aliases: [
      { alias: "gemini-2.5-pro", upstream_model: "gemini-2.5-pro-preview-05-06" },
      { alias: "gemini-2.5-flash", upstream_model: "gemini-2.5-flash-preview-04-17" },
    ],
  },
];

function applyPreset(p: Preset) {
  form.value = {
    name: p.name,
    kind: p.kind,
    base_url: p.base_url,
    auth_ref: null,
    enabled: true,
    priority: p.priority,
    model_aliases: p.aliases.map((a) => ({ alias: a.alias, upstream_model: a.upstream_model })),
  };
  editTarget.value = null;
  showPresets.value = false;
  showForm.value = true;
}

const showPresets = ref(false);
const PRESET_MENU_W = 288;
const presetTriggerWrap = ref<HTMLElement | null>(null);
const presetPanelRef = ref<HTMLElement | null>(null);
const presetMenuPos = ref({ top: 0, left: 0 });

function measurePresetMenu() {
  const wrap = presetTriggerWrap.value;
  if (!wrap) return;
  const r = wrap.getBoundingClientRect();
  const left = Math.min(
    Math.max(8, r.right - PRESET_MENU_W),
    Math.max(8, window.innerWidth - PRESET_MENU_W - 8),
  );
  presetMenuPos.value = { top: r.bottom + 4, left };
}

function onPresetViewportChange() {
  if (showPresets.value) measurePresetMenu();
}

function onPresetGlobalPointerDown(ev: PointerEvent) {
  if (!showPresets.value) return;
  const n = ev.target as Node | null;
  if (!n) return;
  if (presetTriggerWrap.value?.contains(n)) return;
  if (presetPanelRef.value?.contains(n)) return;
  showPresets.value = false;
}

watch(showPresets, async (open) => {
  if (open) {
    document.addEventListener("pointerdown", onPresetGlobalPointerDown, true);
    await nextTick();
    measurePresetMenu();
  } else {
    document.removeEventListener("pointerdown", onPresetGlobalPointerDown, true);
  }
});

const providers = ref<Provider[]>([]);
const healthMap = ref<Record<string, ProviderHealthSummary>>({});
/** `GET /_vp/pools` — 凭证级熔断/限流摘要（与列表并行加载） */
const poolByProviderId = ref<Record<string, ProviderAuthPoolSummary>>({});
const route = useRoute();
const pageAccent = computed(() => resolvePageAccent(route.name));
const workspaceView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const codexRouteTool = computed(() => getCodexClientTool());
/** Hours for `GET /_vp/providers/:id/health?hours=` — gateway `request_logs` rollup only (not Codex plan windows). */
const GATEWAY_ROLLING_STAT_HOURS = 24;
const planSnapByCred = ref<Record<string, CredentialPlanSnapshot | null>>({});
/** Latest ChatGPT `wham/usage` or header snapshot per credential on official Codex providers. */
const codexPlanRowsByProvider = ref<Record<string, ProviderCodexPlanItem[]>>({});
const codexRefreshNote = ref<Record<string, string>>({});
/** True while POST …/codex-plan/refresh is in flight for that provider. */
const codexPlanRefreshing = ref<Record<string, boolean>>({});
const loading = ref(true);
const error = ref("");
/** Inline enable/disable debounce state (PUT /_vp/providers/:id). */
const toggleBusy = ref<Record<string, boolean>>({});
/** Per-provider manual circuit reset busy state (POST /_vp/providers/:id/circuit/reset). */
const circuitResetBusy = ref<Record<string, boolean>>({});
/** Per-credential enable/disable busy state (PUT /_vp/credentials/:id). */
const credToggleBusy = ref<Record<string, boolean>>({});
const activeProviderTab = ref<"common" | ClientToolId>("common");

// Provider form
const showForm = ref(false);
const editTarget = ref<Provider | null>(null);
const emptyForm = (): ProviderInput => ({
  name: "",
  kind: "anthropic",
  base_url: "https://api.anthropic.com",
  auth_ref: null,
  enabled: true,
  priority: 100,
  model_aliases: [
    { alias: "high", upstream_model: "claude-opus-4-7" },
    { alias: "low", upstream_model: "claude-haiku-4-5-20251001" },
  ],
});
const form = ref<ProviderInput>(emptyForm());

// Credential management（列表默认加载，见 load()）
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

// ---------------------------------------------------------------------------
// Local import (scan installed tools)
// ---------------------------------------------------------------------------
const localCandidates = ref<LocalCandidate[]>([]);
const showImport = ref(false);
const importLoading = ref(false);
const importError = ref("");
const importingClients = ref<Set<string>>(new Set());

async function openImport() {
  showImport.value = true;
  importLoading.value = true;
  importError.value = "";
  try {
    const candidates = await api.providers.scanLocal();
    localCandidates.value = candidates.map((candidate) => ({
      ...candidate,
      extra_credentials: candidate.extra_credentials ?? [],
    }));
  } catch (e) {
    importError.value = String(e);
  } finally {
    importLoading.value = false;
  }
}

async function doImport(client: string) {
  importingClients.value.add(client);
  try {
    await api.providers.importLocal([client]);
    showImport.value = false;
    await load();
  } catch (e) {
    importError.value = String(e);
  } finally {
    importingClients.value.delete(client);
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

async function loadCodexPlanRowsAll() {
  await Promise.all(
    providers.value.filter(isOfficialCodexProvider).map((p) => loadCodexPlanRowsForProvider(p.id)),
  );
}

async function refreshCodexPlanFromChatgpt(providerId: string, opts?: { silent?: boolean }) {
  if (!opts?.silent) {
    codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: "" };
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
    await loadCreds(providerId);
    await refreshSinglePool(providerId);
  } catch (e) {
    codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: String(e) };
  } finally {
    codexPlanRefreshing.value = { ...codexPlanRefreshing.value, [providerId]: false };
  }
}

/** After list load: pull ChatGPT wham/usage once per provider (sequential to ease rate limits). */
async function runCodexWhamBackgroundRefresh() {
  const targets = providers.value.filter(isOfficialCodexProvider);
  for (const p of targets) {
    await refreshCodexPlanFromChatgpt(p.id, { silent: true });
    await new Promise((res) => setTimeout(res, 400));
  }
}

/** 尝试合并本机 Codex / Claude：已有同名上游时补充凭证或刷新 auth_ref，幂等。 */
async function mergeLocalToolsFromDisk() {
  try {
    await api.providers.importLocal(["codex", "claude"]);
  } catch {
    /* 网关未启动或无 ~/.codex / ~/.claude */
  }
}

async function load() {
  try {
    await mergeLocalToolsFromDisk();
    providers.value = await api.providers.list();
    error.value = "";
    const results = await Promise.allSettled(
      providers.value.map((p) => api.providers.health(p.id, GATEWAY_ROLLING_STAT_HOURS)),
    );
    const map: Record<string, ProviderHealthSummary> = {};
    for (let i = 0; i < results.length; i++) {
      const r = results[i];
      if (r.status !== "fulfilled") continue;
      const body = r.value;
      if (!isProviderHealthSummary(body)) {
        error.value = "gateway_api:mismatch health.cumulative missing; restart rebuilt vibe binary";
        healthMap.value = {};
        poolByProviderId.value = {};
        return;
      }
      map[providers.value[i].id] = body;
    }
    healthMap.value = map;
    let pools: ProviderAuthPoolSummary[] = [];
    try {
      pools = await api.providers.pools(GATEWAY_ROLLING_STAT_HOURS);
    } catch {
      pools = [];
    }
    const poolMap: Record<string, ProviderAuthPoolSummary> = {};
    for (const pool of pools) {
      poolMap[pool.provider_id] = pool;
    }
    poolByProviderId.value = poolMap;
    await loadCodexPlanRowsAll();
    await Promise.all(providers.value.map((p) => loadCreds(p.id)));
    void runCodexWhamBackgroundRefresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function loadCreds(providerId: string) {
  loadingCreds.value[providerId] = true;
  try {
    credsByProvider.value[providerId] = await api.credentials.list(providerId);
    const creds = credsByProvider.value[providerId];
    await Promise.all(
      creds.map(async (c) => {
        try {
          planSnapByCred.value[c.id] = await api.credentials.plan(c.id);
        } catch {
          planSnapByCred.value[c.id] = null;
        }
      }),
    );
    const p = providers.value.find((x) => x.id === providerId);
    if (p && isOfficialCodexProvider(p)) await loadCodexPlanRowsForProvider(providerId);
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
    /* 保留旧池快照 */
  }
}

async function reloadProviderCreds(providerId: string) {
  await Promise.all([loadCreds(providerId), refreshSinglePool(providerId)]);
}

function poolCred(providerId: string, credentialId: string): CredentialPoolStatus | undefined {
  return poolByProviderId.value[providerId]?.credentials.find(
    (x) => x.credential_id === credentialId,
  );
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
  sortKey: string;
}

interface ProviderSectionView {
  key: ProviderGroupKey;
  title: string;
  description: string;
  providers: ProviderCardView[];
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

function providerKindFamily(kind: Provider["kind"]): string {
  switch (kind) {
    case "openai-responses":
      return "OpenAI Responses";
    case "openai-chat":
      return "OpenAI Chat";
    case "anthropic":
      return "Anthropic";
    case "gemini-native":
      return "Gemini Native";
    default:
      return kind;
  }
}

function protocolBadgeTone(mode: ProtocolSupportInfo["mode"]): string {
  if (mode === "native") return "border-emerald-200 bg-emerald-50 text-emerald-800";
  if (mode === "bridged") return "border-amber-200 bg-amber-50 text-amber-900";
  return "border-slate-200 bg-slate-100 text-slate-600";
}

function providerCardBadges(provider: Provider): ProviderCardProtocolBadge[] {
  return CLIENT_TOOLS.filter((tool) => tool.consumesKinds.includes(provider.kind)).map((tool) => ({
    toolId: tool.id,
    toolLabel: tool.shortLabel,
    toolIcon: tool.icon,
    support: getToolProtocolSupport(provider, tool),
  }));
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

  return {
    provider,
    title,
    badges,
    primarySupport: firstUsefulSupport,
    group,
    sortKey: `${provider.priority.toString().padStart(5, "0")}:${title.toLowerCase()}`,
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

  const groups: ProviderSectionView[] = [
    {
      key: "native",
      title: "native",
      description: "",
      providers: cards.filter((card) => card.group === "native"),
    },
    {
      key: "bridged",
      title: "bridge",
      description: "",
      providers: cards.filter((card) => card.group === "bridged"),
    },
    {
      key: "other",
      title: "other",
      description: "",
      providers: cards.filter((card) => card.group === "other"),
    },
  ];

  return groups.filter((section) => section.providers.length > 0);
});

function startAdd() {
  form.value = emptyForm();
  editTarget.value = null;
  showForm.value = true;
}
function startEdit(p: Provider) {
  form.value = {
    name: p.name,
    kind: p.kind,
    base_url: p.base_url,
    auth_ref: p.auth_ref,
    enabled: p.enabled,
    priority: p.priority,
    model_aliases: [...p.model_aliases],
  };
  editTarget.value = p;
  showForm.value = true;
}

async function save() {
  try {
    if (editTarget.value) await api.providers.update(editTarget.value.id, form.value);
    else await api.providers.create(form.value);
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
      kind: p.kind,
      base_url: p.base_url,
      auth_ref: p.auth_ref,
      enabled: next,
      priority: p.priority,
      model_aliases: [...p.model_aliases],
    });
    const ix = providers.value.findIndex((x) => x.id === p.id);
    if (ix >= 0) {
      providers.value[ix] = { ...providers.value[ix], enabled: next };
      providers.value = [...providers.value];
    }
    // 开关与熔断绑定：重新启用时主动清空 provider/credential 熔断，避免“已启用但仍被阻断”。
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

async function saveCred() {
  try {
    if (editCred.value) {
      await api.credentials.update(editCred.value.id, credForm.value);
    } else {
      await api.credentials.create(credProviderId.value, credForm.value);
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

function circuitBadge(state: string) {
  if (state === "closed")
    return { label: "ok", cls: "bg-emerald-50 text-emerald-800 border-emerald-200" };
  if (state === "half-open")
    return { label: "half-open", cls: "bg-amber-50 text-amber-900 border-amber-200" };
  return { label: "open", cls: "bg-red-50 text-red-800 border-red-200" };
}

function isCircuitResettable(state: string): boolean {
  return state === "open" || state === "half-open";
}

onMounted(() => {
  void load();
  window.addEventListener("scroll", onPresetViewportChange, true);
  window.addEventListener("resize", onPresetViewportChange);
});
onUnmounted(() => {
  document.removeEventListener("pointerdown", onPresetGlobalPointerDown, true);
  window.removeEventListener("scroll", onPresetViewportChange, true);
  window.removeEventListener("resize", onPresetViewportChange);
});
</script>

<template>
  <div class="mx-auto max-w-6xl">
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
            class="btn-ghost flex min-h-11 min-w-11 items-center justify-center gap-2 px-2.5 py-2 text-sm rounded-lg border border-vp-border/80 sm:px-3.5 sm:py-1.5"
            title="local:import"
            aria-label="local:import"
            @click="openImport"
          >
            <VpIcon name="folder-input" size-class="size-4 shrink-0" />
            <span class="sr-only">local:import</span>
          </button>
          <div ref="presetTriggerWrap" class="relative">
            <button
              type="button"
              class="btn-ghost flex min-h-11 min-w-11 items-center justify-center gap-2 px-2.5 py-2 text-sm rounded-lg border border-vp-border/80 sm:px-3.5 sm:py-1.5"
              aria-label="presets"
              title="presets"
              @click="showPresets = !showPresets"
            >
              <VpIcon name="sparkles" size-class="size-4 shrink-0" />
              <span class="sr-only">presets</span>
            </button>
          </div>
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
      <div class="card-base p-3 sm:p-4">
        <div class="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
          <div class="min-w-0">
            <p class="sr-only">provider:view</p>
          </div>
          <div v-if="workspaceView === 'overview'" class="flex flex-wrap gap-2">
            <button
              v-for="tab in providerTabs"
              :key="tab.id"
              type="button"
              class="inline-flex items-center gap-2 rounded-xl border px-3 py-2 text-sm font-medium transition-colors"
              :class="
                activeProviderTab === tab.id
                  ? 'border-violet-300 bg-violet-50 text-violet-800 shadow-sm'
                  : 'border-slate-200 bg-white text-slate-600 hover:border-slate-300 hover:bg-slate-50'
              "
              @click="activeProviderTab = tab.id"
            >
              <span :class="[tab.icon, 'size-4']" aria-hidden="true" />
              <span class="hidden sm:inline">{{ tab.shortLabel }}</span>
            </button>
          </div>
          <div
            v-else
            class="rounded-xl border border-vp-border bg-vp-surface px-3 py-2 text-xs font-medium text-vp-muted font-mono"
          >
            {{ workspaceView }}
          </div>
        </div>
        <div
          v-if="activeToolTab"
          class="mt-3 rounded-xl border border-violet-200 bg-violet-50/70 px-3 py-2 font-mono text-xs text-violet-900"
        >
          {{ activeToolTab.setupHint }}
        </div>
      </div>

      <div v-for="section in providerSections" :key="section.key" class="space-y-2.5">
        <div class="px-1">
          <div class="flex flex-wrap items-center gap-2">
            <span
              :class="[
                section.key === 'native'
                  ? 'i-lucide-plug'
                  : section.key === 'bridged'
                    ? 'i-lucide-route'
                    : 'i-lucide-archive',
                'size-4 text-slate-500',
              ]"
              aria-hidden="true"
            />
            <h2 class="text-sm font-semibold text-slate-900">{{ section.title }}</h2>
            <span
              class="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-[11px] text-slate-500"
            >
              {{ section.providers.length }}
            </span>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-2 xl:grid-cols-2">
          <ProviderCard
            v-for="card in section.providers"
            :key="card.provider.id"
            :card="card"
            :health="healthMap[card.provider.id]"
            :creds="credsByProvider[card.provider.id] ?? []"
            :loading-creds="!!loadingCreds[card.provider.id]"
            :toggle-provider-busy="!!toggleBusy[card.provider.id]"
            :circuit-reset-busy="!!circuitResetBusy[card.provider.id]"
            :cred-toggle-busy="credToggleBusy"
            :pool-rows="poolByProviderId[card.provider.id]?.credentials ?? []"
            :plan-snap-by-cred="planSnapByCred"
            @sync-creds="reloadProviderCreds($event)"
            @toggle-provider="toggleProviderEnabled($event)"
            @reset-circuit="resetProviderCircuit($event)"
            @edit-provider="startEdit($event)"
            @delete-provider="remove($event)"
            @add-cred="startAddCred($event)"
            @toggle-cred="toggleCredentialEnabled($event)"
            @edit-cred="startEditCred($event)"
            @delete-cred="removeCred($event)"
          />
        </div>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="showPresets"
        ref="presetPanelRef"
        class="fixed z-[105] bg-white border border-slate-200 rounded-xl shadow-lg p-3 w-72 max-h-[min(70vh,calc(100dvh-1rem))] overflow-y-auto"
        :style="{ top: `${presetMenuPos.top}px`, left: `${presetMenuPos.left}px` }"
        role="menu"
        aria-label="presets"
      >
        <p class="sr-only">presets</p>
        <button
          v-for="preset in PRESETS"
          :key="preset.label"
          type="button"
          class="w-full text-left px-3 py-2 rounded-lg hover:bg-slate-50 text-sm flex items-center gap-2 transition-colors"
          @click="applyPreset(preset)"
        >
          <span :class="[preset.icon, 'size-4 text-slate-500']" aria-hidden="true" />
          <div>
            <div class="font-medium text-slate-900">{{ preset.label }}</div>
            <div class="text-xs text-slate-500">p{{ preset.priority }} · {{ preset.kind }}</div>
          </div>
        </button>
      </div>
    </Teleport>

    <Teleport to="body">
      <div
        v-if="showForm"
        class="vp-modal-backdrop z-[110]"
        role="dialog"
        aria-modal="true"
        aria-labelledby="provider-form-title"
        @click.self="showForm = false"
      >
        <div class="vp-modal-panel max-w-lg flex flex-col" @click.stop>
          <div class="vp-modal-header">
            <span
              class="grid size-10 shrink-0 place-items-center rounded-xl bg-violet-100 text-violet-700 ring-1 ring-violet-200"
              aria-hidden="true"
            >
              <VpIcon name="server" size-class="size-5" />
            </span>
            <div class="min-w-0 flex-1">
              <h2 id="provider-form-title" class="font-semibold text-lg text-vp-text">
                provider.{{ editTarget ? "edit" : "add" }}
              </h2>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="close"
              title="close"
              @click="showForm = false"
            >
              <VpIcon name="x" size-class="size-5" />
            </button>
          </div>
          <div class="px-6 py-4 overflow-y-auto max-h-[min(32rem,70vh)] space-y-3">
            <label class="block">
              <span class="sr-only">name</span>
              <input
                v-model="form.name"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900 focus:outline-none focus:border-violet-500"
              />
            </label>
            <label class="block">
              <span class="sr-only">kind</span>
              <select
                v-model="form.kind"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              >
                <option value="anthropic">anthropic</option>
                <option value="openai-chat">openai-chat</option>
                <option value="openai-responses">openai-responses</option>
                <option value="gemini-native">gemini-native</option>
              </select>
            </label>
            <label class="block">
              <span class="sr-only">base_url</span>
              <input
                v-model="form.base_url"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
              />
            </label>
            <label class="block">
              <span class="sr-only">auth_ref.default</span>
              <input
                v-model="form.auth_ref"
                placeholder="keyring:my-key  or  env:MY_API_KEY"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
              />
            </label>
            <label class="block">
              <span class="sr-only">priority</span>
              <input
                v-model.number="form.priority"
                type="number"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input v-model="form.enabled" type="checkbox" class="rounded" />
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
              @click="showForm = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span class="sr-only">cancel</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors"
              aria-label="provider:save"
              @click="save"
            >
              <VpIcon name="check" size-class="size-4 text-white" />
              <span class="sr-only">save</span>
            </button>
          </div>
        </div>
      </div>
    </Teleport>

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
                  placeholder="keyring:my-key  or  env:MY_API_KEY  or  literal:sk-…"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
                />
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
    <Teleport to="body">
      <div
        v-if="showImport"
        class="vp-modal-backdrop z-[110]"
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-local-title"
        @click.self="showImport = false"
      >
        <div class="vp-modal-panel max-w-lg flex flex-col max-h-[90vh]" @click.stop>
          <div class="vp-modal-header">
            <span
              class="grid size-10 shrink-0 place-items-center rounded-xl bg-cyan-100 text-cyan-800 ring-1 ring-cyan-200"
              aria-hidden="true"
            >
              <VpIcon name="download" size-class="size-5" />
            </span>
            <div class="min-w-0 flex-1">
              <h2 id="import-local-title" class="sr-only">local.import</h2>
              <p class="text-sm text-vp-muted mt-1 leading-relaxed">
                <code
                  class="font-mono text-slate-700 bg-slate-100 px-1 rounded border border-slate-200 text-xs"
                  >auth*.json</code
                >
              </p>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="close"
              title="close"
              @click="showImport = false"
            >
              <VpIcon name="x" size-class="size-5" />
            </button>
          </div>
          <div class="px-6 py-4 overflow-y-auto flex-1 min-h-0">
            <div
              v-if="importLoading"
              class="text-sm text-slate-600 py-4 text-center flex items-center justify-center gap-2"
            >
              <VpIcon name="loader-2" size-class="size-4 animate-spin" />
              ...
            </div>
            <div
              v-else-if="importError"
              class="text-sm text-red-700 bg-red-50 border border-red-200 rounded-lg px-4 py-2 mb-4"
            >
              {{ importError }}
            </div>
            <div
              v-else-if="localCandidates.length === 0"
              class="text-sm text-slate-600 py-4 text-center"
            >
              <span class="sr-only">empty</span>
            </div>
            <div v-else class="space-y-3">
              <div
                v-for="c in localCandidates"
                :key="c.client"
                class="bg-slate-50 rounded-xl border border-slate-200 p-4"
              >
                <div class="flex items-start justify-between gap-3 card-lift">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 flex-wrap">
                      <span class="font-medium text-slate-900">{{ c.name }}</span>
                      <span
                        :class="
                          c.token_ok
                            ? 'bg-emerald-50 text-emerald-800 border border-emerald-200'
                            : 'bg-amber-50 text-amber-900 border border-amber-200'
                        "
                        class="text-xs px-1.5 py-0.5 rounded"
                      >
                        {{ c.token_ok ? "token:ok" : "token:missing" }}
                      </span>
                      <span
                        class="text-xs px-1.5 py-0.5 rounded bg-white border border-slate-200 text-slate-600"
                        >{{ c.kind }}</span
                      >
                    </div>
                    <div class="text-xs text-slate-600 mt-1 font-mono truncate">
                      {{ c.source_path }}
                    </div>

                    <!-- Extra credentials (additional accounts) -->
                    <div v-if="(c.extra_credentials?.length ?? 0) > 0" class="mt-2 space-y-1">
                      <div class="text-xs text-slate-600 font-medium">
                        +{{ c.extra_credentials?.length ?? 0 }}
                      </div>
                      <div
                        v-for="ec in c.extra_credentials ?? []"
                        :key="ec.source_path"
                        class="flex items-center gap-2 text-xs text-slate-600"
                      >
                        <span :class="ec.token_ok ? 'text-emerald-600' : 'text-amber-600'">●</span>
                        <span class="font-mono truncate">{{ ec.label }}</span>
                      </div>
                    </div>
                  </div>

                  <button
                    type="button"
                    class="shrink-0 inline-flex items-center justify-center rounded-lg bg-violet-600 hover:bg-violet-700 text-white disabled:opacity-50 disabled:cursor-not-allowed p-2.5 sm:px-3 sm:py-1.5 transition-colors"
                    :disabled="importingClients.has(c.client)"
                    :aria-label="`import ${c.name}`"
                    :title="importingClients.has(c.client) ? 'importing' : 'import'"
                    @click="doImport(c.client)"
                  >
                    <VpIcon
                      v-if="importingClients.has(c.client)"
                      name="loader-2"
                      size-class="size-5 text-white animate-spin"
                    />
                    <VpIcon v-else name="folder-input" size-class="size-5 text-white" />
                  </button>
                </div>
              </div>
            </div>
          </div>

          <div
            class="flex justify-end gap-2 px-6 py-4 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
          >
            <button
              type="button"
              class="btn-ghost flex items-center gap-2 !px-3"
              aria-label="close"
              @click="showImport = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span class="sr-only">close</span>
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

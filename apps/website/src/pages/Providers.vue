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
import { getCodexClientTool, providerServesCodexCliRoute } from "../utils/client-tools.ts";
import { useRoute } from "vue-router";
import { resolvePageAccent } from "../utils/page-accent.ts";
import { mapUpstreamUserMessage, displayProviderName } from "../utils/providers-display.ts";
import { hintsFromAuthJsonTokens } from "../utils/codex-oauth-hints.ts";
import VpIcon from "../components/vp-icon.vue";
import ProviderCredentialRow from "../components/provider-credential-row.vue";

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
    icon: "🤖",
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
    icon: "🔮",
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
    icon: "🐳",
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
    icon: "☁️",
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
    icon: "🌙",
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
    icon: "⚡",
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
    icon: "✨",
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
          [providerId]: "本供应商尚无 OAuth 凭证，请用「本机导入」或下方「添加密钥」。",
        };
      } else {
        codexRefreshNote.value = {
          ...codexRefreshNote.value,
          [providerId]: errPart
            ? `已更新 ${r.ok}/${r.attempted} · ${errPart}`
            : `已更新 ${r.ok}/${r.attempted} 条凭证。`,
        };
      }
    } else if (errPart || (r.attempted > 0 && r.ok === 0)) {
      codexRefreshNote.value = {
        ...codexRefreshNote.value,
        [providerId]: errPart || `套餐同步失败（${r.ok}/${r.attempted}）。`,
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
        error.value =
          "网关 API 已升级：正在运行的 vibe 进程仍是旧二进制（health 无 cumulative）。请停止进程、用本仓库重新构建并启动 vibe，再硬刷新页面；前端 HMR 不会替换 Rust 进程。";
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
  return `Codex CLI：经网关前缀 ${t.pathPrefix} 的到达请求可路由到此前端供应商「${displayProviderName(provider.name)}」，上游类型为 ${provider.kind}。`;
}

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

function circuitBadge(state: string) {
  if (state === "closed")
    return { label: "正常", cls: "bg-emerald-50 text-emerald-800 border-emerald-200" };
  if (state === "half-open")
    return { label: "探测中", cls: "bg-amber-50 text-amber-900 border-amber-200" };
  return { label: "熔断", cls: "bg-red-50 text-red-800 border-red-200" };
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
  <div>
    <div
      class="relative rounded-2xl border border-slate-200/90 bg-gradient-to-br from-violet-100/80 via-white to-cyan-50/60 mb-6 shadow-sm"
    >
      <div
        aria-hidden="true"
        class="pointer-events-none absolute inset-0 overflow-hidden rounded-2xl"
      >
        <div class="absolute -right-20 -top-24 size-64 rounded-full bg-violet-200/40 blur-3xl" />
        <div class="absolute -bottom-24 left-24 size-72 rounded-full bg-cyan-200/30 blur-3xl" />
      </div>
      <div
        class="relative z-10 flex flex-col gap-4 p-5 sm:flex-row sm:items-start sm:justify-between sm:gap-6"
      >
        <div class="min-w-0 flex-1">
          <span :class="['text-xs uppercase', pageAccent.kicker]">网关 · 上游</span>
          <h1 :class="['text-3xl font-bold tracking-tight', pageAccent.heading]">Providers</h1>
          <p class="text-sm text-slate-600 mt-1.5 max-w-2xl leading-relaxed">
            优先服务 <strong class="text-slate-800">Codex</strong>：OAuth 与密钥池走网关，CLI 将
            <code
              class="font-mono text-violet-800 bg-violet-50 px-1 rounded border border-violet-200"
              >/codex/v1</code
            >
            指到本机端口；其它客户端路由在下方列表中为每个上游单独配置。
          </p>
        </div>
        <div class="flex w-full shrink-0 flex-wrap items-center justify-end gap-2 sm:w-auto">
          <button
            type="button"
            class="btn-ghost flex min-h-11 min-w-11 items-center justify-center gap-2 px-2.5 py-2 text-sm rounded-lg border border-vp-border/80 sm:px-3.5 sm:py-1.5"
            title="从本机已安装工具导入（Claude Code、Codex CLI 等）"
            aria-label="从本机已安装工具导入"
            @click="openImport"
          >
            <VpIcon name="folder-input" size-class="size-4 shrink-0" />
            <span class="hidden sm:inline">本机导入</span>
          </button>
          <div ref="presetTriggerWrap" class="relative">
            <button
              type="button"
              class="btn-ghost flex min-h-11 min-w-11 items-center justify-center gap-2 px-2.5 py-2 text-sm rounded-lg border border-vp-border/80 sm:px-3.5 sm:py-1.5"
              aria-label="打开上游预设列表"
              title="预设"
              @click="showPresets = !showPresets"
            >
              <VpIcon name="sparkles" size-class="size-4 shrink-0" />
              <span class="hidden sm:inline">预设</span>
            </button>
          </div>
          <button
            type="button"
            :class="[
              'flex min-h-11 min-w-11 items-center justify-center gap-2 px-3 py-2 sm:py-1.5 rounded-lg text-sm font-medium',
              pageAccent.btnPrimary,
            ]"
            aria-label="添加上游供应商"
            @click="startAdd"
          >
            <VpIcon name="plus" size-class="size-4 shrink-0 text-white" />
            <span class="hidden sm:inline">添加上游</span>
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

    <div v-if="loading" class="text-slate-500 text-sm">加载中…</div>
    <div v-else-if="providers.length === 0" class="text-slate-500 text-sm py-12 text-center">
      尚无上游。点击 <strong>+ 添加上游</strong> 开始配置。
    </div>
    <div v-else class="space-y-3">
      <div v-for="p in providers" :key="p.id" class="card-base min-w-0 overflow-hidden card-lift">
        <!-- Provider row -->
        <div
          class="px-4 py-4 bg-gradient-to-r from-slate-50/80 to-transparent border-b border-slate-100 sm:px-5"
        >
          <div class="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-start sm:gap-4">
            <div
              class="grid size-11 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-violet-100 to-cyan-50 text-lg ring-1 ring-slate-200"
            >
              <span v-if="p.kind === 'anthropic'">🔮</span>
              <span v-else-if="p.kind === 'gemini-native'">✨</span>
              <span v-else-if="p.kind === 'openai-responses'">🤖</span>
              <span v-else>⚡</span>
            </div>
            <div class="w-full min-w-0 flex-1">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="min-w-0 break-words font-semibold text-slate-900">{{
                  displayProviderName(p.name)
                }}</span>
                <span
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-slate-200 bg-slate-100 text-slate-600"
                  >{{ p.kind }}</span
                >
                <span
                  v-if="providerServesCodexCliRoute(p)"
                  class="inline-flex items-center gap-0.5 rounded-md border border-violet-200 bg-violet-50 px-1.5 py-0.5 text-[10px] font-semibold text-violet-800"
                  role="img"
                  :aria-label="codexCliRouteAriaLabel(p)"
                >
                  <span aria-hidden="true">{{ codexRouteTool.icon }}</span>
                  <span aria-hidden="true">Codex</span>
                </span>
                <span
                  v-if="!p.enabled"
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-amber-200 bg-amber-50 text-amber-900"
                  >已暂停</span
                >
                <template v-if="healthMap[p.id]?.cumulative">
                  <span
                    :class="circuitBadge(healthMap[p.id].cumulative.circuit_state).cls"
                    class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border"
                  >
                    {{ circuitBadge(healthMap[p.id].cumulative.circuit_state).label }}
                  </span>
                </template>
              </div>

              <div
                v-if="isOfficialCodexProvider(p)"
                class="mt-2 flex flex-wrap items-center gap-2 text-xs text-slate-600"
              >
                <span class="font-medium text-slate-800 shrink-0">Codex</span>
                <button
                  type="button"
                  class="shrink-0 vp-icon-btn border border-violet-200 text-violet-700"
                  :disabled="!!codexPlanRefreshing[p.id]"
                  aria-label="同步 Codex 用量"
                  title="同步用量"
                  @click.stop="refreshCodexPlanFromChatgpt(p.id)"
                >
                  <VpIcon
                    name="refresh-cw"
                    size-class="size-4"
                    :spin="!!codexPlanRefreshing[p.id]"
                  />
                </button>
                <span v-if="codexPlanRefreshing[p.id]" class="text-slate-500">同步中…</span>
                <template v-else>
                  <span v-if="!codexPlanRowsByProvider[p.id]?.length" class="min-w-0">
                    尚无 OAuth 凭证，请本机导入或下方添加。
                  </span>
                  <span v-else class="text-slate-600">
                    已同步 {{ codexPlanRowsByProvider[p.id].length }} 条
                  </span>
                </template>
                <p v-if="codexRefreshNote[p.id]" class="w-full text-amber-800 break-words">
                  {{ codexRefreshNote[p.id] }}
                </p>
              </div>

              <p
                class="mt-1 truncate text-xs text-slate-500"
                :title="`${p.base_url} · priority ${p.priority}`"
              >
                {{ p.base_url }} · 优先级 {{ p.priority }}
              </p>

              <details
                v-if="
                  healthMap[p.id]?.rolling || healthMap[p.id]?.cumulative || p.model_aliases.length
                "
                class="mt-2 rounded-lg border border-slate-200 bg-white/70 px-3 py-2 text-[11px] text-slate-600"
              >
                <summary
                  class="cursor-pointer select-none font-medium text-slate-700 list-none marker:content-none [&::-webkit-details-marker]:hidden"
                >
                  网关与模型别名
                </summary>
                <div class="mt-2 space-y-2 border-t border-slate-100 pt-2">
                  <div v-if="healthMap[p.id]?.rolling" class="flex flex-wrap gap-x-3 gap-y-0.5">
                    <span class="text-slate-500">近 {{ healthMap[p.id]?.rolling_hours }}h</span>
                    <span>{{ healthMap[p.id]!.rolling!.requests.toLocaleString() }} 次</span>
                    <span
                      :class="healthMap[p.id]!.rolling!.success_rate < 0.9 ? 'text-red-600' : ''"
                    >
                      {{ (healthMap[p.id]!.rolling!.success_rate * 100).toFixed(1) }}% 成功
                    </span>
                    <span v-if="healthMap[p.id]!.rolling!.avg_latency_ms != null">
                      {{ healthMap[p.id]!.rolling!.avg_latency_ms }}ms
                    </span>
                    <span
                      v-if="(healthMap[p.id]!.rolling!.err_429 ?? 0) > 0"
                      class="text-amber-700"
                    >
                      429 ×{{ healthMap[p.id]!.rolling!.err_429 }}
                    </span>
                  </div>
                  <div
                    v-if="healthMap[p.id]?.cumulative"
                    class="flex flex-wrap gap-x-2 gap-y-0.5 text-slate-500"
                  >
                    <span>
                      累计 {{ healthMap[p.id]!.cumulative.total_requests.toLocaleString() }} 次 ·
                      {{ (healthMap[p.id]!.cumulative.success_rate * 100).toFixed(1) }}% 成功
                    </span>
                    <span
                      v-if="healthMap[p.id]!.cumulative.consecutive_failures > 0"
                      class="text-red-600"
                    >
                      连失败 {{ healthMap[p.id]!.cumulative.consecutive_failures }}
                    </span>
                    <span
                      v-if="healthMap[p.id]!.cumulative.last_error"
                      class="max-w-full truncate text-red-600"
                      :title="healthMap[p.id]!.cumulative.last_error ?? ''"
                    >
                      {{
                        mapUpstreamUserMessage(healthMap[p.id]!.cumulative.last_error) ??
                        healthMap[p.id]!.cumulative.last_error
                      }}
                    </span>
                    <span
                      v-if="isCircuitResettable(healthMap[p.id]!.cumulative.circuit_state)"
                      class="text-amber-800"
                    >
                      熔断拦截中
                    </span>
                  </div>
                  <div v-if="p.model_aliases.length" class="flex flex-wrap gap-1.5">
                    <span
                      v-for="a in p.model_aliases"
                      :key="a.alias"
                      class="max-w-full break-words rounded border border-slate-200 bg-slate-50 px-1.5 py-0.5 font-mono text-[10px] text-slate-700"
                    >
                      {{ a.alias }}→{{ a.upstream_model }}
                    </span>
                  </div>
                </div>
              </details>
            </div>

            <div
              class="flex w-full shrink-0 flex-wrap items-start justify-start gap-2 sm:w-auto sm:justify-end"
            >
              <button
                v-if="
                  healthMap[p.id]?.cumulative &&
                  isCircuitResettable(healthMap[p.id].cumulative.circuit_state)
                "
                type="button"
                class="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-amber-300 bg-amber-50 px-2 py-2 text-xs text-amber-900 transition-colors hover:bg-amber-100 disabled:opacity-50 sm:px-3 sm:py-1.5"
                :disabled="!!circuitResetBusy[p.id]"
                aria-label="重置熔断器"
                title="重置熔断"
                @click="resetProviderCircuit(p.id)"
              >
                <VpIcon name="rotate-ccw" size-class="size-3.5 shrink-0" />
                <span class="hidden sm:inline">{{
                  circuitResetBusy[p.id] ? "重置中…" : "重置熔断"
                }}</span>
              </button>
              <button
                type="button"
                role="switch"
                :aria-checked="p.enabled"
                :aria-label="p.enabled ? `Disable provider ${p.name}` : `Enable provider ${p.name}`"
                :disabled="!!toggleBusy[p.id]"
                class="relative w-11 h-6 shrink-0 rounded-full transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white disabled:opacity-50 shadow-inner"
                :class="p.enabled ? 'bg-emerald-500 shadow-emerald-600/20' : 'bg-slate-300'"
                @click.stop="toggleProviderEnabled(p)"
              >
                <span
                  class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform"
                  :class="p.enabled ? 'translate-x-5' : 'translate-x-0'"
                />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/80"
                :disabled="!!loadingCreds[p.id]"
                aria-label="重新拉取凭证与密钥池"
                title="同步凭证"
                @click="reloadProviderCreds(p.id)"
              >
                <VpIcon name="refresh-cw" size-class="size-4" :spin="!!loadingCreds[p.id]" />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/80"
                aria-label="编辑供应商"
                title="编辑"
                @click="startEdit(p)"
              >
                <VpIcon name="pencil" size-class="size-4" />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700"
                aria-label="删除供应商"
                title="删除"
                @click="remove(p.id)"
              >
                <VpIcon name="trash-2" size-class="size-4" />
              </button>
            </div>
          </div>
        </div>

        <!-- 凭证：单行摘要 + 每行内「高级」 -->
        <div
          class="border-t border-solid border-default surface-muted px-4 py-3 rounded-b-xl sm:px-5"
        >
          <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
            <span class="text-sm font-medium text-slate-800">凭证</span>
            <button
              type="button"
              :class="[
                'flex min-h-11 items-center gap-2 text-xs px-3 py-2 sm:py-1.5 rounded-lg font-medium transition-colors shadow-sm',
                pageAccent.btnPrimary,
              ]"
              aria-label="添加密钥"
              @click="startAddCred(p.id)"
            >
              <VpIcon name="key" size-class="size-3.5 shrink-0 text-white" />
              <span>添加密钥</span>
            </button>
          </div>

          <div v-if="loadingCreds[p.id]" class="text-xs text-slate-500">加载中…</div>
          <div
            v-else-if="!credsByProvider[p.id] || credsByProvider[p.id].length === 0"
            class="text-xs text-slate-600"
          >
            无独立凭证；将使用供应商默认密钥（在「编辑上游」中配置）。
          </div>
          <div v-else class="space-y-2">
            <ProviderCredentialRow
              v-for="c in credsByProvider[p.id]"
              :key="c.id"
              :credential="c"
              :pool-row="poolCred(p.id, c.id)"
              :plan-snap="planSnapByCred[c.id] ?? null"
              :peer-creds="credsByProvider[p.id] ?? []"
              @edit="startEditCred($event)"
              @delete="removeCred($event)"
            />
          </div>
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
        aria-label="上游预设"
      >
        <p class="text-xs text-slate-500 mb-2 px-1">快速填充常见上游</p>
        <button
          v-for="preset in PRESETS"
          :key="preset.label"
          type="button"
          class="w-full text-left px-3 py-2 rounded-lg hover:bg-slate-50 text-sm flex items-center gap-2 transition-colors"
          @click="applyPreset(preset)"
        >
          <span>{{ preset.icon }}</span>
          <div>
            <div class="font-medium text-slate-900">{{ preset.label }}</div>
            <div class="text-xs text-slate-500">
              优先级 {{ preset.priority }} · {{ preset.kind }}
            </div>
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
                {{ editTarget ? "编辑" : "添加" }} 上游
              </h2>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="关闭"
              title="关闭"
              @click="showForm = false"
            >
              <VpIcon name="x" size-class="size-5" />
            </button>
          </div>
          <div class="px-6 py-4 overflow-y-auto max-h-[min(32rem,70vh)] space-y-3">
            <label class="block">
              <span class="text-xs text-slate-500">名称</span>
              <input
                v-model="form.name"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900 focus:outline-none focus:border-violet-500"
              />
            </label>
            <label class="block">
              <span class="text-xs text-slate-500">类型 Kind</span>
              <select
                v-model="form.kind"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              >
                <option value="anthropic">Anthropic</option>
                <option value="openai-chat">OpenAI Chat (/v1/chat/completions)</option>
                <option value="openai-responses">OpenAI Responses</option>
                <option value="gemini-native">Gemini Native</option>
              </select>
            </label>
            <label class="block">
              <span class="text-xs text-slate-500">Base URL</span>
              <input
                v-model="form.base_url"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
              />
            </label>
            <label class="block">
              <span class="text-xs text-slate-500"> 默认 auth_ref（无凭证条目时使用） </span>
              <input
                v-model="form.auth_ref"
                placeholder="keyring:my-key  or  env:MY_API_KEY"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
              />
            </label>
            <label class="block">
              <span class="text-xs text-slate-500">优先级（数值越小越优先）</span>
              <input
                v-model.number="form.priority"
                type="number"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input v-model="form.enabled" type="checkbox" class="rounded" />
              Enabled
            </label>
          </div>
          <div
            class="flex gap-3 px-6 py-4 border-t border-vp-border justify-end bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
          >
            <button
              type="button"
              class="btn-ghost flex items-center gap-2 !px-3"
              aria-label="取消"
              @click="showForm = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span>取消</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors"
              aria-label="保存上游"
              @click="save"
            >
              <VpIcon name="check" size-class="size-4 text-white" />
              <span>保存</span>
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
                {{ editCred ? "编辑" : "添加" }} 凭证
              </h2>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="关闭"
              title="关闭"
              @click="showCredForm = false"
            >
              <VpIcon name="x" size-class="size-5" />
            </button>
          </div>
          <div class="px-6 py-4 space-y-3 overflow-y-auto max-h-[min(36rem,72vh)]">
            <label class="block">
              <span class="text-xs text-slate-500">Label</span>
              <input
                v-model="credForm.label"
                placeholder="e.g. Codex Pro (account 2)"
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
                API Key / auth_ref
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
                OAuth（Codex）
              </button>
            </div>

            <!-- API Key mode -->
            <template v-if="credAuthMode === 'apikey'">
              <label class="block">
                <span class="text-xs text-slate-500">Auth ref</span>
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
                    导入 Codex <code class="font-mono text-slate-600">auth*.json</code>
                  </p>
                  <button
                    type="button"
                    class="shrink-0 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-white border border-slate-200 hover:bg-slate-50 text-slate-800 transition-colors w-full sm:w-auto"
                    aria-label="选择 JSON 文件"
                    title="选择文件"
                    @click="triggerAuthJsonFilePick"
                  >
                    <VpIcon name="folder-input" size-class="size-4" />
                    <span class="hidden sm:inline">选择文件</span>
                  </button>
                </div>
                <p class="text-[11px] text-slate-600 leading-snug">
                  Paste JSON below or drag-and-drop a file. Parsed only in your browser; nothing is
                  uploaded until you save.
                </p>
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
                    aria-label="解析 JSON 并填入字段"
                    @click="parseAuthJsonPaste"
                  >
                    <VpIcon name="zap" size-class="size-4 text-white" />
                    <span>解析并填入</span>
                  </button>
                </div>
              </div>
              <label class="block">
                <span class="text-xs text-slate-500">Access Token</span>
                <input
                  v-model="credForm.oauth_access_token"
                  placeholder="eyJhbGciOiJSUzI1NiJ9…"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                />
              </label>
              <label class="block">
                <span class="text-xs text-slate-500"
                  >Refresh token (write-only; never shown after save)</span
                >
                <input
                  v-model="credForm.oauth_refresh_token"
                  placeholder="Leave empty to keep stored refresh token"
                  type="password"
                  class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                />
              </label>
              <p class="text-xs text-slate-600">
                Expires:
                {{
                  credForm.oauth_expires_at
                    ? new Date(credForm.oauth_expires_at * 1000).toLocaleString()
                    : "Unknown (paste auth.json to detect)"
                }}
              </p>
            </template>

            <label class="block">
              <span class="text-xs text-slate-500">Plan type (optional)</span>
              <input
                v-model="credForm.plan_type"
                placeholder="claude-pro · codex-plus · codex-pro · payg · …"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="block">
              <span class="text-xs text-slate-500">Priority</span>
              <input
                v-model.number="credForm.priority"
                type="number"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="block">
              <span class="text-xs text-slate-500">Notes</span>
              <input
                v-model="credForm.notes"
                placeholder="optional notes"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              />
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input v-model="credForm.enabled" type="checkbox" class="rounded" />
              Enabled
            </label>
          </div>
          <div
            class="flex gap-3 px-6 py-4 border-t border-vp-border justify-end bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
          >
            <button
              type="button"
              class="btn-ghost flex items-center gap-2 !px-3"
              aria-label="取消"
              @click="showCredForm = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span>取消</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors"
              aria-label="保存凭证"
              @click="saveCred"
            >
              <VpIcon name="check" size-class="size-4 text-white" />
              <span>保存</span>
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
              <h2 id="import-local-title" class="font-semibold text-lg text-vp-text">
                从本机工具导入
              </h2>
              <p class="text-sm text-vp-muted mt-1 leading-relaxed">
                扫描本机已安装客户端。Codex 的
                <code
                  class="font-mono text-slate-700 bg-slate-100 px-1 rounded border border-slate-200 text-xs"
                  >auth*.json</code
                >
                仅读取一次，OAuth 写入网关数据库。
              </p>
            </div>
            <button
              type="button"
              class="vp-icon-btn shrink-0"
              aria-label="关闭"
              title="关闭"
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
              扫描中…
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
              未发现本地工具。请先安装 Claude Code 或 Codex CLI。
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
                        {{ c.token_ok ? "令牌可用" : "无令牌" }}
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
                        另有 {{ c.extra_credentials?.length ?? 0 }} 个账号将添加为凭证：
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
                    :aria-label="`导入 ${c.name}`"
                    :title="importingClients.has(c.client) ? '导入中' : '导入'"
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
              aria-label="关闭"
              @click="showImport = false"
            >
              <VpIcon name="x" size-class="size-4" />
              <span>关闭</span>
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

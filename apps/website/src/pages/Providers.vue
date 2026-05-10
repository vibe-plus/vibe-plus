<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
  api,
  type Provider,
  type ProviderInput,
  type ProviderHealthSummary,
  type Credential,
  type CredentialInput,
  type CredentialPlanSnapshot,
  type ProviderCodexPlanItem,
  type LocalCandidate,
  isProviderHealthSummary,
} from "../api/client.ts";
import { CLIENT_TOOLS, toolProviderStats, toolProxyExample } from "../utils/client-tools.ts";

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

const providers = ref<Provider[]>([]);
/** Codex / Claude Code / OpenCode vs provider kind coverage. */
const clientToolSummaries = computed(() =>
  CLIENT_TOOLS.map((tool) => ({
    tool,
    stats: toolProviderStats(providers.value, tool),
  })),
);
const healthMap = ref<Record<string, ProviderHealthSummary>>({});
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

// Credential management
const expandedProvider = ref<string | null>(null);
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

function fillOauthFromTriple(triple: OauthTriple) {
  credForm.value.oauth_access_token = triple.access;
  credForm.value.oauth_refresh_token = triple.refresh;
  credForm.value.oauth_expires_at = triple.exp ?? jwtExp(triple.access);
  credForm.value.auth_ref = null;
  credAuthMode.value = "oauth";
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
      fillOauthFromTriple(triple);
      if (clearPaste) authJsonPaste.value = "";
      return;
    }

    if (mode === "apikey" || mode === "") {
      if (useLiteralKey) {
        credForm.value.auth_ref = `literal:${String(openaiKey).trim()}`;
        credForm.value.oauth_access_token = null;
        credForm.value.oauth_refresh_token = null;
        credForm.value.oauth_expires_at = null;
        credAuthMode.value = "apikey";
        if (clearPaste) authJsonPaste.value = "";
        return;
      }
      const triple = extractOauthTriple(v);
      if (triple) {
        fillOauthFromTriple(triple);
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
          [providerId]: "No OAuth credentials on this provider — use Import local or Keys.",
        };
      } else {
        codexRefreshNote.value = {
          ...codexRefreshNote.value,
          [providerId]: errPart
            ? `Updated ${r.ok}/${r.attempted} · ${errPart}`
            : `Updated ${r.ok}/${r.attempted} credential(s).`,
        };
      }
    } else if (errPart || (r.attempted > 0 && r.ok === 0)) {
      codexRefreshNote.value = {
        ...codexRefreshNote.value,
        [providerId]: errPart || `Could not refresh membership (${r.ok}/${r.attempted}).`,
      };
    } else {
      codexRefreshNote.value = { ...codexRefreshNote.value, [providerId]: "" };
    }
    await loadCodexPlanRowsForProvider(providerId);
    if (expandedProvider.value === providerId) await loadCreds(providerId);
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

async function load() {
  try {
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
          "Gateway API upgraded: the running vibe process is still the old binary (health has no `cumulative`). Stop it, run this repo’s `vibe` build, then hard-refresh. Frontend HMR does not replace the Rust process.";
        healthMap.value = {};
        return;
      }
      map[providers.value[i].id] = body;
    }
    healthMap.value = map;
    await loadCodexPlanRowsAll();
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

async function toggleCreds(providerId: string) {
  if (expandedProvider.value === providerId) {
    expandedProvider.value = null;
    return;
  }
  expandedProvider.value = providerId;
  if (!credsByProvider.value[providerId]) {
    await loadCreds(providerId);
  }
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
  } catch (e) {
    error.value = String(e);
  }
}

async function removeCred(cred: Credential) {
  if (!confirm(`Remove credential "${cred.label}"?`)) return;
  try {
    await api.credentials.delete(cred.id);
    await loadCreds(cred.provider_id);
  } catch (e) {
    error.value = String(e);
  }
}

function isDupFingerprint(c: Credential, creds: Credential[] | undefined): boolean {
  if (!creds?.length || !c.auth_fingerprint) return false;
  return creds.filter((x) => x.auth_fingerprint === c.auth_fingerprint).length > 1;
}

function planPctClass(p: number | null | undefined): string {
  if (p == null || Number.isNaN(p)) return "bg-gray-600";
  if (p < 60) return "bg-emerald-500";
  if (p < 85) return "bg-yellow-500";
  return "bg-red-500";
}

function circuitBadge(state: string) {
  if (state === "closed") return { label: "healthy", cls: "bg-emerald-900 text-emerald-400" };
  if (state === "half-open") return { label: "probing", cls: "bg-yellow-900 text-yellow-400" };
  return { label: "circuit open", cls: "bg-red-900 text-red-400" };
}

function isCircuitResettable(state: string): boolean {
  return state === "open" || state === "half-open";
}

function rlPercent(remaining: number | null, limit: number | null): number {
  if (remaining == null || limit == null || limit === 0) return 100;
  return Math.round((remaining / limit) * 100);
}

function rlClass(pct: number) {
  if (pct > 50) return "bg-emerald-500";
  if (pct > 20) return "bg-yellow-500";
  return "bg-red-500";
}

function fmtTs(ts: number | null) {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleTimeString();
}

onMounted(load);
</script>

<template>
  <div>
    <div
      class="relative overflow-hidden rounded-2xl border border-white/[0.06] bg-gradient-to-br from-violet-600/12 via-[#1a1a1f] to-cyan-600/8 p-5 mb-6"
    >
      <div class="absolute -right-20 -top-24 size-64 rounded-full bg-violet-500/12 blur-3xl" />
      <div class="absolute -bottom-24 left-24 size-72 rounded-full bg-cyan-500/8 blur-3xl" />
      <div class="relative z-10 flex items-center justify-between flex-wrap gap-4">
        <div>
          <span class="text-xs font-mono text-violet-300 tracking-[0.15em] uppercase"
            >Gateway stack</span
          >
          <h1 class="text-3xl font-bold text-white tracking-tight">Providers</h1>
          <p class="text-sm text-zinc-500 mt-1.5 max-w-2xl leading-relaxed">
            Manage upstream AI providers, credentials, circuit health, and local client routing for
            Codex, Claude Code, and OpenCode.
          </p>
        </div>
        <div class="flex gap-2">
          <button
            @click="openImport"
            class="px-3.5 py-1.5 btn-ghost text-sm"
            title="Import from installed tools (Claude Code, Codex CLI…)"
          >
            ↓ Import local
          </button>
          <div class="relative">
            <button @click="showPresets = !showPresets" class="px-3.5 py-1.5 btn-ghost text-sm">
              ⚡ Presets
            </button>
            <div
              v-if="showPresets"
              class="absolute right-0 top-full mt-1 z-40 bg-[#1a1a1f] border border-white/[0.1] rounded-xl shadow-xl shadow-black/40 p-3 w-72"
            >
              <p class="text-xs text-zinc-500 mb-2 px-1">Quick-fill a known provider</p>
              <button
                v-for="p in PRESETS"
                :key="p.label"
                @click="applyPreset(p)"
                class="w-full text-left px-3 py-2 rounded-lg hover:bg-white/[0.04] text-sm flex items-center gap-2 transition-colors"
              >
                <span>{{ p.icon }}</span>
                <div>
                  <div class="font-medium text-white">{{ p.label }}</div>
                  <div class="text-xs text-zinc-500">priority {{ p.priority }} · {{ p.kind }}</div>
                </div>
              </button>
            </div>
          </div>
          <button @click="startAdd" class="px-3 py-1.5 btn-primary text-sm">+ Add provider</button>
        </div>
      </div>
    </div>

    <!-- Client tools -->
    <div v-if="!loading" class="mb-6 grid grid-cols-1 md:grid-cols-3 gap-3">
      <div
        v-for="{ tool, stats } in clientToolSummaries"
        :key="tool.id"
        class="group relative overflow-hidden rounded-2xl border border-white/[0.06] bg-[#1a1a1f]/80 p-4 flex flex-col gap-3 card-lift"
      >
        <div
          class="absolute inset-0 bg-gradient-to-br from-violet-500/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
        />
        <div class="relative z-10 flex items-center gap-3">
          <span
            aria-hidden="true"
            class="grid size-9 place-items-center rounded-xl bg-white/[0.04] text-lg ring-1 ring-white/[0.06]"
            >{{ tool.icon }}</span
          >
          <div class="min-w-0">
            <span class="block font-semibold text-zinc-100">{{ tool.label }}</span>
            <span class="text-[10px] uppercase tracking-wider text-zinc-600">client route</span>
          </div>
        </div>
        <code
          class="relative z-10 text-[11px] text-violet-300/95 font-mono break-all leading-snug rounded-xl border border-violet-500/15 bg-violet-500/8 px-3 py-2"
          >{{ toolProxyExample(tool) }}</code
        >
        <p class="relative z-10 text-[11px] text-zinc-500 leading-snug">
          Upstream kinds:
          <span class="text-zinc-300 font-mono">{{ tool.consumesKinds.join(" · ") }}</span>
        </p>
        <p class="relative z-10 text-xs text-zinc-400 leading-snug">{{ tool.setupHint }}</p>
        <div class="relative z-10 mt-auto pt-1 flex flex-wrap items-center gap-2 text-[11px]">
          <span
            class="px-2 py-0.5 rounded-md border"
            :class="
              stats.total === 0
                ? 'border-amber-500/30 bg-amber-500/10 text-amber-400'
                : stats.enabledCount > 0
                  ? 'border-emerald-500/25 bg-emerald-500/10 text-emerald-400'
                  : 'border-zinc-700 bg-zinc-800/80 text-zinc-500'
            "
          >
            providers:
            {{ stats.enabledCount }}
            /
            {{ stats.total }}
            enabled
          </span>
          <span v-if="stats.total === 0" class="text-amber-500/90">
            no compatible provider yet
          </span>
        </div>
      </div>
    </div>

    <div
      v-if="error"
      class="mb-4 text-sm text-red-400 bg-red-950/50 border border-red-500/20 rounded-lg px-4 py-2"
    >
      {{ error }}
    </div>

    <div v-if="loading" class="text-zinc-500 text-sm">Loading…</div>
    <div v-else-if="providers.length === 0" class="text-zinc-500 text-sm py-12 text-center">
      No providers yet. Click <strong>+ Add provider</strong> to get started.
    </div>
    <div v-else class="space-y-3">
      <div v-for="p in providers" :key="p.id" class="card-base overflow-hidden card-lift">
        <!-- Provider row -->
        <div class="px-5 py-4 bg-gradient-to-r from-white/[0.025] to-transparent">
          <div class="flex items-start gap-4">
            <div
              class="grid size-11 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-violet-500/20 to-cyan-500/10 text-lg ring-1 ring-white/[0.08]"
            >
              <span v-if="p.kind === 'anthropic'">🔮</span>
              <span v-else-if="p.kind === 'gemini-native'">✨</span>
              <span v-else-if="p.kind === 'openai-responses'">🤖</span>
              <span v-else>⚡</span>
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="font-semibold text-white">{{ p.name }}</span>
                <span
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-white/[0.08] bg-zinc-800/60 text-zinc-400"
                  >{{ p.kind }}</span
                >
                <span
                  v-if="!p.enabled"
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-amber-500/30 bg-amber-500/10 text-amber-400"
                  >paused</span
                >
                <template v-if="healthMap[p.id]?.cumulative">
                  <span
                    :class="circuitBadge(healthMap[p.id].cumulative.circuit_state).cls"
                    class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-current/20"
                  >
                    {{ circuitBadge(healthMap[p.id].cumulative.circuit_state).label }}
                  </span>
                </template>
              </div>

              <div class="text-xs text-zinc-500 mt-1 truncate font-mono">
                {{ p.base_url }} · priority {{ p.priority }}
              </div>

              <div
                v-if="healthMap[p.id]?.rolling"
                class="mt-1 flex flex-wrap gap-3 text-xs text-zinc-400"
              >
                <span class="text-zinc-500"
                  >Proxy rollup · last {{ healthMap[p.id]?.rolling_hours }}h</span
                >
                <span>{{ healthMap[p.id].rolling!.requests.toLocaleString() }} req</span>
                <span :class="healthMap[p.id].rolling!.success_rate < 0.9 ? 'text-red-400' : ''">
                  {{ (healthMap[p.id].rolling!.success_rate * 100).toFixed(1) }}% ok
                </span>
                <span v-if="healthMap[p.id].rolling!.avg_latency_ms != null">
                  {{ healthMap[p.id].rolling!.avg_latency_ms }}ms avg
                </span>
                <span v-if="(healthMap[p.id].rolling!.err_429 ?? 0) > 0" class="text-amber-400">
                  429 ×{{ healthMap[p.id].rolling!.err_429 }}
                </span>
              </div>

              <div
                v-if="healthMap[p.id]?.cumulative"
                class="mt-1 text-[10px] leading-snug text-zinc-600 flex flex-wrap gap-x-3 gap-y-0.5"
              >
                <span>
                  All-time (SQLite)
                  {{ healthMap[p.id].cumulative.total_requests.toLocaleString() }} req ·
                  {{ (healthMap[p.id].cumulative.success_rate * 100).toFixed(1) }}% ok
                  <span v-if="healthMap[p.id].cumulative.avg_latency_ms != null">
                    · {{ healthMap[p.id].cumulative.avg_latency_ms }}ms avg
                  </span>
                </span>
                <span
                  v-if="healthMap[p.id].cumulative.consecutive_failures > 0"
                  class="text-red-400"
                >
                  {{ healthMap[p.id].cumulative.consecutive_failures }} failures
                </span>
                <span
                  v-if="healthMap[p.id].cumulative.last_error"
                  class="text-red-400/90 truncate max-w-xs"
                  :title="healthMap[p.id].cumulative.last_error ?? ''"
                >
                  {{ healthMap[p.id].cumulative.last_error }}
                </span>
                <span
                  v-if="isCircuitResettable(healthMap[p.id].cumulative.circuit_state)"
                  class="text-amber-300/90"
                >
                  requests are currently being blocked by circuit breaker
                </span>
              </div>

              <div v-if="isOfficialCodexProvider(p)" class="mt-3 pt-3 border-t border-zinc-800/80">
                <div class="flex items-start justify-between gap-3 card-lift">
                  <div class="min-w-0 flex-1 space-y-1">
                    <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px]">
                      <span class="text-zinc-300 font-medium">Codex membership</span>
                      <span v-if="codexPlanRefreshing[p.id]" class="text-zinc-600">Syncing…</span>
                    </div>

                    <p
                      v-if="!codexPlanRowsByProvider[p.id]?.length"
                      class="text-[11px] text-zinc-600 leading-snug"
                    >
                      No credentials with stored OAuth yet — use
                      <strong class="text-zinc-500">Import local</strong>
                      or add a key under Keys.
                    </p>

                    <ul v-else class="space-y-1">
                      <li
                        v-for="row in codexPlanRowsByProvider[p.id]"
                        :key="row.credential_id"
                        class="text-[11px] leading-snug"
                      >
                        <span class="text-zinc-500">{{ row.label }}</span>
                        <span
                          v-if="row.plan?.summary"
                          class="text-zinc-300 font-mono ml-2 break-words"
                          :title="row.plan.source ?? ''"
                        >
                          {{ row.plan.summary }}
                        </span>
                        <span v-else class="text-zinc-600 ml-2">No snapshot</span>
                      </li>
                    </ul>

                    <p
                      v-if="codexRefreshNote[p.id]"
                      class="text-[11px] text-amber-500/90 break-words"
                    >
                      {{ codexRefreshNote[p.id] }}
                    </p>
                  </div>

                  <button
                    type="button"
                    class="shrink-0 text-[11px] text-zinc-500 hover:text-zinc-300 underline-offset-4 hover:underline disabled:opacity-40"
                    :disabled="!!codexPlanRefreshing[p.id]"
                    @click.stop="refreshCodexPlanFromChatgpt(p.id)"
                  >
                    Retry
                  </button>
                </div>
              </div>

              <div class="flex gap-2 mt-2 flex-wrap">
                <span
                  v-for="a in p.model_aliases"
                  :key="a.alias"
                  class="text-[11px] bg-zinc-800/60 text-zinc-400 rounded-lg border border-white/[0.05] px-2 py-1 font-mono"
                >
                  {{ a.alias }} → {{ a.upstream_model }}
                </span>
              </div>
            </div>

            <div class="flex gap-2 shrink-0 items-start flex-wrap justify-end">
              <button
                v-if="
                  healthMap[p.id]?.cumulative &&
                  isCircuitResettable(healthMap[p.id].cumulative.circuit_state)
                "
                @click="resetProviderCircuit(p.id)"
                :disabled="!!circuitResetBusy[p.id]"
                class="text-xs px-3 py-1.5 rounded-lg border border-amber-500/30 bg-amber-500/10 hover:bg-amber-500/15 text-amber-300 transition-colors disabled:opacity-50"
              >
                {{ circuitResetBusy[p.id] ? "Resetting…" : "Reset circuit" }}
              </button>
              <button
                type="button"
                role="switch"
                :aria-checked="p.enabled"
                :aria-label="p.enabled ? `Disable provider ${p.name}` : `Enable provider ${p.name}`"
                :disabled="!!toggleBusy[p.id]"
                class="relative w-11 h-6 shrink-0 rounded-full transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-zinc-950 disabled:opacity-50 shadow-inner"
                :class="p.enabled ? 'bg-emerald-500 shadow-emerald-900/50' : 'bg-zinc-700'"
                @click.stop="toggleProviderEnabled(p)"
              >
                <span
                  class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform"
                  :class="p.enabled ? 'translate-x-5' : 'translate-x-0'"
                />
              </button>
              <button
                @click="toggleCreds(p.id)"
                class="text-xs px-3 py-1.5 rounded-lg transition-colors border"
                :class="
                  expandedProvider === p.id
                    ? 'bg-violet-500/15 text-violet-300 border-violet-500/25'
                    : 'bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 border-white/[0.06]'
                "
              >
                Keys ({{ credsByProvider[p.id]?.length ?? "…" }})
              </button>
              <button
                @click="startEdit(p)"
                class="text-xs px-3 py-1.5 rounded-lg bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 border border-white/[0.06] transition-colors"
              >
                Edit
              </button>
              <button
                @click="remove(p.id)"
                class="text-xs px-3 py-1.5 rounded-lg bg-red-500/10 hover:bg-red-500/15 text-red-300 border border-red-500/20 transition-colors"
              >
                Remove
              </button>
            </div>
          </div>
        </div>

        <!-- Credentials section -->
        <div
          v-if="expandedProvider === p.id"
          class="border-t border-white/[0.06] px-5 py-4 bg-[#09090b]/80 rounded-b-xl"
        >
          <div class="flex items-center justify-between mb-3">
            <span class="text-sm font-medium text-zinc-300">API Keys / Credentials</span>
            <button
              @click="startAddCred(p.id)"
              class="text-xs px-3 py-1.5 rounded-lg bg-violet-500/15 hover:bg-violet-500/20 text-violet-300 border border-violet-500/25 transition-colors"
            >
              + Add key
            </button>
          </div>

          <div v-if="loadingCreds[p.id]" class="text-xs text-zinc-500">Loading…</div>
          <div
            v-else-if="!credsByProvider[p.id] || credsByProvider[p.id].length === 0"
            class="text-xs text-zinc-600"
          >
            No credentials. Provider uses its built-in
            <code class="font-mono">auth_ref</code>.
          </div>
          <div v-else class="space-y-2">
            <div
              v-for="c in credsByProvider[p.id]"
              :key="c.id"
              class="flex items-start gap-3 bg-gray-900 rounded-lg px-3 py-2.5"
            >
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="text-sm font-medium text-white">{{ c.label }}</span>
                  <span
                    v-if="c.plan_type"
                    class="text-xs px-1.5 py-0.5 rounded bg-zinc-800/60 text-zinc-400"
                  >
                    {{ c.plan_type }}
                  </span>
                  <span
                    v-if="!c.enabled"
                    class="text-xs px-1.5 py-0.5 rounded bg-yellow-900/60 text-yellow-400"
                    >disabled</span
                  >
                  <span
                    v-if="isDupFingerprint(c, credsByProvider[p.id])"
                    class="text-xs px-1.5 py-0.5 rounded bg-amber-900/50 text-amber-400"
                    title="Same auth_fingerprint as another credential on this provider"
                  >
                    Duplicate fingerprint
                  </span>
                  <span
                    v-if="c.consecutive_failures > 0"
                    class="text-xs px-1.5 py-0.5 rounded bg-red-900/60 text-red-400"
                  >
                    {{ c.consecutive_failures }} fail{{ c.consecutive_failures > 1 ? "s" : "" }}
                  </span>
                </div>

                <div class="text-xs text-zinc-500 mt-0.5">
                  <!-- OAuth credential: show expiry status -->
                  <template v-if="c.oauth_access_token || c.oauth_has_refresh">
                    <span class="text-purple-400 font-medium">OAuth</span>
                    <span v-if="c.oauth_has_refresh" class="text-green-500"> · auto-refresh ✓</span>
                    <span v-if="c.oauth_expires_at">
                      ·
                      <span
                        :class="
                          c.oauth_expires_at * 1000 < Date.now()
                            ? 'text-red-400'
                            : c.oauth_expires_at * 1000 < Date.now() + 300_000
                              ? 'text-yellow-400'
                              : 'text-zinc-400'
                        "
                      >
                        {{
                          c.oauth_expires_at * 1000 < Date.now()
                            ? "Expired"
                            : "Expires " + new Date(c.oauth_expires_at * 1000).toLocaleString()
                        }}
                      </span>
                    </span>
                  </template>
                  <!-- API Key credential: show auth_ref -->
                  <template v-else>
                    <span class="font-mono">{{ c.auth_ref ?? "(no auth_ref)" }}</span>
                  </template>
                  <span v-if="c.last_used_at"> · last used {{ fmtTs(c.last_used_at) }}</span>
                </div>

                <!-- Rate-limit bars -->
                <div
                  v-if="c.rl_requests_limit != null || c.rl_tokens_limit != null"
                  class="mt-2 space-y-1"
                >
                  <div v-if="c.rl_requests_limit != null" class="flex items-center gap-2">
                    <span class="text-xs text-zinc-600 w-16 shrink-0">Requests</span>
                    <div class="flex-1 h-1.5 rounded-full bg-zinc-800/60 overflow-hidden">
                      <div
                        :class="rlClass(rlPercent(c.rl_requests_remaining, c.rl_requests_limit))"
                        class="h-full rounded-full transition-all"
                        :style="`width: ${rlPercent(c.rl_requests_remaining, c.rl_requests_limit)}%`"
                      />
                    </div>
                    <span class="text-xs text-zinc-500 w-20 text-right shrink-0">
                      {{ c.rl_requests_remaining?.toLocaleString() }} /
                      {{ c.rl_requests_limit?.toLocaleString() }}
                    </span>
                  </div>
                  <div v-if="c.rl_tokens_limit != null" class="flex items-center gap-2">
                    <span class="text-xs text-zinc-600 w-16 shrink-0">Tokens</span>
                    <div class="flex-1 h-1.5 rounded-full bg-zinc-800/60 overflow-hidden">
                      <div
                        :class="rlClass(rlPercent(c.rl_tokens_remaining, c.rl_tokens_limit))"
                        class="h-full rounded-full transition-all"
                        :style="`width: ${rlPercent(c.rl_tokens_remaining, c.rl_tokens_limit)}%`"
                      />
                    </div>
                    <span class="text-xs text-zinc-500 w-20 text-right shrink-0">
                      {{ c.rl_tokens_remaining?.toLocaleString() }} /
                      {{ c.rl_tokens_limit?.toLocaleString() }}
                    </span>
                  </div>
                </div>

                <!-- Codex Plan usage from upstream x-codex-* headers (OAuth Codex only, best-effort) -->
                <div
                  v-if="
                    planSnapByCred[c.id] &&
                    (planSnapByCred[c.id]!.summary ||
                      planSnapByCred[c.id]!.codex_5h_used_percent != null ||
                      planSnapByCred[c.id]!.codex_7d_used_percent != null)
                  "
                  class="mt-3 rounded-lg border border-white/[0.06] bg-zinc-950/80 px-2.5 py-2"
                >
                  <div class="text-[10px] uppercase tracking-wide text-zinc-600 mb-1">
                    ChatGPT Codex Plan (upstream headers)
                  </div>
                  <div
                    v-if="planSnapByCred[c.id]!.summary"
                    class="text-xs text-zinc-400 mb-2 font-mono"
                  >
                    {{ planSnapByCred[c.id]!.summary }}
                  </div>
                  <div
                    v-if="planSnapByCred[c.id]!.codex_5h_used_percent != null"
                    class="flex items-center gap-2 mb-1"
                  >
                    <span class="text-[10px] text-zinc-600 w-8 shrink-0">5h</span>
                    <div class="flex-1 h-1.5 rounded-full bg-zinc-800/60 overflow-hidden">
                      <div
                        :class="planPctClass(planSnapByCred[c.id]!.codex_5h_used_percent)"
                        class="h-full rounded-full transition-all"
                        :style="`width: ${Math.min(100, planSnapByCred[c.id]!.codex_5h_used_percent ?? 0)}%`"
                      />
                    </div>
                    <span class="text-[10px] text-zinc-500 w-12 text-right shrink-0">
                      {{ planSnapByCred[c.id]!.codex_5h_used_percent?.toFixed(1) }}%
                    </span>
                  </div>
                  <div
                    v-if="planSnapByCred[c.id]!.codex_7d_used_percent != null"
                    class="flex items-center gap-2"
                  >
                    <span class="text-[10px] text-zinc-600 w-8 shrink-0">7d</span>
                    <div class="flex-1 h-1.5 rounded-full bg-zinc-800/60 overflow-hidden">
                      <div
                        :class="planPctClass(planSnapByCred[c.id]!.codex_7d_used_percent)"
                        class="h-full rounded-full transition-all"
                        :style="`width: ${Math.min(100, planSnapByCred[c.id]!.codex_7d_used_percent ?? 0)}%`"
                      />
                    </div>
                    <span class="text-[10px] text-zinc-500 w-12 text-right shrink-0">
                      {{ planSnapByCred[c.id]!.codex_7d_used_percent?.toFixed(1) }}%
                    </span>
                  </div>
                  <div class="text-[10px] text-zinc-600 mt-1.5">
                    Captured {{ fmtTs(planSnapByCred[c.id]!.captured_at) }} ·
                    {{ planSnapByCred[c.id]!.source }}
                  </div>
                </div>

                <div v-if="c.last_error" class="text-xs text-red-400 mt-1 truncate">
                  {{ c.last_error }}
                </div>
                <div v-if="c.notes" class="text-xs text-zinc-600 mt-0.5 italic">{{ c.notes }}</div>
              </div>

              <div class="flex gap-1.5 shrink-0">
                <button
                  @click="startEditCred(c)"
                  class="text-xs px-2 py-1 rounded bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 transition-colors"
                >
                  Edit
                </button>
                <button
                  @click="removeCred(c)"
                  class="text-xs px-2 py-1 rounded bg-red-900/40 hover:bg-red-900 text-red-400 transition-colors"
                >
                  ×
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Provider add/edit modal -->
    <div
      v-if="showForm"
      class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
    >
      <div class="bg-[#1a1a1f] border border-white/[0.08] rounded-2xl w-full max-w-lg p-6">
        <h2 class="font-semibold text-lg mb-5">{{ editTarget ? "Edit" : "Add" }} provider</h2>
        <div class="space-y-3">
          <label class="block">
            <span class="text-xs text-zinc-400">Name</span>
            <input
              v-model="form.name"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-indigo-500"
            />
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">Kind</span>
            <select
              v-model="form.kind"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm"
            >
              <option value="anthropic">Anthropic</option>
              <option value="openai-chat">OpenAI Chat (/v1/chat/completions)</option>
              <option value="openai-responses">OpenAI Responses</option>
              <option value="gemini-native">Gemini Native</option>
            </select>
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">Base URL</span>
            <input
              v-model="form.base_url"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm font-mono"
            />
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">
              Default auth ref (used when no credentials are set)
            </span>
            <input
              v-model="form.auth_ref"
              placeholder="keyring:my-key  or  env:MY_API_KEY"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm font-mono"
            />
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">Priority (lower = higher priority)</span>
            <input
              v-model.number="form.priority"
              type="number"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm"
            />
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input v-model="form.enabled" type="checkbox" class="rounded" />
            Enabled
          </label>
        </div>
        <div class="flex gap-3 mt-6 justify-end">
          <button
            @click="showForm = false"
            class="px-4 py-2 text-sm rounded-md bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 transition-colors"
          >
            Cancel
          </button>
          <button
            @click="save"
            class="px-4 py-2 text-sm rounded-md bg-violet-600 hover:bg-violet-500 font-medium transition-colors"
          >
            Save
          </button>
        </div>
      </div>
    </div>

    <!-- Credential add/edit modal -->
    <div
      v-if="showCredForm"
      class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4 overflow-y-auto"
    >
      <div class="bg-[#1a1a1f] border border-white/[0.08] rounded-2xl w-full max-w-lg p-6 my-auto">
        <h2 class="font-semibold text-lg mb-5">{{ editCred ? "Edit" : "Add" }} credential</h2>
        <div class="space-y-3">
          <label class="block">
            <span class="text-xs text-zinc-400">Label</span>
            <input
              v-model="credForm.label"
              placeholder="e.g. Codex Pro (account 2)"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-indigo-500"
            />
          </label>

          <!-- Auth mode selector -->
          <div class="flex gap-2">
            <button
              @click="credAuthMode = 'apikey'"
              :class="
                credAuthMode === 'apikey'
                  ? 'bg-violet-600 text-white'
                  : 'bg-zinc-800/60 text-zinc-400 hover:bg-zinc-700/60'
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
                  : 'bg-zinc-800/60 text-zinc-400 hover:bg-zinc-700/60'
              "
              class="flex-1 py-1.5 text-xs rounded-md transition-colors"
            >
              OAuth (ChatGPT Pro)
            </button>
          </div>

          <!-- API Key mode -->
          <template v-if="credAuthMode === 'apikey'">
            <label class="block">
              <span class="text-xs text-zinc-400">Auth ref</span>
              <input
                v-model="credForm.auth_ref"
                placeholder="keyring:my-key  or  env:MY_API_KEY  or  literal:sk-…"
                class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm font-mono"
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
              class="rounded-lg border border-dashed p-3 space-y-2 bg-indigo-950/30 transition-colors"
              :class="
                authJsonDragActive
                  ? 'border-violet-400 ring-2 ring-violet-500/40 bg-violet-950/40'
                  : 'border-indigo-600/50'
              "
              @dragover="onAuthJsonDragOver"
              @dragleave="onAuthJsonDragLeave"
              @drop="onAuthJsonDrop"
            >
              <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-2">
                <p class="text-xs text-zinc-300 font-medium">
                  Import Codex <code class="font-mono text-zinc-400">auth*.json</code>
                </p>
                <button
                  type="button"
                  @click="triggerAuthJsonFilePick"
                  class="shrink-0 px-3 py-1 text-xs rounded-md bg-zinc-700/80 hover:bg-zinc-600 text-zinc-200 transition-colors w-full sm:w-auto"
                >
                  Choose file…
                </button>
              </div>
              <p class="text-[11px] text-zinc-500 leading-snug">
                Paste JSON below or drag-and-drop a file. Parsed only in your browser; nothing is
                uploaded until you save.
              </p>
              <textarea
                v-model="authJsonPaste"
                rows="5"
                placeholder='{"auth_mode":"chatgpt","tokens":{"access_token":"eyJ…","refresh_token":"…"}}'
                class="w-full min-h-[7rem] bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-xs font-mono resize-y"
              />
              <p v-if="authJsonPasteErr" class="text-xs text-red-400">{{ authJsonPasteErr }}</p>
              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  :disabled="!authJsonPaste.trim()"
                  @click="parseAuthJsonPaste"
                  class="px-3 py-1.5 text-xs rounded-md bg-violet-600 hover:bg-violet-500 disabled:opacity-40 transition-colors"
                >
                  Parse and fill fields
                </button>
              </div>
            </div>
            <label class="block">
              <span class="text-xs text-zinc-400">Access Token</span>
              <input
                v-model="credForm.oauth_access_token"
                placeholder="eyJhbGciOiJSUzI1NiJ9…"
                class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-xs font-mono"
              />
            </label>
            <label class="block">
              <span class="text-xs text-zinc-400"
                >Refresh token (write-only; never shown after save)</span
              >
              <input
                v-model="credForm.oauth_refresh_token"
                placeholder="Leave empty to keep stored refresh token"
                type="password"
                class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-xs font-mono"
              />
            </label>
            <p class="text-xs text-zinc-500">
              Expires:
              {{
                credForm.oauth_expires_at
                  ? new Date(credForm.oauth_expires_at * 1000).toLocaleString()
                  : "Unknown (paste auth.json to detect)"
              }}
            </p>
          </template>

          <label class="block">
            <span class="text-xs text-zinc-400">Plan type (optional)</span>
            <input
              v-model="credForm.plan_type"
              placeholder="claude-pro · codex-plus · codex-pro · payg · …"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm"
            />
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">Priority</span>
            <input
              v-model.number="credForm.priority"
              type="number"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm"
            />
          </label>
          <label class="block">
            <span class="text-xs text-zinc-400">Notes</span>
            <input
              v-model="credForm.notes"
              placeholder="optional notes"
              class="mt-1 w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm"
            />
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input v-model="credForm.enabled" type="checkbox" class="rounded" />
            Enabled
          </label>
        </div>
        <div class="flex gap-3 mt-6 justify-end">
          <button
            @click="showCredForm = false"
            class="px-4 py-2 text-sm rounded-md bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 transition-colors"
          >
            Cancel
          </button>
          <button
            @click="saveCred"
            class="px-4 py-2 text-sm rounded-md bg-violet-600 hover:bg-violet-500 font-medium transition-colors"
          >
            Save
          </button>
        </div>
      </div>
    </div>
    <!-- Import local modal -->
    <div
      v-if="showImport"
      class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
      @click.self="showImport = false"
    >
      <div class="bg-[#1a1a1f] border border-white/[0.08] rounded-2xl w-full max-w-lg p-6">
        <h2 class="font-semibold text-lg mb-1">Import from local tools</h2>
        <p class="text-sm text-zinc-500 mb-5">
          Detected on your machine. Codex
          <code class="font-mono text-zinc-400">auth*.json</code> files are read once and OAuth
          tokens are stored in the gateway database — not referenced at runtime.
        </p>

        <div v-if="importLoading" class="text-sm text-zinc-500 py-4 text-center">Scanning…</div>
        <div
          v-else-if="importError"
          class="text-sm text-red-400 bg-red-950/50 border border-red-500/20/50 border border-red-500/20 rounded-lg px-4 py-2 mb-4"
        >
          {{ importError }}
        </div>
        <div
          v-else-if="localCandidates.length === 0"
          class="text-sm text-zinc-500 py-4 text-center"
        >
          No local tools found. Install Claude Code or Codex CLI first.
        </div>
        <div v-else class="space-y-3">
          <div
            v-for="c in localCandidates"
            :key="c.client"
            class="bg-zinc-800/30 rounded-xl border border-white/[0.06] p-4"
          >
            <div class="flex items-start justify-between gap-3 card-lift">
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium text-white">{{ c.name }}</span>
                  <span
                    :class="
                      c.token_ok
                        ? 'bg-emerald-900 text-emerald-400'
                        : 'bg-yellow-900 text-yellow-400'
                    "
                    class="text-xs px-1.5 py-0.5 rounded"
                  >
                    {{ c.token_ok ? "token ok" : "no token" }}
                  </span>
                  <span class="text-xs px-1.5 py-0.5 rounded bg-zinc-700/60 text-zinc-400">{{
                    c.kind
                  }}</span>
                </div>
                <div class="text-xs text-zinc-500 mt-1 font-mono truncate">{{ c.source_path }}</div>

                <!-- Extra credentials (additional accounts) -->
                <div v-if="(c.extra_credentials?.length ?? 0) > 0" class="mt-2 space-y-1">
                  <div class="text-xs text-zinc-400 font-medium">
                    + {{ c.extra_credentials?.length ?? 0 }} extra account{{
                      (c.extra_credentials?.length ?? 0) > 1 ? "s" : ""
                    }}
                    (will be added as credentials):
                  </div>
                  <div
                    v-for="ec in c.extra_credentials ?? []"
                    :key="ec.source_path"
                    class="flex items-center gap-2 text-xs text-zinc-500"
                  >
                    <span :class="ec.token_ok ? 'text-emerald-400' : 'text-yellow-400'">●</span>
                    <span class="font-mono truncate">{{ ec.label }}</span>
                  </div>
                </div>
              </div>

              <button
                @click="doImport(c.client)"
                :disabled="importingClients.has(c.client)"
                class="shrink-0 px-3 py-1.5 text-sm rounded-lg bg-violet-600 hover:bg-violet-500 disabled:opacity-50 disabled:cursor-not-allowed font-medium transition-colors"
              >
                {{ importingClients.has(c.client) ? "…" : "Import" }}
              </button>
            </div>
          </div>
        </div>

        <div class="flex justify-end mt-5">
          <button
            @click="showImport = false"
            class="px-4 py-2 text-sm rounded-md bg-zinc-800/60 hover:bg-zinc-700/60 text-zinc-300 transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

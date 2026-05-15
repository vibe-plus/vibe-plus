<script setup lang="ts">
import { ref, watch, computed } from "vue";
import type {
  Provider,
  ProviderInput,
  ProviderKind,
  Credential,
  ModelAlias,
} from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import ProviderLogo from "./provider-logo.vue";
import { credentialPrimaryAccountLabel } from "../utils/providers-display.ts";

const props = defineProps<{
  open: boolean;
  editTarget: Provider | null;
  creds: Credential[];
  loadingCreds: boolean;
  credToggleBusy: Record<string, boolean>;
  modelRefreshBusy: boolean;
  speedLabel: string;
}>();

const emit = defineEmits<{
  close: [];
  save: [form: ProviderInput];
  addCredential: [];
  reloadCreds: [];
  editCredential: [cred: Credential];
  removeCredential: [cred: Credential];
  toggleCredential: [cred: Credential];
  refreshModels: [];
}>();

type Phase = "paste" | "review";

const phase = ref<Phase>("paste");
const pasteRaw = ref("");
const parseNote = ref("");
const parseError = ref("");
const showAdvanced = ref(false);
const showAliases = ref(false);
const showCreds = ref(false);

const form = ref<ProviderInput>(emptyForm());

// ── Presets ──────────────────────────────────────────────────────────────────

interface Preset {
  label: string;
  icon: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  group_name: string;
}

const PRESETS: Preset[] = [
  {
    label: "OpenAI",
    icon: "i-lucide-bot",
    name: "OpenAI",
    kind: "openai-responses",
    base_url: "https://api.openai.com",
    group_name: "OpenAI",
  },
  {
    label: "Anthropic",
    icon: "i-lucide-sparkles",
    name: "Anthropic",
    kind: "anthropic",
    base_url: "https://api.anthropic.com",
    group_name: "Anthropic",
  },
  {
    label: "DeepSeek",
    icon: "i-lucide-brain",
    name: "DeepSeek",
    kind: "openai-chat",
    base_url: "https://api.deepseek.com",
    group_name: "DeepSeek",
  },
  {
    label: "Gemini",
    icon: "i-lucide-gem",
    name: "Google Gemini",
    kind: "gemini-native",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
    group_name: "Google",
  },
  {
    label: "Qwen",
    icon: "i-lucide-cloud",
    name: "Qwen",
    kind: "openai-chat",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    group_name: "Alibaba",
  },
  {
    label: "Moonshot",
    icon: "i-lucide-moon",
    name: "Moonshot",
    kind: "openai-chat",
    base_url: "https://api.moonshot.cn/v1",
    group_name: "Moonshot",
  },
];

const PROVIDER_KINDS: ProviderKind[] = [
  "anthropic",
  "openai-chat",
  "openai-responses",
  "gemini-native",
];

// ── Form helpers ──────────────────────────────────────────────────────────────

function emptyForm(): ProviderInput {
  return {
    name: "",
    group_name: null,
    avatar_url: null,
    kind: "openai-chat",
    base_url: "",
    auth_ref: null,
    enabled: true,
    priority: 100,
    supports_websocket: null,
    passthrough_mode: true,
    model_aliases: [],
  };
}

function providerToForm(p: Provider): ProviderInput {
  return {
    name: p.name,
    group_name: p.group_name ?? null,
    avatar_url: p.avatar_url ?? null,
    kind: p.kind,
    base_url: p.base_url,
    auth_ref: p.auth_ref,
    enabled: p.enabled,
    priority: p.priority,
    supports_websocket: p.supports_websocket ?? null,
    passthrough_mode: p.passthrough_mode ?? true,
    model_aliases: [...(p.model_aliases ?? [])],
  };
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    pasteRaw.value = "";
    parseNote.value = "";
    parseError.value = "";
    showAdvanced.value = false;
    showAliases.value = false;
    showCreds.value = false;
    if (props.editTarget) {
      phase.value = "review";
      form.value = providerToForm(props.editTarget);
    } else {
      phase.value = "paste";
      form.value = emptyForm();
    }
  },
);

// ── Parse engine ──────────────────────────────────────────────────────────────

const URL_RE = /https?:\/\/[^\s"'<>，,；;\])}]+/i;
const API_KEY_RE =
  /(?:sk-ant-[A-Za-z0-9_-]{20,}|sk-proj-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9_-]{30,}|AIza[A-Za-z0-9_-]{25,}|gsk_[A-Za-z0-9_-]{20,})/;

const WELL_KNOWN: Array<{ urlPart: string; kind: ProviderKind; name: string; base_url: string }> = [
  {
    urlPart: "api.anthropic.com",
    kind: "anthropic",
    name: "Anthropic",
    base_url: "https://api.anthropic.com",
  },
  {
    urlPart: "api.openai.com",
    kind: "openai-responses",
    name: "OpenAI",
    base_url: "https://api.openai.com",
  },
  {
    urlPart: "generativelanguage.googleapis.com",
    kind: "gemini-native",
    name: "Google Gemini",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
  },
  {
    urlPart: "api.deepseek.com",
    kind: "openai-chat",
    name: "DeepSeek",
    base_url: "https://api.deepseek.com",
  },
  {
    urlPart: "dashscope.aliyuncs.com",
    kind: "openai-chat",
    name: "Qwen",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  },
  {
    urlPart: "api.moonshot.cn",
    kind: "openai-chat",
    name: "Moonshot",
    base_url: "https://api.moonshot.cn/v1",
  },
  {
    urlPart: "open.bigmodel.cn",
    kind: "openai-chat",
    name: "Zhipu",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
  },
  {
    urlPart: "api.groq.com",
    kind: "openai-chat",
    name: "Groq",
    base_url: "https://api.groq.com/openai",
  },
  {
    urlPart: "openrouter.ai",
    kind: "openai-chat",
    name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
  },
];

const KEY_PREFIXES: Array<{ prefix: string; kind: ProviderKind; name: string; base_url: string }> =
  [
    {
      prefix: "sk-ant-",
      kind: "anthropic",
      name: "Anthropic",
      base_url: "https://api.anthropic.com",
    },
    {
      prefix: "AIza",
      kind: "gemini-native",
      name: "Google Gemini",
      base_url: "https://generativelanguage.googleapis.com/v1beta",
    },
    { prefix: "gsk_", kind: "openai-chat", name: "Groq", base_url: "https://api.groq.com/openai" },
  ];

const ENV_KEY_MAP: Record<string, { kind: ProviderKind; name: string; base_url: string }> = {
  OPENAI_API_KEY: { kind: "openai-responses", name: "OpenAI", base_url: "https://api.openai.com" },
  ANTHROPIC_API_KEY: {
    kind: "anthropic",
    name: "Anthropic",
    base_url: "https://api.anthropic.com",
  },
  GEMINI_API_KEY: {
    kind: "gemini-native",
    name: "Google Gemini",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
  },
  DEEPSEEK_API_KEY: { kind: "openai-chat", name: "DeepSeek", base_url: "https://api.deepseek.com" },
  DASHSCOPE_API_KEY: {
    kind: "openai-chat",
    name: "Qwen",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  },
  MOONSHOT_API_KEY: {
    kind: "openai-chat",
    name: "Moonshot",
    base_url: "https://api.moonshot.cn/v1",
  },
  ZHIPU_API_KEY: {
    kind: "openai-chat",
    name: "Zhipu",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
  },
};

function detectFromUrl(url: string): { kind: ProviderKind; name: string; base_url: string } {
  const lower = url.toLowerCase();
  for (const p of WELL_KNOWN) {
    if (lower.includes(p.urlPart)) return { kind: p.kind, name: p.name, base_url: p.base_url };
  }
  try {
    const host = new URL(url.includes("://") ? url : `https://${url}`).hostname;
    return { kind: "openai-chat", name: host, base_url: url };
  } catch {
    return { kind: "openai-chat", name: "Custom", base_url: url };
  }
}

function detectFromKey(key: string): { kind: ProviderKind; name: string; base_url: string } | null {
  for (const m of KEY_PREFIXES) {
    if (key.startsWith(m.prefix)) return { kind: m.kind, name: m.name, base_url: m.base_url };
  }
  if (key.startsWith("sk-"))
    return { kind: "openai-responses", name: "OpenAI", base_url: "https://api.openai.com" };
  return null;
}

function tryParse(raw: string): boolean {
  parseError.value = "";
  parseNote.value = "";
  const trimmed = raw.trim();
  if (!trimmed) return false;

  // JSON object
  if (trimmed.startsWith("{")) {
    try {
      const obj = JSON.parse(trimmed) as Record<string, unknown>;

      // ProviderInput-shaped JSON
      if (
        typeof obj.base_url === "string" ||
        typeof obj.kind === "string" ||
        typeof obj.name === "string"
      ) {
        const next = emptyForm();
        if (typeof obj.name === "string") next.name = obj.name.trim();
        if (typeof obj.group_name === "string") next.group_name = obj.group_name.trim() || null;
        if (typeof obj.kind === "string" && PROVIDER_KINDS.includes(obj.kind as ProviderKind)) {
          next.kind = obj.kind as ProviderKind;
        }
        if (typeof obj.base_url === "string") {
          next.base_url = obj.base_url.trim();
          const det = detectFromUrl(next.base_url);
          if (!next.name) next.name = det.name;
          if (!obj.kind) next.kind = det.kind;
        }
        if (typeof obj.auth_ref === "string") next.auth_ref = obj.auth_ref.trim() || null;
        if (typeof obj.priority === "number") next.priority = Math.round(obj.priority);
        if (typeof obj.enabled === "boolean") next.enabled = obj.enabled;
        if (typeof obj.passthrough_mode === "boolean") next.passthrough_mode = obj.passthrough_mode;
        if (Array.isArray(obj.model_aliases)) {
          const aliases: ModelAlias[] = [];
          for (const row of obj.model_aliases) {
            if (row && typeof row === "object") {
              const r = row as Record<string, unknown>;
              if (typeof r.alias === "string" && typeof r.upstream_model === "string") {
                const alias = r.alias.trim();
                const upstream = r.upstream_model.trim();
                if (alias && upstream) aliases.push({ alias, upstream_model: upstream });
              }
            }
          }
          if (aliases.length) next.model_aliases = aliases;
        }
        form.value = next;
        parseNote.value = "已从 JSON 解析供应商配置";
        return true;
      }

      // Env-key JSON (e.g. { OPENAI_API_KEY: "sk-..." })
      for (const [envKey, preset] of Object.entries(ENV_KEY_MAP)) {
        const val = obj[envKey];
        if (typeof val === "string" && val.trim()) {
          const next = emptyForm();
          next.name = preset.name;
          next.kind = preset.kind;
          next.base_url = preset.base_url;
          next.auth_ref = `literal:${val.trim()}`;
          form.value = next;
          parseNote.value = `已从 ${envKey} 识别出 ${preset.name}`;
          return true;
        }
      }
    } catch {
      // not valid JSON — continue to other patterns
    }
  }

  // env file line: KEY=value
  const envLineMatch = trimmed.match(/^([A-Z][A-Z0-9_]*)=(.+)$/m);
  if (envLineMatch) {
    const envKey = envLineMatch[1];
    const envVal = envLineMatch[2].trim().replace(/^["']|["']$/g, "");
    const preset = ENV_KEY_MAP[envKey];
    if (preset && envVal) {
      const next = emptyForm();
      next.name = preset.name;
      next.kind = preset.kind;
      next.base_url = preset.base_url;
      next.auth_ref = `literal:${envVal}`;
      form.value = next;
      parseNote.value = `已从 ${envKey} 识别出 ${preset.name}`;
      return true;
    }
  }

  // URL + optional API key
  const urlMatch = trimmed.match(URL_RE);
  const keyMatch = trimmed.match(API_KEY_RE);
  if (urlMatch) {
    const url = urlMatch[0].replace(/[),.;，。；、\])}]+$/, "");
    const det = detectFromUrl(url);
    const next = emptyForm();
    next.name = det.name;
    next.kind = det.kind;
    next.base_url = det.base_url;
    if (keyMatch) {
      next.auth_ref = `literal:${keyMatch[0]}`;
      parseNote.value = `已从 URL + Key 识别出 ${det.name}`;
    } else {
      parseNote.value = `已从 URL 识别出 ${det.name}`;
    }
    form.value = next;
    return true;
  }

  // Bare API key
  if (keyMatch) {
    const key = keyMatch[0];
    const det = detectFromKey(key);
    if (det) {
      const next = emptyForm();
      next.name = det.name;
      next.kind = det.kind;
      next.base_url = det.base_url;
      next.auth_ref = `literal:${key}`;
      form.value = next;
      parseNote.value = `已从 API Key 识别出 ${det.name}`;
      return true;
    }
  }

  parseError.value = "无法识别。请粘贴 JSON 配置、API Key、Base URL 或 env 变量。";
  return false;
}

function onTextareaInput(ev: Event) {
  pasteRaw.value = (ev.target as HTMLTextAreaElement).value;
}

function onTextareaPaste(ev: ClipboardEvent) {
  const text = ev.clipboardData?.getData("text") ?? "";
  if (!text.trim()) return;
  // Let the browser populate the textarea, then parse
  requestAnimationFrame(() => {
    pasteRaw.value = text;
    if (tryParse(text)) phase.value = "review";
  });
}

async function readClipboard() {
  try {
    const text = await navigator.clipboard.readText();
    if (!text.trim()) {
      parseError.value = "剪贴板为空";
      return;
    }
    pasteRaw.value = text;
    if (tryParse(text)) phase.value = "review";
  } catch {
    parseError.value = "无法读取剪贴板，请直接粘贴到文本框";
  }
}

function applyPreset(p: Preset) {
  form.value = {
    name: p.name,
    group_name: p.group_name,
    kind: p.kind,
    base_url: p.base_url,
    auth_ref: null,
    avatar_url: null,
    enabled: true,
    priority: 100,
    supports_websocket: null,
    passthrough_mode: true,
    model_aliases: [],
  };
  parseNote.value = `已应用「${p.label}」预设`;
  phase.value = "review";
}

function backToPaste() {
  phase.value = "paste";
  parseNote.value = "";
  parseError.value = "";
}

function addAliasRow() {
  form.value.model_aliases = [...form.value.model_aliases, { alias: "", upstream_model: "" }];
}

function removeAliasRow(index: number) {
  form.value.model_aliases = form.value.model_aliases.filter((_, i) => i !== index);
}

function kindLabel(kind: ProviderKind): string {
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

const title = computed(() => {
  if (props.editTarget) return "编辑供应商";
  return phase.value === "review" ? "确认配置" : "添加供应商";
});

function doParseAndAdvance() {
  if (tryParse(pasteRaw.value)) phase.value = "review";
}

function normalizeAuthRef(raw: string | null | undefined): string | null {
  const v = raw?.trim();
  if (!v) return null;
  // Already has a known prefix — pass through unchanged
  if (v.startsWith("literal:") || v.startsWith("env:") || v.startsWith("keyring:")) return v;
  // Looks like a raw API key — wrap with literal:
  if (
    v.startsWith("sk-") ||
    v.startsWith("AIza") ||
    v.startsWith("gsk_") ||
    v.startsWith("ck-") ||
    v.startsWith("dk-")
  )
    return `literal:${v}`;
  // Anything else (e.g. a JWT) — pass through
  return v;
}

function handleSave() {
  const payload: ProviderInput = {
    ...form.value,
    name: form.value.name.trim(),
    group_name: form.value.group_name?.trim() || null,
    avatar_url: form.value.avatar_url?.trim() || null,
    base_url: form.value.base_url.trim(),
    auth_ref: normalizeAuthRef(form.value.auth_ref),
    model_aliases: form.value.model_aliases
      .map((a) => ({ alias: a.alias.trim(), upstream_model: a.upstream_model.trim() }))
      .filter((a) => a.alias && a.upstream_model),
  };
  emit("save", payload);
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="vp-modal-backdrop z-[110] overflow-y-auto py-6"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="'provider-smart-title'"
      @click.self="emit('close')"
    >
      <div
        class="vp-modal-panel my-auto flex max-h-[min(94dvh,52rem)] w-[min(100vw-1rem,42rem)] flex-col"
        @click.stop
      >
        <!-- Header -->
        <div class="vp-modal-header border-b border-vp-border/70">
          <div class="flex min-w-0 flex-1 items-center gap-3">
            <ProviderLogo
              v-if="phase === 'review'"
              :kind="form.kind"
              :avatar-url="form.avatar_url ?? null"
              :provider-name="form.name || 'provider'"
              size-class="size-10"
              icon-size-class="size-5"
            />
            <div class="min-w-0 flex-1">
              <h2 id="provider-smart-title" class="text-base font-semibold text-vp-text">
                {{ title }}
              </h2>
              <p v-if="parseNote && phase === 'review'" class="mt-0.5 text-xs text-emerald-600">
                <VpIcon name="check" size-class="mr-0.5 inline size-3.5 align-middle" />
                {{ parseNote }}
              </p>
            </div>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            aria-label="关闭"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>

        <!-- ── PASTE PHASE ── -->
        <div v-if="phase === 'paste'" class="flex flex-1 flex-col overflow-y-auto px-5 py-5">
          <p class="mb-3 text-sm text-vp-muted">
            粘贴任意配置，系统自动识别并填好所有字段，你只需要确认。
          </p>

          <textarea
            :value="pasteRaw"
            rows="6"
            class="w-full resize-none rounded-xl border border-vp-border bg-white px-4 py-3 font-mono text-sm text-slate-900 placeholder:text-slate-400 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
            placeholder="粘贴 JSON 配置 / API Key / Base URL / env 变量..."
            @input="onTextareaInput"
            @paste="onTextareaPaste"
          />

          <p v-if="parseError" class="mt-2 text-xs text-red-600">{{ parseError }}</p>

          <div class="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg border border-vp-border bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-50"
              @click="readClipboard"
            >
              <VpIcon name="clipboard" size-class="size-4" />
              从剪贴板读取
            </button>
            <button
              v-if="pasteRaw.trim()"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg bg-violet-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-violet-700"
              @click="doParseAndAdvance"
            >
              <VpIcon name="sparkles" size-class="size-4" />
              解析
            </button>
          </div>

          <!-- Presets -->
          <div class="mt-5">
            <p class="mb-2.5 text-xs font-medium uppercase tracking-wide text-vp-muted">
              或选择预设
            </p>
            <div class="grid grid-cols-3 gap-2">
              <button
                v-for="preset in PRESETS"
                :key="preset.label"
                type="button"
                class="flex items-center gap-2 rounded-xl border border-vp-border bg-white px-3 py-2.5 text-sm text-slate-700 transition-colors hover:border-violet-300 hover:bg-violet-50"
                @click="applyPreset(preset)"
              >
                <span :class="[preset.icon, 'size-4 shrink-0 text-slate-500']" aria-hidden="true" />
                <span class="truncate font-medium">{{ preset.label }}</span>
              </button>
            </div>
          </div>

          <!-- Manual link -->
          <button
            type="button"
            class="mt-4 self-start text-xs text-vp-muted hover:text-vp-text hover:underline"
            @click="phase = 'review'"
          >
            跳过，手动填写 →
          </button>
        </div>

        <!-- ── REVIEW PHASE ── -->
        <div v-else class="flex flex-1 flex-col gap-4 overflow-y-auto px-5 py-5">
          <!-- Core fields -->
          <section class="space-y-3 rounded-2xl border border-vp-border bg-white p-4">
            <h3 class="text-xs font-semibold uppercase tracking-wide text-vp-muted">基本信息</h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <label>
                <span class="mb-1 block text-xs font-medium text-slate-600">名称</span>
                <input
                  v-model="form.name"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                  placeholder="供应商名称"
                />
              </label>
              <!-- Protocol — visible by default, crucial for compatibility -->
              <label>
                <span class="mb-1 block text-xs font-medium text-slate-600">协议</span>
                <select
                  v-model="form.kind"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                >
                  <option v-for="k in PROVIDER_KINDS" :key="k" :value="k">
                    {{ kindLabel(k) }}
                  </option>
                </select>
              </label>
              <label class="sm:col-span-2">
                <span class="mb-1 block text-xs font-medium text-slate-600">Base URL</span>
                <input
                  v-model="form.base_url"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 font-mono text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                  placeholder="https://api.example.com"
                />
              </label>
              <label class="sm:col-span-2">
                <span class="mb-1 block text-xs font-medium text-slate-600">API Key</span>
                <input
                  v-model="form.auth_ref"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 font-mono text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                  placeholder="sk-… 直接粘贴即可，自动识别格式"
                />
                <p class="mt-1 text-[11px] text-vp-muted">
                  原始 Key（sk-/sk-ant-/AIza* 等）直接粘贴，保存时自动加
                  <code>literal:</code> 前缀。
                </p>
              </label>
            </div>
          </section>

          <!-- Advanced -->
          <section class="rounded-2xl border border-vp-border bg-white">
            <button
              type="button"
              class="flex w-full items-center justify-between px-4 py-3 text-sm font-medium text-slate-700 hover:bg-slate-50"
              @click="showAdvanced = !showAdvanced"
            >
              <span class="flex items-center gap-2">
                <VpIcon name="settings" size-class="size-4 text-slate-400" />
                高级设置
              </span>
              <VpIcon
                name="chevron-down"
                :class="showAdvanced ? 'rotate-180' : ''"
                size-class="size-4 text-slate-400 transition-transform"
              />
            </button>
            <div v-if="showAdvanced" class="border-t border-vp-border/60 px-4 pb-4 pt-3">
              <div class="grid gap-3 sm:grid-cols-2">
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">优先级</span>
                  <input
                    v-model.number="form.priority"
                    type="number"
                    class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm focus:border-violet-400 focus:outline-none"
                  />
                </label>
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">分组名</span>
                  <input
                    v-model="form.group_name"
                    class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm focus:border-violet-400 focus:outline-none"
                    placeholder="e.g. official / 自用"
                  />
                </label>
                <label
                  class="flex cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.enabled" type="checkbox" class="rounded" />
                  <span>启用此供应商</span>
                </label>
                <label
                  class="flex cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.passthrough_mode" type="checkbox" class="rounded" />
                  <span>透传模型名称</span>
                </label>
              </div>
            </div>
          </section>

          <!-- Aliases -->
          <section class="rounded-2xl border border-vp-border bg-white">
            <button
              type="button"
              class="flex w-full items-center justify-between px-4 py-3 text-sm font-medium text-slate-700 hover:bg-slate-50"
              @click="showAliases = !showAliases"
            >
              <span class="flex items-center gap-2">
                <VpIcon name="route" size-class="size-4 text-slate-400" />
                模型别名
                <span
                  v-if="form.model_aliases.length"
                  class="rounded-full bg-slate-100 px-1.5 py-0.5 text-[10px] text-slate-600"
                >
                  {{ form.model_aliases.length }}
                </span>
              </span>
              <VpIcon
                name="chevron-down"
                :class="showAliases ? 'rotate-180' : ''"
                size-class="size-4 text-slate-400 transition-transform"
              />
            </button>
            <div v-if="showAliases" class="border-t border-vp-border/60 px-4 pb-4 pt-3 space-y-2">
              <div
                v-if="!form.model_aliases.length"
                class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
              >
                无别名。若上游模型 ID 与客户端请求的不同时才需要配置。
              </div>
              <div
                v-for="(alias, index) in form.model_aliases"
                :key="index"
                class="grid grid-cols-[1fr_1fr_auto] gap-2"
              >
                <input
                  v-model="alias.alias"
                  class="rounded-lg border border-slate-200 px-2.5 py-1.5 text-sm focus:border-violet-400 focus:outline-none"
                  placeholder="客户端别名"
                />
                <input
                  v-model="alias.upstream_model"
                  class="rounded-lg border border-slate-200 px-2.5 py-1.5 font-mono text-sm focus:border-violet-400 focus:outline-none"
                  placeholder="上游 model ID"
                />
                <button
                  type="button"
                  class="rounded-lg border border-red-200 px-2.5 py-1.5 text-xs text-red-700 hover:bg-red-50"
                  @click="removeAliasRow(index)"
                >
                  <VpIcon name="trash-2" size-class="size-3.5" />
                </button>
              </div>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-3 py-1.5 text-xs text-slate-700 hover:bg-slate-50"
                @click="addAliasRow"
              >
                <VpIcon name="plus" size-class="size-3.5" />
                添加行
              </button>
            </div>
          </section>

          <!-- Credentials (edit mode only) -->
          <section v-if="editTarget" class="rounded-2xl border border-vp-border bg-white">
            <button
              type="button"
              class="flex w-full items-center justify-between px-4 py-3 text-sm font-medium text-slate-700 hover:bg-slate-50"
              @click="showCreds = !showCreds"
            >
              <span class="flex items-center gap-2">
                <VpIcon name="key" size-class="size-4 text-slate-400" />
                凭证
                <span
                  v-if="creds.length"
                  class="rounded-full bg-slate-100 px-1.5 py-0.5 text-[10px] text-slate-600"
                >
                  {{ creds.length }}
                </span>
              </span>
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  class="rounded-md bg-teal-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-teal-700"
                  @click.stop="emit('addCredential')"
                >
                  添加
                </button>
                <VpIcon
                  name="chevron-down"
                  :class="showCreds ? 'rotate-180' : ''"
                  size-class="size-4 text-slate-400 transition-transform"
                />
              </div>
            </button>
            <div v-if="showCreds" class="border-t border-vp-border/60 px-4 pb-4 pt-3">
              <div v-if="loadingCreds" class="py-3 text-center text-xs text-slate-500">加载中…</div>
              <div
                v-else-if="!creds.length"
                class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
              >
                暂无凭证。
              </div>
              <ul v-else class="space-y-2">
                <li
                  v-for="cred in creds"
                  :key="cred.id"
                  class="flex flex-wrap items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2"
                >
                  <span class="flex-1 truncate text-sm font-medium text-slate-900">
                    {{ credentialPrimaryAccountLabel(cred) }}
                  </span>
                  <span
                    v-if="cred.oauth_access_token || cred.oauth_has_refresh"
                    class="rounded bg-violet-100 px-1.5 py-0.5 text-[10px] text-violet-800"
                    >OAuth</span
                  >
                  <div class="flex gap-1.5">
                    <button
                      type="button"
                      class="rounded-md border border-slate-200 px-2 py-1 text-[11px] text-slate-700 hover:bg-white disabled:opacity-50"
                      :disabled="!!credToggleBusy[cred.id]"
                      @click="emit('toggleCredential', cred)"
                    >
                      {{ cred.enabled ? "禁用" : "启用" }}
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-slate-200 px-2 py-1 text-[11px] text-slate-700 hover:bg-white"
                      @click="emit('editCredential', cred)"
                    >
                      编辑
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-red-200 px-2 py-1 text-[11px] text-red-700 hover:bg-red-50"
                      @click="emit('removeCredential', cred)"
                    >
                      删除
                    </button>
                  </div>
                </li>
              </ul>
            </div>
          </section>

          <!-- New-provider hint about credentials -->
          <p
            v-if="!editTarget"
            class="rounded-xl border border-amber-200 bg-amber-50/60 px-3 py-2.5 text-xs text-amber-900"
          >
            保存后可在此处添加凭证（API Key 等）。
          </p>
        </div>

        <!-- Footer -->
        <div
          class="flex flex-wrap items-center gap-2 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-5 py-3 sm:justify-end"
        >
          <button
            v-if="phase === 'review' && !editTarget"
            type="button"
            class="btn-ghost inline-flex flex-1 items-center justify-center gap-1.5 px-3 py-2 text-sm sm:flex-none"
            @click="backToPaste"
          >
            <VpIcon name="rotate-ccw" size-class="size-4" />
            重新粘贴
          </button>
          <button
            type="button"
            class="btn-ghost inline-flex flex-1 items-center justify-center gap-1.5 px-4 py-2 text-sm sm:flex-none"
            @click="emit('close')"
          >
            取消
          </button>
          <button
            v-if="phase === 'review'"
            type="button"
            class="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-700 sm:flex-none"
            @click="handleSave"
          >
            <VpIcon name="check" size-class="size-4" />
            {{ editTarget ? "保存修改" : "创建供应商" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

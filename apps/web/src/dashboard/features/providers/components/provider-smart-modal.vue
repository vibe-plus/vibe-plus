<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { useI18n } from "vue-i18n";
import type {
  Provider,
  ProviderInput,
  ProviderKind,
  ProviderProtocol,
  Credential,
  ModelAlias,
} from "../../../api/client.ts";
import { api } from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderLogo from "../../../components/provider-logo.vue";
import { credentialPrimaryAccountLabel } from "../../../utils/providers-display.ts";
import { providerHasKind } from "../../../utils/provider-protocols.ts";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  editTarget: Provider | null;
  existingProviders: Provider[];
  creds: Credential[];
  loadingCreds: boolean;
  credToggleBusy: Record<string, boolean>;
  modelRefreshBusy: boolean;
  speedLabel: string;
}>();

/** API keys are never stored on the provider — only as credentials. */
const pendingCredentialAuthRef = ref<string | null>(null);
/** When set, Save only adds a credential to this provider (paste-key flow). */
const credentialTargetId = ref<string | null>(null);

const emit = defineEmits<{
  close: [];
  save: [form: ProviderInput, credentialAuthRef: string | null];
  saveCredentialOnly: [providerId: string, credentialAuthRef: string];
  addCredential: [];
  reloadCreds: [];
  editCredential: [cred: Credential];
  removeCredential: [cred: Credential];
  toggleCredential: [cred: Credential];
  refreshModels: [];
}>();

type Phase = "paste" | "review";
type ProviderInputForm = Omit<ProviderInput, "protocols"> & { protocols: ProviderProtocol[] };
type ProviderDetection = {
  kind: ProviderKind;
  name: string;
  host: string;
  base_url: string;
  protocols: ProviderProtocol[];
};

const phase = ref<Phase>("paste");
const pasteRaw = ref("");
const parseNote = ref("");
const parseError = ref("");
const showAdvanced = ref(false);
const showAliases = ref(false);
const showCreds = ref(false);
const probeBusy = ref(false);
const probeError = ref("");
const probedProvider = ref<ProviderDetection | null>(null);

const form = ref<ProviderInputForm>(emptyForm());

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
    icon: "i-[lucide--bot]",
    name: "OpenAI",
    kind: "openai-responses",
    base_url: "https://api.openai.com",
    group_name: "OpenAI",
  },
  {
    label: "Anthropic",
    icon: "i-[lucide--sparkles]",
    name: "Anthropic",
    kind: "anthropic",
    base_url: "https://api.anthropic.com",
    group_name: "Anthropic",
  },
  {
    label: "DeepSeek",
    icon: "i-[lucide--brain]",
    name: "DeepSeek",
    kind: "openai-chat",
    base_url: "https://api.deepseek.com",
    group_name: "DeepSeek",
  },
  {
    label: "Gemini",
    icon: "i-[lucide--gem]",
    name: "Google Gemini",
    kind: "gemini-native",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
    group_name: "Google",
  },
  {
    label: "Qwen",
    icon: "i-[lucide--cloud]",
    name: "Qwen",
    kind: "openai-chat",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    group_name: "Alibaba",
  },
  {
    label: "Moonshot",
    icon: "i-[lucide--moon]",
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

function emptyForm(): ProviderInputForm {
  return {
    name: "",
    group_name: null,
    avatar_url: null,
    kind: "openai-responses",
    base_url: "",
    protocols: [],
    host: null,
    auth_ref: null,
    enabled: true,
    priority: 100,
    supports_websocket: null,
    passthrough_mode: true,
    model_aliases: [],
  };
}

function providerToForm(p: Provider): ProviderInputForm {
  return {
    name: p.name,
    group_name: p.group_name ?? null,
    avatar_url: p.avatar_url ?? null,
    kind: p.kind,
    base_url: p.base_url,
    protocols: [...(p.protocols ?? [])],
    host: p.host ?? hostFromUrlLike(p.base_url),
    auth_ref: null,
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
    probeError.value = "";
    probedProvider.value = null;
    pendingCredentialAuthRef.value = null;
    credentialTargetId.value = null;
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

watch(
  () => form.value.host,
  (host) => {
    if (!host?.trim()) return;
    if (!form.value.name.trim()) {
      const det = detectProviderFromHostInput(host);
      form.value.name = det.name || form.value.name;
    }
  },
);

let probeSeq = 0;
watch(
  () => form.value.host,
  (host) => {
    const seq = ++probeSeq;
    probeError.value = "";
    probedProvider.value = null;
    const trimmed = host?.trim() ?? "";
    if (!trimmed) return;
    const local = detectProviderFromHostInput(trimmed);
    if (!local.base_url) {
      probeError.value = t("probe.invalidHost");
      return;
    }
    probeBusy.value = true;
    window.setTimeout(() => {
      if (seq !== probeSeq) return;
      api.providers
        .probe(trimmed)
        .then((res) => {
          if (seq !== probeSeq) return;
          if (!res.protocols.length) {
            probeError.value = t("probe.noEndpoint");
            return;
          }
          const protocols = res.protocols.map((proto) => ({
            kind: proto.kind,
            base_url: proto.base_url,
            model_aliases: [...form.value.model_aliases],
          }));
          probedProvider.value = {
            kind: protocols[0]?.kind ?? local.kind,
            name: res.display_name || local.name,
            host: res.host,
            base_url: protocols[0]?.base_url ?? local.base_url,
            protocols,
          };
          if (!form.value.name.trim()) form.value.name = res.display_name || local.name;
        })
        .catch(() => {
          if (seq !== probeSeq) return;
          probeError.value = t("probe.failed");
        })
        .finally(() => {
          if (seq === probeSeq) probeBusy.value = false;
        });
    }, 350);
  },
);

// ── Parse engine ──────────────────────────────────────────────────────────────

const URL_RE = /https?:\/\/[^\s"'<>，,；;\])}]+/i;
const API_KEY_RE =
  /(?:sk-ant-[A-Za-z0-9_-]{20,}|sk-proj-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9_-]{30,}|AIza[A-Za-z0-9_-]{25,}|gsk_[A-Za-z0-9_-]{20,})/;
const CODEX_CONFIG_BASE_URL_RE = /base_url\s*=\s*["']([^"']+)["']/i;
const CODEX_CONFIG_WIRE_API_RE = /wire_api\s*=\s*["']([^"']+)["']/i;

const WELL_KNOWN: Array<{
  urlPart: string;
  kind: ProviderKind;
  name: string;
  base_url: string;
}> = [
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

const KEY_PREFIXES: Array<{
  prefix: string;
  kind: ProviderKind;
  name: string;
  base_url: string;
}> = [
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
  {
    prefix: "gsk_",
    kind: "openai-chat",
    name: "Groq",
    base_url: "https://api.groq.com/openai",
  },
];

const ENV_KEY_MAP: Record<string, { kind: ProviderKind; name: string; base_url: string }> = {
  OPENAI_API_KEY: {
    kind: "openai-responses",
    name: "OpenAI",
    base_url: "https://api.openai.com",
  },
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
  DEEPSEEK_API_KEY: {
    kind: "openai-chat",
    name: "DeepSeek",
    base_url: "https://api.deepseek.com",
  },
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

function hostFromUrlLike(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  try {
    const url = new URL(trimmed.includes("://") ? trimmed : `https://${trimmed}`);
    if (!/^[a-z0-9.-]+$/i.test(url.hostname)) return null;
    return url.port ? `${url.hostname}:${url.port}` : url.hostname;
  } catch {
    return null;
  }
}

function normalizeHostInput(input: string): string | null {
  return hostFromUrlLike(input)?.toLowerCase() ?? null;
}

function detectProviderFromHostInput(input: string): ProviderDetection {
  const raw = input.trim();
  if (!raw) {
    return {
      kind: "openai-responses",
      name: "",
      host: "",
      base_url: "",
      protocols: [],
    };
  }
  const host = hostFromUrlLike(raw);
  if (!host) {
    return {
      kind: "openai-responses",
      name: raw,
      host: raw,
      base_url: "",
      protocols: [],
    };
  }
  const normalized = host?.toLowerCase() ?? raw.toLowerCase();

  for (const p of WELL_KNOWN) {
    if (normalized.includes(p.urlPart)) {
      const protocols = multiProtocolForBase(p.base_url, p.kind);
      return {
        kind: p.kind,
        name: p.name,
        host: host ?? p.urlPart,
        base_url: p.base_url,
        protocols,
      };
    }
  }

  const baseUrl = `https://${host}`;
  const kind = "openai-responses" as ProviderKind;
  const name = host ?? "Custom";
  const protocols = multiProtocolForBase(baseUrl, kind);
  return {
    kind,
    name,
    host: host ?? hostFromUrlLike(baseUrl) ?? "localhost",
    base_url: baseUrl,
    protocols,
  };
}

function dedupeProtocols(protocols: ProviderProtocol[]): ProviderProtocol[] {
  const out: ProviderProtocol[] = [];
  const seen = new Set<string>();
  for (const proto of protocols) {
    const baseUrl = proto.base_url.trim();
    if (!baseUrl) continue;
    const key = `${proto.kind}::${baseUrl.replace(/\/+$/, "").toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({
      ...proto,
      base_url: baseUrl,
      model_aliases: [...(proto.model_aliases ?? [])],
    });
  }
  return out;
}

function multiProtocolForBase(
  baseUrl: string,
  primaryKind: ProviderKind = "openai-responses",
  aliases: ModelAlias[] = [],
): ProviderProtocol[] {
  if (primaryKind === "openai-responses" || primaryKind === "openai-chat") {
    return dedupeProtocols([
      {
        kind: "openai-responses",
        base_url: baseUrl,
        model_aliases: [...aliases],
      },
      { kind: "openai-chat", base_url: baseUrl, model_aliases: [...aliases] },
    ]);
  }
  return [{ kind: primaryKind, base_url: baseUrl, model_aliases: [...aliases] }];
}

function applyProviderDetection(next: ProviderInputForm, det: ProviderDetection) {
  next.name = next.name || det.name;
  next.kind = det.kind;
  next.base_url = det.base_url;
  next.host = det.host;
  next.protocols = det.protocols.map((proto) => ({
    ...proto,
    model_aliases: [...next.model_aliases],
  }));
}

function providerHostKey(provider: Pick<Provider, "host" | "base_url">): string | null {
  return normalizeHostInput(provider.host ?? hostFromUrlLike(provider.base_url) ?? "");
}

function findProvidersForDetection(det: ProviderDetection): Provider[] {
  const host = normalizeHostInput(det.host) ?? "";
  return props.existingProviders.filter((p) => {
    if (!providerHasKind(p, det.kind)) return false;
    return providerHostKey(p) === host;
  });
}

function tryAttachCredentialToExisting(key: string, det: ProviderDetection): boolean {
  if (props.editTarget) {
    stashCredentialKey(key);
    form.value = providerToForm(props.editTarget);
    parseNote.value = t("parseNotes.addKeyToEditing");
    phase.value = "review";
    return true;
  }
  const matches = findProvidersForDetection(det);
  if (matches.length === 1) {
    stashCredentialKey(key);
    credentialTargetId.value = matches[0].id;
    form.value = providerToForm(matches[0]);
    parseNote.value = t("parseNotes.addKeyToExisting", { name: matches[0].name });
    phase.value = "review";
    return true;
  }
  return false;
}

function parseCodexConfig(raw: string): { base_url: string; kind: ProviderKind } | null {
  const baseUrl = raw.match(CODEX_CONFIG_BASE_URL_RE)?.[1]?.trim();
  if (!baseUrl) return null;
  const wire = raw.match(CODEX_CONFIG_WIRE_API_RE)?.[1]?.trim().toLowerCase();
  return {
    base_url: baseUrl,
    kind: wire === "chat" || wire === "chat_completions" ? "openai-chat" : "openai-responses",
  };
}

function stashCredentialKey(raw: string) {
  pendingCredentialAuthRef.value = normalizeAuthRef(raw);
}

function detectFromKey(key: string): ProviderDetection | null {
  for (const m of KEY_PREFIXES) {
    if (key.startsWith(m.prefix)) {
      const det = detectProviderFromHostInput(m.base_url);
      return { ...det, kind: m.kind, name: m.name };
    }
  }
  if (key.startsWith("sk-"))
    return { ...detectProviderFromHostInput("https://api.openai.com"), name: "OpenAI" };
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

      // Codex auth.json (or any pure key object) should import as a credential on
      // the matching provider preset, not as a custom provider profile.
      for (const [envKey, preset] of Object.entries(ENV_KEY_MAP)) {
        const val = obj[envKey];
        if (typeof val === "string" && val.trim()) {
          const det = detectProviderFromHostInput(preset.base_url);
          if (tryAttachCredentialToExisting(val.trim(), det)) return true;
          const next = emptyForm();
          applyProviderDetection(next, det);
          next.name = preset.name;
          stashCredentialKey(val.trim());
          form.value = next;
          parseNote.value = t("parseNotes.detectedFromEnv", {
            name: preset.name,
            envKey,
          });
          return true;
        }
      }

      // ProviderInput-shaped JSON
      if (
        typeof obj.base_url === "string" ||
        typeof obj.kind === "string" ||
        typeof obj.name === "string"
      ) {
        const next = emptyForm();
        if (typeof obj.name === "string") next.name = obj.name.trim();
        if (typeof obj.group_name === "string") next.group_name = obj.group_name.trim() || null;
        if (typeof obj.base_url === "string") next.base_url = obj.base_url.trim();
        if (typeof obj.host === "string") next.host = obj.host.trim() || null;
        if (typeof obj.auth_ref === "string" && obj.auth_ref.trim()) {
          stashCredentialKey(obj.auth_ref.trim());
        }
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
        const hostLike = next.host ?? hostFromUrlLike(next.base_url) ?? next.base_url;
        const det = detectProviderFromHostInput(hostLike);
        applyProviderDetection(next, det);
        if (!next.name && typeof obj.name === "string") next.name = obj.name.trim();
        if (typeof obj.kind === "string" && PROVIDER_KINDS.includes(obj.kind as ProviderKind)) {
          next.kind = obj.kind as ProviderKind;
        }
        if (typeof obj.base_url === "string" && obj.base_url.trim()) {
          next.base_url = obj.base_url.trim();
        }
        if (typeof obj.host === "string" && obj.host.trim()) {
          next.host = hostFromUrlLike(obj.host) ?? obj.host.trim();
        }
        next.host = next.host ?? hostFromUrlLike(next.base_url) ?? det.host;
        next.protocols = multiProtocolForBase(
          next.base_url || det.base_url,
          next.kind,
          next.model_aliases,
        );
        form.value = next;
        parseNote.value = pendingCredentialAuthRef.value
          ? t("parseNotes.providerProfileWithKey")
          : t("parseNotes.providerProfile");
        return true;
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
      const det = detectProviderFromHostInput(preset.base_url);
      if (tryAttachCredentialToExisting(envVal, det)) return true;
      const next = emptyForm();
      applyProviderDetection(next, det);
      next.name = preset.name;
      stashCredentialKey(envVal);
      form.value = next;
      parseNote.value = t("parseNotes.detectedFromEnv", {
        name: preset.name,
        envKey,
      });
      return true;
    }
  }

  const codexConfig = parseCodexConfig(trimmed);
  if (codexConfig) {
    const next = emptyForm();
    applyProviderDetection(next, detectProviderFromHostInput(codexConfig.base_url));
    next.kind = codexConfig.kind;
    next.base_url = codexConfig.base_url;
    next.protocols = multiProtocolForBase(
      codexConfig.base_url,
      codexConfig.kind,
      next.model_aliases,
    );
    const keyMatch = trimmed.match(API_KEY_RE);
    if (keyMatch) stashCredentialKey(keyMatch[0]);
    form.value = next;
    parseNote.value = keyMatch
      ? t("parseNotes.detectedFromUrlKey", { name: next.name })
      : t("parseNotes.detectedFromUrl", { name: next.name });
    return true;
  }

  // URL + optional API key
  const urlMatch = trimmed.match(URL_RE);
  const keyMatch = trimmed.match(API_KEY_RE);
  if (urlMatch) {
    const url = urlMatch[0].replace(/[),.;，。；、\])}]+$/, "");
    const next = emptyForm();
    const det = detectProviderFromHostInput(url);
    applyProviderDetection(next, det);
    if (keyMatch) {
      stashCredentialKey(keyMatch[0]);
      parseNote.value = t("parseNotes.detectedFromUrlKey", { name: det.name });
    } else {
      parseNote.value = t("parseNotes.detectedFromUrl", { name: det.name });
    }
    form.value = next;
    return true;
  }

  // Bare API key (no URL / config) — prefer adding to an existing provider
  if (keyMatch && !urlMatch && !codexConfig && !trimmed.startsWith("{")) {
    const key = keyMatch[0];
    const det = detectFromKey(key);
    if (det) {
      if (tryAttachCredentialToExisting(key, det)) return true;
      const next = emptyForm();
      applyProviderDetection(next, det);
      stashCredentialKey(key);
      form.value = next;
      parseNote.value = t("parseNotes.detectedFromApiKey", { name: det.name });
      return true;
    }
  }

  parseError.value = t("parseErrors.unrecognized");
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
      parseError.value = t("parseErrors.clipboardEmpty");
      return;
    }
    pasteRaw.value = text;
    if (tryParse(text)) phase.value = "review";
  } catch {
    parseError.value = t("parseErrors.clipboardReadFailed");
  }
}

function applyPreset(p: Preset) {
  const det = detectProviderFromHostInput(p.base_url);
  form.value = {
    name: p.name,
    group_name: p.group_name,
    kind: det.kind,
    base_url: det.base_url,
    protocols: det.protocols,
    host: det.host,
    auth_ref: null,
    avatar_url: null,
    enabled: true,
    priority: 100,
    supports_websocket: null,
    passthrough_mode: true,
    model_aliases: [],
  };
  pendingCredentialAuthRef.value = null;
  parseNote.value = t("parseNotes.appliedPreset", { label: p.label });
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
  if (props.editTarget) return t("title.edit");
  return phase.value === "review" ? t("title.review") : t("title.add");
});

const localDetectedProvider = computed(() =>
  detectProviderFromHostInput(form.value.host ?? form.value.base_url),
);
const detectedProvider = computed(() => probedProvider.value ?? localDetectedProvider.value);

const primaryProtocolKind = computed(() => detectedProvider.value.kind);
const detectedProtocols = computed(() => probedProvider.value?.protocols ?? []);
const detectedBaseUrl = computed(() => probedProvider.value?.base_url ?? "");
const detectedHost = computed(() => probedProvider.value?.host ?? localDetectedProvider.value.host);
const canSave = computed(
  () => !probeBusy.value && !!probedProvider.value && detectedProtocols.value.length > 0,
);

const hasLegacyProviderKey = computed(() => !!props.editTarget?.auth_ref?.trim());

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
  const authRef = normalizeAuthRef(pendingCredentialAuthRef.value);
  if (credentialTargetId.value && authRef) {
    emit("saveCredentialOnly", credentialTargetId.value, authRef);
    return;
  }
  const detected = probedProvider.value;
  if (!detected) {
    probeError.value = t("probe.required");
    return;
  }
  const payload: ProviderInput = {
    ...form.value,
    name: form.value.name.trim(),
    group_name: form.value.group_name?.trim() || null,
    avatar_url: form.value.avatar_url?.trim() || null,
    kind: detected.kind,
    base_url: detected.base_url.trim(),
    protocols: detected.protocols.map((proto) => ({
      ...proto,
      model_aliases: [...form.value.model_aliases],
    })),
    host: detected.host,
    auth_ref: null,
    model_aliases: form.value.model_aliases
      .map((a) => ({
        alias: a.alias.trim(),
        upstream_model: a.upstream_model.trim(),
      }))
      .filter((a) => a.alias && a.upstream_model),
  };
  emit("save", payload, authRef);
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
              :kind="primaryProtocolKind"
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
            :aria-label="t('actions.close')"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>

        <!-- ── PASTE PHASE ── -->
        <div v-if="phase === 'paste'" class="flex flex-1 flex-col overflow-y-auto px-5 py-5">
          <p class="mb-3 text-sm text-vp-muted">
            {{ t("paste.description") }}
          </p>

          <textarea
            :value="pasteRaw"
            rows="6"
            class="w-full resize-none rounded-xl border border-vp-border bg-white px-4 py-3 font-mono text-sm text-slate-900 placeholder:text-slate-400 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
            :placeholder="t('paste.placeholder')"
            @input="onTextareaInput"
            @paste="onTextareaPaste"
          />

          <p v-if="parseError" class="mt-2 text-xs text-red-600">
            {{ parseError }}
          </p>

          <div class="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg border border-vp-border bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-50"
              @click="readClipboard"
            >
              <VpIcon name="clipboard" size-class="size-4" />
              {{ t("actions.readClipboard") }}
            </button>
            <button
              v-if="pasteRaw.trim()"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg bg-violet-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-violet-700"
              @click="doParseAndAdvance"
            >
              <VpIcon name="sparkles" size-class="size-4" />
              {{ t("actions.parse") }}
            </button>
          </div>

          <!-- Presets -->
          <div class="mt-5">
            <p class="mb-2.5 text-xs font-medium uppercase tracking-wide text-vp-muted">
              {{ t("paste.pickPreset") }}
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
            {{ t("actions.skipManual") }}
          </button>
        </div>

        <!-- ── REVIEW PHASE ── -->
        <div v-else class="flex flex-1 flex-col gap-4 overflow-y-auto px-5 py-5">
          <p
            v-if="hasLegacyProviderKey"
            class="rounded-xl border border-amber-200 bg-amber-50/80 px-3 py-2.5 text-xs text-amber-950"
          >
            {{ t("hints.legacyKeyBefore") }}
            <strong>{{ t("sections.credentials") }}</strong>
            {{ t("hints.legacyKeyAfter") }}
          </p>
          <p
            v-else-if="pendingCredentialAuthRef"
            class="rounded-xl border border-emerald-200 bg-emerald-50/80 px-3 py-2.5 text-xs text-emerald-950"
          >
            {{ t("hints.detectedKeyBefore") }}
            <strong>{{ t("hints.credential") }}</strong>
            {{ t("hints.detectedKeyAfter") }}
          </p>

          <!-- Core fields -->
          <section class="space-y-3 rounded-2xl border border-vp-border bg-white p-4">
            <h3 class="text-xs font-semibold uppercase tracking-wide text-vp-muted">
              {{ t("sections.basics") }}
            </h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <label>
                <span class="mb-1 block text-xs font-medium text-slate-600">{{
                  t("fields.name")
                }}</span>
                <input
                  v-model="form.name"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                  :placeholder="t('fields.providerName')"
                />
              </label>
              <label class="sm:col-span-2">
                <span class="mb-1 block text-xs font-medium text-slate-600">{{
                  t("fields.host")
                }}</span>
                <input
                  v-model="form.host"
                  class="w-full rounded-lg border border-slate-200 px-3 py-2 font-mono text-sm text-slate-900 focus:border-violet-400 focus:outline-none focus:ring-2 focus:ring-violet-400/20"
                  placeholder="api.example.com"
                />
              </label>
              <div
                class="sm:col-span-2 rounded-xl border border-slate-200 bg-slate-50/70 px-3 py-2"
              >
                <p class="text-xs font-medium text-slate-600">{{ t("fields.autoDetection") }}</p>
                <div v-if="detectedProtocols.length" class="mt-2 flex flex-wrap gap-1.5">
                  <span
                    v-for="proto in detectedProtocols"
                    :key="`${proto.kind}:${proto.base_url}`"
                    class="rounded-full border border-violet-200 bg-white px-2 py-0.5 text-xs text-violet-700"
                  >
                    {{ kindLabel(proto.kind) }}
                  </span>
                </div>
                <p
                  v-if="detectedHost && detectedBaseUrl"
                  class="mt-2 break-all font-mono text-[11px] text-slate-500"
                >
                  {{ detectedHost }} → {{ detectedBaseUrl }}
                </p>
                <p class="mt-1 text-xs text-slate-500">{{ t("hints.protocols") }}</p>
                <p v-if="probeBusy" class="mt-2 text-xs text-slate-500">
                  {{ t("probe.checking") }}
                </p>
                <p v-else-if="probeError" class="mt-2 text-xs text-red-600">
                  {{ probeError }}
                </p>
              </div>
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
                {{ t("sections.advanced") }}
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
                  <span class="mb-1 block text-xs font-medium text-slate-600">{{
                    t("fields.priority")
                  }}</span>
                  <input
                    v-model.number="form.priority"
                    type="number"
                    class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm focus:border-violet-400 focus:outline-none"
                  />
                </label>
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">{{
                    t("fields.group")
                  }}</span>
                  <input
                    v-model="form.group_name"
                    class="w-full rounded-lg border border-slate-200 px-3 py-2 text-sm focus:border-violet-400 focus:outline-none"
                    :placeholder="t('fields.groupPlaceholder')"
                  />
                </label>
                <label
                  class="flex cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.enabled" type="checkbox" class="rounded" />
                  <span>{{ t("fields.providerEnabled") }}</span>
                </label>
                <label
                  class="flex cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.passthrough_mode" type="checkbox" class="rounded" />
                  <span>{{ t("fields.passthrough") }}</span>
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
                {{ t("sections.aliases") }}
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
                {{ t("aliases.empty") }}
              </div>
              <div
                v-for="(alias, index) in form.model_aliases"
                :key="index"
                class="grid grid-cols-[1fr_1fr_auto] gap-2"
              >
                <input
                  v-model="alias.alias"
                  class="rounded-lg border border-slate-200 px-2.5 py-1.5 text-sm focus:border-violet-400 focus:outline-none"
                  :placeholder="t('aliases.clientAlias')"
                />
                <input
                  v-model="alias.upstream_model"
                  class="rounded-lg border border-slate-200 px-2.5 py-1.5 font-mono text-sm focus:border-violet-400 focus:outline-none"
                  :placeholder="t('aliases.upstreamModel')"
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
                {{ t("actions.addRow") }}
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
                {{ t("sections.credentials") }}
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
                  {{ t("actions.add") }}
                </button>
                <VpIcon
                  name="chevron-down"
                  :class="showCreds ? 'rotate-180' : ''"
                  size-class="size-4 text-slate-400 transition-transform"
                />
              </div>
            </button>
            <div v-if="showCreds" class="border-t border-vp-border/60 px-4 pb-4 pt-3">
              <div v-if="loadingCreds" class="py-3 text-center text-xs text-slate-500">
                {{ t("states.loading") }}
              </div>
              <div
                v-else-if="!creds.length"
                class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
              >
                {{ t("states.noCredentials") }}
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
                      {{ cred.enabled ? t("actions.disable") : t("actions.enable") }}
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-slate-200 px-2 py-1 text-[11px] text-slate-700 hover:bg-white"
                      @click="emit('editCredential', cred)"
                    >
                      {{ t("actions.edit") }}
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-red-200 px-2 py-1 text-[11px] text-red-700 hover:bg-red-50"
                      @click="emit('removeCredential', cred)"
                    >
                      {{ t("actions.remove") }}
                    </button>
                  </div>
                </li>
              </ul>
            </div>
          </section>

          <!-- New-provider hint about credentials -->
          <p
            v-if="!editTarget && !pendingCredentialAuthRef"
            class="rounded-xl border border-amber-200 bg-amber-50/60 px-3 py-2.5 text-xs text-amber-900"
          >
            {{ t("hints.keysUnderBefore") }}
            <strong>{{ t("sections.credentials") }}</strong>
            {{ t("hints.keysUnderAfter") }}
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
            {{ t("actions.pasteAgain") }}
          </button>
          <button
            type="button"
            class="btn-ghost inline-flex flex-1 items-center justify-center gap-1.5 px-4 py-2 text-sm sm:flex-none"
            @click="emit('close')"
          >
            {{ t("actions.cancel") }}
          </button>
          <button
            v-if="phase === 'review'"
            type="button"
            class="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-700 sm:flex-none"
            :disabled="!canSave"
            :class="!canSave ? 'cursor-not-allowed opacity-50 hover:bg-violet-600' : ''"
            @click="handleSave"
          >
            <VpIcon name="check" size-class="size-4" />
            {{ editTarget ? t("actions.saveChanges") : t("actions.createProvider") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<i18n lang="json">
{
  "en": {
    "actions": {
      "add": "Add",
      "addProtocol": "Add protocol",
      "addRow": "Add row",
      "cancel": "Cancel",
      "close": "Close",
      "createProvider": "Create provider",
      "disable": "Disable",
      "edit": "Edit",
      "enable": "Enable",
      "parse": "Parse",
      "pasteAgain": "Paste again",
      "readClipboard": "Read clipboard",
      "remove": "Remove",
      "removeProtocol": "Remove protocol",
      "saveChanges": "Save changes",
      "skipManual": "Skip — fill in manually →"
    },
    "aliases": {
      "clientAlias": "Client alias",
      "empty": "No aliases. Add rows only when upstream model IDs differ from client requests.",
      "upstreamModel": "Upstream model ID"
    },
    "fields": {
      "autoDetection": "Auto-detected endpoint",
      "baseUrl": "Base URL",
      "group": "Group",
      "groupPlaceholder": "e.g. official / personal",
      "host": "Host",
      "name": "Name",
      "passthrough": "Passthrough model names",
      "priority": "Priority",
      "protocol": "Protocol",
      "protocols": "Protocols",
      "providerEnabled": "Provider enabled",
      "providerName": "Provider name"
    },
    "hints": {
      "credential": "credential",
      "detectedKeyAfter": "when you save (not stored on the provider).",
      "detectedKeyBefore": "An API key was detected and will be created as a",
      "keysUnderAfter": "on the provider card after you create this provider — never on the provider record itself.",
      "keysUnderBefore": "API keys live under",
      "legacyKeyAfter": "below.",
      "legacyKeyBefore": "This provider has a legacy provider-level API key. Saving clears it — manage keys under",
      "protocols": "Protocol and path are inferred from the host automatically; no manual route setup is required."
    },
    "paste": {
      "description": "Paste JSON, a host, or an env line. We auto-detect the path and protocol from the host; API keys are stored as credentials only — never on the provider.",
      "pickPreset": "Or pick a preset",
      "placeholder": "JSON config / host / KEY=value / URL + API key…"
    },
    "parseErrors": {
      "clipboardEmpty": "Clipboard is empty.",
      "clipboardReadFailed": "Could not read clipboard — paste into the text box instead.",
      "unrecognized": "Could not parse input. Paste JSON, base URL, env line, or URL + API key."
    },
    "parseNotes": {
      "addKeyToEditing": "API key will be added as a credential to the provider you are editing.",
      "addKeyToExisting": "API key will be added as a credential to “{name}” (existing provider).",
      "appliedPreset": "Applied “{label}” preset.",
      "detectedFromApiKey": "Detected {name} from API key; key will be saved as a credential.",
      "detectedFromEnv": "Detected {name} from {envKey}; key will be saved as a credential.",
      "detectedFromUrl": "Detected {name} from URL.",
      "detectedFromUrlKey": "Detected {name} from URL + key; key will be saved as a credential.",
      "providerProfile": "Parsed provider profile from JSON.",
      "providerProfileWithKey": "Parsed provider profile; API key will be saved as a credential."
    },
    "probe": {
      "checking": "Checking endpoint…",
      "failed": "Endpoint probe failed.",
      "invalidHost": "Invalid host.",
      "noEndpoint": "No supported API endpoint responded successfully.",
      "required": "Wait for endpoint detection to finish before saving."
    },
    "sections": {
      "advanced": "Advanced",
      "aliases": "Model aliases",
      "basics": "Basics",
      "credentials": "Credentials"
    },
    "states": { "loading": "Loading…", "noCredentials": "No credentials yet." },
    "title": {
      "add": "Add provider",
      "edit": "Edit provider",
      "review": "Review configuration"
    }
  },
  "zh-CN": {
    "actions": {
      "add": "添加",
      "addProtocol": "添加协议",
      "addRow": "添加行",
      "cancel": "取消",
      "close": "关闭",
      "createProvider": "创建供应商",
      "disable": "禁用",
      "edit": "编辑",
      "enable": "启用",
      "parse": "解析",
      "pasteAgain": "重新粘贴",
      "readClipboard": "读取剪贴板",
      "remove": "移除",
      "removeProtocol": "移除协议",
      "saveChanges": "保存更改",
      "skipManual": "跳过，手动填写 →"
    },
    "aliases": {
      "clientAlias": "客户端别名",
      "empty": "暂无别名。仅当上游模型 ID 与客户端请求不同时才需要添加行。",
      "upstreamModel": "上游模型 ID"
    },
    "fields": {
      "autoDetection": "自动识别的端点",
      "baseUrl": "Base URL",
      "group": "分组",
      "groupPlaceholder": "例如 official / personal",
      "host": "Host",
      "name": "名称",
      "passthrough": "透传模型名",
      "priority": "优先级",
      "protocol": "协议",
      "protocols": "协议",
      "providerEnabled": "启用供应商",
      "providerName": "供应商名称"
    },
    "hints": {
      "credential": "凭证",
      "detectedKeyAfter": "，保存时创建（不会存到供应商上）。",
      "detectedKeyBefore": "检测到 API Key，会作为",
      "keysUnderAfter": "中管理，不会写入供应商记录。",
      "keysUnderBefore": "创建后 API Key 会在供应商卡片的",
      "legacyKeyAfter": "中管理。",
      "legacyKeyBefore": "该供应商存在旧版 provider-level API Key。保存会清除它，请改在",
      "protocols": "协议与路径会根据 Host 自动推断，无需手动新增路由。"
    },
    "paste": {
      "description": "粘贴 JSON、Host 或 env 行。我们会根据 Host 自动识别路径和协议；API Key 只会作为凭证保存，绝不存到供应商上。",
      "pickPreset": "或选择预设",
      "placeholder": "JSON 配置 / Host / KEY=value / URL + API Key…"
    },
    "parseErrors": {
      "clipboardEmpty": "剪贴板为空。",
      "clipboardReadFailed": "无法读取剪贴板，请直接粘贴到文本框。",
      "unrecognized": "无法解析输入。请粘贴 JSON、Base URL、env 行，或 URL + API Key。"
    },
    "parseNotes": {
      "addKeyToEditing": "将把 API Key 作为凭证添加到当前正在编辑的供应商。",
      "addKeyToExisting": "将把 API Key 作为凭证添加到已有供应商「{name}」。",
      "appliedPreset": "已应用「{label}」预设。",
      "detectedFromApiKey": "从 API Key 识别到 {name}；Key 会保存为凭证。",
      "detectedFromEnv": "从 {envKey} 识别到 {name}；Key 会保存为凭证。",
      "detectedFromUrl": "从 URL 识别到 {name}。",
      "detectedFromUrlKey": "从 URL + Key 识别到 {name}；Key 会保存为凭证。",
      "providerProfile": "已从 JSON 解析供应商配置。",
      "providerProfileWithKey": "已解析供应商配置；API Key 会保存为凭证。"
    },
    "probe": {
      "checking": "正在检测端点…",
      "failed": "端点检测失败。",
      "invalidHost": "Host 无效。",
      "noEndpoint": "没有检测到可用的 API 端点。",
      "required": "请等待端点检测完成后再保存。"
    },
    "sections": {
      "advanced": "高级",
      "aliases": "模型别名",
      "basics": "基础",
      "credentials": "凭证"
    },
    "states": { "loading": "加载中…", "noCredentials": "暂无凭证。" },
    "title": { "add": "添加供应商", "edit": "编辑供应商", "review": "检查配置" }
  }
}
</i18n>

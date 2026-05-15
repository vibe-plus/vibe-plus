import { computed, onMounted, onUnmounted, ref, shallowRef } from "vue";
import { useRoute, useRouter } from "vue-router";
import { api, type CcsProfileExportBundle } from "../api/client.ts";
import { hintsFromAuthJsonTokens } from "../utils/codex-oauth-hints.ts";

export type SmartIntakeKind =
  | "codex-auth"
  | "api-key"
  | "remote-provider"
  | "codex-config"
  | "ccswitch-provider"
  | "ccs-profile"
  | "unknown";
export type SmartIntakeSource = "clipboard" | "paste" | "drop" | "deeplink";
export type SmartDragIntent = "text" | "files" | "folder" | "mixed" | null;

export interface SmartIntakeItem {
  id: string;
  kind: SmartIntakeKind;
  source: SmartIntakeSource;
  name: string;
  summary: string;
  text?: string;
  auth?: ParsedAuth;
  ccsBundle?: CcsProfileExportBundle;
  ccSwitchUrl?: string;
  remoteText?: string;
}

interface ParsedAuth {
  access: string | null;
  refresh: string | null;
  exp: number | null;
  apiKey: string | null;
  email: string | null;
  subject: string | null;
  plan: string | null;
}

const MAX_DROP_FILES = 24;
const MAX_DROP_DEPTH = 3;
const MAX_TEXT_BYTES = 1024 * 512;
const AUTH_JSON_HINTS = ["auth_mode", "access_token", "refresh_token", "OPENAI_API_KEY"];
const CC_SWITCH_SCHEME = "ccswitch://";
const URL_TOKEN_RE = /https?:\/\/[^\s"'<>]+/gi;
const REMOTE_SECRET_RE =
  /(?:sk|sk-ant|sk_ant|sk-or|sk-proj|ck|dk|tk|xai|gsk|pat|hf)[-_][A-Za-z0-9_\-.]{16,}|AIza[A-Za-z0-9_-]{24,}/gi;

function cleanUrlToken(raw: string): string {
  return raw.trim().replace(/[),.;，。；、]+$/g, "");
}

function remoteSignature(entry: { text: string; name: string }): string {
  try {
    const obj = JSON.parse(entry.text) as Record<string, unknown>;
    const url = typeof obj.url === "string" ? cleanUrlToken(obj.url) : entry.name;
    const key = typeof obj.key === "string" ? obj.key.trim() : "";
    if (url && key) return `${url}::${key}`;
  } catch {
    // fall through to token parsing
  }
  const url = entry.text.match(URL_TOKEN_RE)?.[0];
  const key = entry.text.match(REMOTE_SECRET_RE)?.[0];
  return `${url ? cleanUrlToken(url) : entry.name}::${key ?? entry.text}`;
}

function parseRemoteProviderText(raw: string): { text: string; name: string } | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  try {
    const obj = JSON.parse(trimmed) as Record<string, unknown>;
    if (
      obj._type === "newapi_channel_conn" &&
      typeof obj.url === "string" &&
      typeof obj.key === "string"
    ) {
      const cleanUrl = cleanUrlToken(String(obj.url));
      const cleanKey = String(obj.key).trim();
      if (cleanUrl && cleanKey) {
        return { text: `${cleanUrl} ${cleanKey}`, name: cleanUrl };
      }
    }
  } catch {
    // fall through to plain-text parsing
  }

  const tokens = trimmed
    .split(/\s+/)
    .map((token) => token.trim())
    .filter(Boolean);
  const url = tokens
    .map((token) => token.match(/^https?:\/\/[^\s"'<>]+/i)?.[0] ?? null)
    .find((token): token is string => !!token);
  const key = tokens.find((token) => REMOTE_SECRET_RE.test(token));
  if (url && key) {
    const cleanUrl = cleanUrlToken(url);
    const cleanKey = key.trim().replace(/[),.;，。；、]+$/g, "");
    if (cleanUrl && cleanKey) {
      return { text: `${cleanUrl} ${cleanKey}`, name: cleanUrl };
    }
  }
  return null;
}

function linesFromBlob(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function extractRemoteProviderTexts(raw: string): Array<{ text: string; name: string }> {
  const trimmed = raw.trim();
  if (!trimmed) return [];

  const found: Array<{ text: string; name: string }> = [];
  const seen = new Set<string>();
  const push = (entry: { text: string; name: string } | null) => {
    if (!entry) return;
    const sig = remoteSignature(entry);
    if (seen.has(sig)) return;
    seen.add(sig);
    found.push(entry);
  };

  push(parseRemoteProviderText(trimmed));

  const jsonPattern = /\{[^{}]*"_type"\s*:\s*"newapi_channel_conn"[^{}]*\}/g;
  for (const match of trimmed.matchAll(jsonPattern)) push(parseRemoteProviderText(match[0]));
  for (const obj of extractJsonObjects(trimmed)) push(parseRemoteProviderText(obj));

  for (const line of linesFromBlob(trimmed)) push(parseRemoteProviderText(line));
  for (const block of remoteBlocksFromText(trimmed)) push(block);
  return found;
}

function remoteBlocksFromText(raw: string): Array<{ text: string; name: string }> {
  const lines = linesFromBlob(raw);
  const out: Array<{ text: string; name: string }> = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].includes("_type") && lines[i].includes("newapi_channel_conn")) continue;
    const key = lines[i].match(REMOTE_SECRET_RE)?.[0];
    if (!key) continue;
    const start = Math.max(0, i - 4);
    const end = Math.min(lines.length, i + 5);
    const windowText = lines.slice(start, end).join("\n");
    const urls = [...windowText.matchAll(URL_TOKEN_RE)].map((m) => cleanUrlToken(m[0]));
    const url = urls.find((candidate) => !candidate.includes("127.0.0.1")) ?? urls[0];
    if (url) out.push({ text: `${url} ${key}`, name: url });
  }
  return out;
}

function extractJsonObjects(raw: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let start = -1;
  let inString = false;
  let escaped = false;
  for (let i = 0; i < raw.length; i += 1) {
    const ch = raw[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "{") {
      if (depth === 0) start = i;
      depth += 1;
      continue;
    }
    if (ch === "}") {
      depth -= 1;
      if (depth === 0 && start >= 0) {
        out.push(raw.slice(start, i + 1));
        start = -1;
      }
    }
  }
  return out;
}

function extractAuthLikeTexts(raw: string): string[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  const found: string[] = [];
  const seen = new Set<string>();
  const push = (text: string | null | undefined) => {
    const next = text?.trim();
    if (!next || seen.has(next)) return;
    seen.add(next);
    found.push(next);
  };

  push(trimmed);
  for (const obj of extractJsonObjects(trimmed)) push(obj);
  for (const line of linesFromBlob(trimmed)) push(line);

  const keyPattern =
    /(?:sk|sk-ant|sk_ant|sk-or|sk-proj|ck|dk|tk|xai|gsk|pat|hf)[-_][A-Za-z0-9_\-.]{16,}|AIza[A-Za-z0-9_-]{24,}|eyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}/g;
  for (const match of trimmed.matchAll(keyPattern)) push(match[0]);
  return found;
}

function isTextInput(target: EventTarget | null): boolean {
  const el = target instanceof HTMLElement ? target : null;
  if (!el) return false;
  return (
    el.tagName === "TEXTAREA" ||
    el.tagName === "INPUT" ||
    el.isContentEditable ||
    !!el.closest("[contenteditable=true]")
  );
}

function jwtExp(jwt: string | null): number | null {
  if (!jwt) return null;
  const mid = jwt.split(".")[1];
  if (!mid) return null;
  try {
    let s = mid.replace(/-/g, "+").replace(/_/g, "/");
    while (s.length % 4 !== 0) s += "=";
    const payload = JSON.parse(atob(s)) as { exp?: unknown };
    return typeof payload.exp === "number" ? payload.exp : null;
  } catch {
    return null;
  }
}

function parseJsonAuth(raw: string): ParsedAuth | null {
  let doc: unknown;
  try {
    doc = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return null;
  const obj = doc as Record<string, unknown>;
  const tokens = typeof obj.tokens === "object" && obj.tokens !== null ? obj.tokens : null;
  const t = tokens as Record<string, unknown> | null;
  const accessRaw = t?.access_token ?? obj.access_token;
  const refreshRaw = t?.refresh_token ?? obj.refresh_token;
  const keyRaw = obj.OPENAI_API_KEY ?? obj.apiKey ?? obj.api_key;
  const access = typeof accessRaw === "string" && accessRaw.trim() ? accessRaw.trim() : null;
  const apiKey =
    typeof keyRaw === "string" && keyRaw.trim() && keyRaw !== "PROXY_MANAGED"
      ? keyRaw.trim()
      : null;
  if (!access && !apiKey) return null;

  const expiresAt =
    typeof t?.expires_at === "number"
      ? t.expires_at
      : typeof obj.expires_at === "number"
        ? obj.expires_at
        : typeof t?.expiry === "number"
          ? t.expiry
          : null;
  const hints = hintsFromAuthJsonTokens(tokens);
  const email =
    hints.oauth_cached_email ??
    (typeof obj.email === "string" && obj.email.trim() ? obj.email.trim() : null);
  return {
    access,
    refresh: typeof refreshRaw === "string" && refreshRaw.trim() ? refreshRaw.trim() : null,
    exp: expiresAt ?? jwtExp(access),
    apiKey,
    email,
    subject: hints.oauth_cached_subject,
    plan: hints.oauth_cached_plan_slug,
  };
}

function parseCcsProfileBundle(raw: string): CcsProfileExportBundle | null {
  let doc: unknown;
  try {
    doc = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return null;
  const obj = doc as Record<string, unknown>;
  if (obj.schemaVersion !== 1) return null;
  const profile = obj.profile;
  const settings = obj.settings;
  if (typeof profile !== "object" || profile === null || Array.isArray(profile)) return null;
  if (typeof settings !== "object" || settings === null || Array.isArray(settings)) return null;
  const profileObj = profile as Record<string, unknown>;
  if (typeof profileObj.name !== "string" || !profileObj.name.trim()) return null;
  const settingsObj = settings as Record<string, unknown>;
  const env = settingsObj.env;
  if (typeof env !== "object" || env === null || Array.isArray(env)) return null;
  return {
    schemaVersion: 1,
    exportedAt: typeof obj.exportedAt === "string" ? obj.exportedAt : undefined,
    profile: {
      name: profileObj.name,
      target: typeof profileObj.target === "string" ? profileObj.target : undefined,
    },
    settings: settingsObj,
  };
}

function parseCcSwitchProviderUrl(raw: string): { url: string; name: string; app: string } | null {
  const trimmed = raw.trim();
  if (!trimmed.toLowerCase().startsWith(CC_SWITCH_SCHEME)) return null;
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return null;
  }
  if (url.protocol !== "ccswitch:" || url.hostname !== "v1" || url.pathname !== "/import")
    return null;
  if (url.searchParams.get("resource") !== "provider") return null;
  const app = url.searchParams.get("app")?.trim();
  const name = url.searchParams.get("name")?.trim();
  if (!app || !name) return null;
  return { url: trimmed, name, app };
}

function looksLikeCodexConfig(raw: string, name = ""): boolean {
  const s = raw.slice(0, 4000).toLowerCase();
  const n = name.toLowerCase();
  return (
    n.endsWith(".toml") ||
    n === "config.toml" ||
    s.includes("model_provider") ||
    s.includes("[model_providers.") ||
    s.includes("wire_api") ||
    s.includes("stream_idle_timeout_ms")
  );
}

/**
 * Compatible with common API key formats:
 * - OpenAI / compatible proxies `sk-…`、`sk-ant-…`、`sk-or-…`
 * - Google AI Studio `AIza…`
 * - xAI `xai-…`、Anthropic legacy format `sk_ant_…`、Groq `gsk_…`
 * - Domestic proxies:`ck-…`、`dk-…`、`tk-…`、`hf_…`、`Bearer …`、`pat-…`
 * - Fallback: single-line raw secret, length >= 24, only `[A-Za-z0-9_\-./]`, and at least one segment >= 16 characters to avoid normal words.
 */
const KNOWN_KEY_PREFIX_RE =
  /\b(sk|sk-ant|sk_ant|sk-or|sk-proj|ck|dk|tk|xai|gsk|pat|hf|api|key)[-_][A-Za-z0-9_\-.]{16,}/;
const JWT_RE = /eyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}/;
const GOOGLE_KEY_RE = /AIza[A-Za-z0-9_-]{24,}/;

function looksLikeBareKey(raw: string): boolean {
  const trimmed = raw.trim();
  if (!trimmed || trimmed.includes("\n") || trimmed.includes(" ")) return false;
  if (trimmed.length < 24 || trimmed.length > 512) return false;
  // Allow only base64url/API-key characters
  if (!/^[A-Za-z0-9_\-./]+$/.test(trimmed)) return false;
  // At least one segment split by `.` or `-` must be >= 16 chars to avoid short words
  const longest = Math.max(...trimmed.split(/[.\-_/]/).map((s) => s.length));
  return longest >= 16;
}

function textLooksUseful(raw: string): boolean {
  const trimmed = raw.trim();
  if (!trimmed) return false;
  if (parseRemoteProviderText(trimmed)) return true;
  if (looksLikeCodexConfig(trimmed)) return true;
  if (trimmed.toLowerCase().startsWith(CC_SWITCH_SCHEME)) return true;
  if (trimmed.startsWith("{") && trimmed.includes("schemaVersion") && trimmed.includes("settings"))
    return true;
  if (trimmed.startsWith("{") && AUTH_JSON_HINTS.some((hint) => trimmed.includes(hint)))
    return true;
  if (KNOWN_KEY_PREFIX_RE.test(trimmed)) return true;
  if (GOOGLE_KEY_RE.test(trimmed)) return true;
  if (JWT_RE.test(trimmed)) return true;
  return looksLikeBareKey(trimmed);
}

function parseBareSecret(raw: string): ParsedAuth | null {
  const trimmed = raw.trim().replace(/^Bearer\s+/i, "");
  // Keys with known prefixes
  const prefixed = trimmed.match(KNOWN_KEY_PREFIX_RE)?.[0] ?? null;
  // Google AI Studio
  const google = trimmed.match(GOOGLE_KEY_RE)?.[0] ?? null;
  const apiKey = prefixed ?? google;
  if (apiKey) {
    return {
      access: null,
      refresh: null,
      exp: null,
      apiKey,
      email: null,
      subject: null,
      plan: null,
    };
  }
  // JWT shaped like codex/claude OAuth access_token
  const jwt = trimmed.match(JWT_RE)?.[0] ?? null;
  if (jwt) {
    return {
      access: jwt,
      refresh: null,
      exp: jwtExp(jwt),
      apiKey: null,
      email: null,
      subject: null,
      plan: null,
    };
  }
  // Fallback: single-line raw secret
  if (looksLikeBareKey(trimmed)) {
    return {
      access: null,
      refresh: null,
      exp: null,
      apiKey: trimmed,
      email: null,
      subject: null,
      plan: null,
    };
  }
  return null;
}

export function itemFromText(
  raw: string,
  source: SmartIntakeSource,
  name = "clipboard",
): SmartIntakeItem {
  const trimmed = raw.trim();
  const remote = parseRemoteProviderText(trimmed);
  if (remote) {
    return {
      id: crypto.randomUUID(),
      kind: "remote-provider",
      source,
      name,
      summary: remote.name,
      text: remote.text,
      remoteText: remote.text,
    };
  }
  const ccSwitch = parseCcSwitchProviderUrl(trimmed);
  if (ccSwitch) {
    return {
      id: crypto.randomUUID(),
      kind: "ccswitch-provider",
      source,
      name,
      summary: `${ccSwitch.app}:${ccSwitch.name}`,
      text: trimmed,
      ccSwitchUrl: ccSwitch.url,
    };
  }
  const ccsBundle = parseCcsProfileBundle(trimmed);
  if (ccsBundle) {
    return {
      id: crypto.randomUUID(),
      kind: "ccs-profile",
      source,
      name,
      summary: ccsBundle.profile.name,
      text: trimmed,
      ccsBundle,
    };
  }
  const auth = parseJsonAuth(trimmed);
  if (auth?.access) {
    return {
      id: crypto.randomUUID(),
      kind: "codex-auth",
      source,
      name,
      summary: auth.email ?? auth.plan ?? "OAuth",
      text: trimmed,
      auth,
    };
  }
  if (auth?.apiKey) {
    return {
      id: crypto.randomUUID(),
      kind: "api-key",
      source,
      name,
      summary: "OPENAI_API_KEY",
      text: trimmed,
      auth,
    };
  }
  const bareAuth = parseBareSecret(trimmed);
  if (bareAuth?.access) {
    return {
      id: crypto.randomUUID(),
      kind: "codex-auth",
      source,
      name,
      summary: "OAuth",
      text: trimmed,
      auth: bareAuth,
    };
  }
  if (bareAuth?.apiKey) {
    return {
      id: crypto.randomUUID(),
      kind: "api-key",
      source,
      name,
      summary: "OPENAI_API_KEY",
      text: trimmed,
      auth: bareAuth,
    };
  }
  if (looksLikeCodexConfig(trimmed, name)) {
    return {
      id: crypto.randomUUID(),
      kind: "codex-config",
      source,
      name,
      summary: "config.toml",
      text: raw,
    };
  }
  return {
    id: crypto.randomUUID(),
    kind: "unknown",
    source,
    name,
    summary: "unknown",
    text: trimmed.slice(0, 1200),
  };
}

export function itemsFromText(
  raw: string,
  source: SmartIntakeSource,
  name = "clipboard",
): SmartIntakeItem[] {
  const remoteMatches = extractRemoteProviderTexts(raw);
  const remoteItems = remoteMatches.map((remote) => ({
    id: crypto.randomUUID(),
    kind: "remote-provider" as const,
    source,
    name,
    summary: remote.name,
    text: remote.text,
    remoteText: remote.text,
  }));

  const remoteSecrets = new Set(
    remoteItems
      .map((item) => item.remoteText?.match(REMOTE_SECRET_RE)?.[0])
      .filter((secret): secret is string => !!secret),
  );
  const authItems = extractAuthLikeTexts(raw)
    .map((chunk) => itemFromText(chunk, source, name))
    .filter((item) => item.kind === "api-key" || item.kind === "codex-auth")
    .filter((item) => !item.auth?.apiKey || !remoteSecrets.has(item.auth.apiKey));

  const other = itemFromText(raw, source, name);
  const miscItems =
    other.kind === "ccswitch-provider" ||
    other.kind === "ccs-profile" ||
    other.kind === "codex-config"
      ? [other]
      : [];

  const merged = [...remoteItems, ...authItems, ...miscItems];
  const unique = new Map<string, SmartIntakeItem>();
  for (const item of merged) {
    const sig = `${item.kind}::${item.summary}::${item.text ?? ""}`;
    if (!unique.has(sig)) unique.set(sig, item);
  }
  if (unique.size > 0) return [...unique.values()];
  return [other];
}

function shouldReadFile(file: File): boolean {
  const name = file.name.toLowerCase();
  return (
    file.size <= MAX_TEXT_BYTES &&
    (name.endsWith(".json") ||
      name.endsWith(".toml") ||
      name.includes("auth") ||
      name === "config" ||
      file.type.includes("json") ||
      file.type.startsWith("text/"))
  );
}

async function readFileItems(file: File, source: SmartIntakeSource): Promise<SmartIntakeItem[]> {
  if (!shouldReadFile(file)) return [];
  try {
    const text = await file.text();
    return itemsFromText(text, source, file.name);
  } catch {
    return [];
  }
}

async function readEntry(entry: FileSystemEntry, depth: number, out: File[]): Promise<void> {
  if (out.length >= MAX_DROP_FILES || depth > MAX_DROP_DEPTH) return;
  if (entry.isFile) {
    await new Promise<void>((resolve) => {
      (entry as FileSystemFileEntry).file(
        (file) => {
          out.push(file);
          resolve();
        },
        () => resolve(),
      );
    });
    return;
  }
  if (!entry.isDirectory) return;
  const reader = (entry as FileSystemDirectoryEntry).createReader();
  await new Promise<void>((resolve) => {
    reader.readEntries(
      async (entries) => {
        for (const child of entries) {
          await readEntry(child, depth + 1, out);
          if (out.length >= MAX_DROP_FILES) break;
        }
        resolve();
      },
      () => resolve(),
    );
  });
}

async function filesFromDataTransfer(dt: DataTransfer): Promise<File[]> {
  const out: File[] = [];
  const entries = Array.from(dt.items ?? [])
    .map((item) => item.webkitGetAsEntry?.())
    .filter((entry): entry is FileSystemEntry => !!entry);
  if (entries.length) {
    for (const entry of entries) {
      await readEntry(entry, 0, out);
      if (out.length >= MAX_DROP_FILES) break;
    }
    return out;
  }
  return Array.from(dt.files ?? []).slice(0, MAX_DROP_FILES);
}

function dragIntentFromDataTransfer(dt: DataTransfer | null): SmartDragIntent {
  if (!dt) return null;
  const types = Array.from(dt.types ?? []);
  const hasText = types.includes("text/plain") || types.includes("text/uri-list");
  const hasFiles = types.includes("Files") || (dt.files?.length ?? 0) > 0;
  const entries = Array.from(dt.items ?? [])
    .map((item) => item.webkitGetAsEntry?.())
    .filter((entry): entry is FileSystemEntry => !!entry);
  const hasFolder = entries.some((entry) => entry.isDirectory);
  if (hasFolder && hasText) return "mixed";
  if (hasFolder) return "folder";
  if (hasFiles && hasText) return "mixed";
  if (hasFiles) return "files";
  if (hasText) return "text";
  return null;
}

export interface UseSmartIntakeOptions {
  /**
   * Called when paste/drop/clipboard-read detects at least one api-key or codex-auth candidate.
   * Returning true means an external flow took over, such as opening IntakeWizard, so SmartIntake silently dismisses itself.
   */
  onAuthLikeRecognized?: (items: SmartIntakeItem[]) => boolean;
}

export function useSmartIntake(options: UseSmartIntakeOptions = {}) {
  const route = useRoute();
  const router = useRouter();
  const onAuthLikeRecognized = options.onAuthLikeRecognized;
  const items = ref<SmartIntakeItem[]>([]);
  const dragActive = ref(false);
  const dragIntent = shallowRef<SmartDragIntent>(null);
  const busy = ref(false);
  const message = shallowRef<string | null>(null);
  const error = shallowRef<string | null>(null);
  const clipboardWatch = shallowRef(false);
  const clipboardWatchAvailable = shallowRef(
    typeof navigator !== "undefined" && !!navigator.clipboard?.readText,
  );
  const lastClipboardText = shallowRef("");
  let clipboardTimer: number | null = null;
  let dragDepth = 0;

  const recognized = computed(() => items.value.filter((item) => item.kind !== "unknown"));
  const authItems = computed(() =>
    items.value.filter(
      (item) =>
        item.kind === "codex-auth" || item.kind === "api-key" || item.kind === "remote-provider",
    ),
  );
  const configItems = computed(() => items.value.filter((item) => item.kind === "codex-config"));
  const ccsProfileItems = computed(() =>
    items.value.filter((item) => item.kind === "ccswitch-provider" || item.kind === "ccs-profile"),
  );
  const hasPanel = computed(
    () => dragActive.value || items.value.length > 0 || !!message.value || !!error.value,
  );

  function setItems(next: SmartIntakeItem[]) {
    const known = next.filter((item) => item.kind !== "unknown");
    const authLike = known.filter(
      (item) =>
        item.kind === "api-key" || item.kind === "codex-auth" || item.kind === "remote-provider",
    );
    if (authLike.length && onAuthLikeRecognized && onAuthLikeRecognized(authLike)) {
      // IntakeWizard has taken over: leave the config/profile category to legacy SmartIntake cards, if any,
      // but api-key / codex-auth no longer use the old three-button path.
      const remainder = known.filter(
        (item) =>
          item.kind !== "api-key" && item.kind !== "codex-auth" && item.kind !== "remote-provider",
      );
      items.value = remainder;
      message.value = remainder.length
        ? `${remainder.length} item${remainder.length > 1 ? "s" : ""}`
        : null;
      error.value = null;
      return;
    }
    const visible = known.length
      ? known
      : next.filter((item) => textLooksUseful(item.text ?? "")).slice(0, 4);
    items.value = visible;
    message.value = known.length ? `${known.length} item${known.length > 1 ? "s" : ""}` : null;
    error.value = null;
  }

  async function readClipboard() {
    error.value = null;
    try {
      const text = await navigator.clipboard.readText();
      if (!text.trim()) {
        message.value = "Clipboard empty";
        return;
      }
      lastClipboardText.value = text;
      const parsed = itemsFromText(text, "clipboard");
      const authLike = parsed.filter(
        (item) =>
          item.kind === "api-key" || item.kind === "codex-auth" || item.kind === "remote-provider",
      );
      if (authLike.length && onAuthLikeRecognized && onAuthLikeRecognized(authLike)) {
        const remainder = parsed.filter(
          (item) =>
            item.kind !== "api-key" &&
            item.kind !== "codex-auth" &&
            item.kind !== "remote-provider",
        );
        items.value = remainder;
        message.value = remainder.length
          ? `${remainder.length} item${remainder.length > 1 ? "s" : ""}`
          : null;
        return;
      }
      setItems(parsed);
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function pollClipboard() {
    if (!clipboardWatch.value || document.visibilityState !== "visible") return;
    try {
      const text = await navigator.clipboard.readText();
      if (!text.trim() || text === lastClipboardText.value || !textLooksUseful(text)) return;
      lastClipboardText.value = text;
      setItems(itemsFromText(text, "clipboard"));
    } catch (e) {
      clipboardWatch.value = false;
      error.value = e instanceof Error ? e.message : String(e);
      stopClipboardWatch();
    }
  }

  function startClipboardWatch() {
    if (!clipboardWatchAvailable.value) {
      error.value = "Clipboard watch is unavailable in this browser.";
      return;
    }
    clipboardWatch.value = true;
    message.value = "Clipboard watch on";
    void pollClipboard();
    if (clipboardTimer != null) window.clearInterval(clipboardTimer);
    clipboardTimer = window.setInterval(() => void pollClipboard(), 1800);
  }

  function stopClipboardWatch() {
    clipboardWatch.value = false;
    if (clipboardTimer != null) {
      window.clearInterval(clipboardTimer);
      clipboardTimer = null;
    }
  }

  function toggleClipboardWatch() {
    if (clipboardWatch.value) {
      stopClipboardWatch();
      message.value = "Clipboard watch off";
      return;
    }
    startClipboardWatch();
  }

  function onPaste(ev: ClipboardEvent) {
    if (isTextInput(ev.target)) return;
    const text = ev.clipboardData?.getData("text/plain") ?? "";
    const files = Array.from(ev.clipboardData?.files ?? []);
    if (!text.trim() && files.length === 0) return;
    ev.preventDefault();
    void (async () => {
      const fileItems = (
        await Promise.all(files.map((file) => readFileItems(file, "paste")))
      ).flat();
      const textItems = text.trim() ? itemsFromText(text, "paste") : [];
      setItems([...fileItems, ...textItems]);
    })();
  }

  function onDragEnter(ev: DragEvent) {
    if (isTextInput(ev.target)) return;
    dragDepth += 1;
    dragIntent.value = dragIntentFromDataTransfer(ev.dataTransfer);
    dragActive.value = true;
  }

  function onDragOver(ev: DragEvent) {
    if (isTextInput(ev.target)) return;
    ev.preventDefault();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = "copy";
    dragIntent.value = dragIntentFromDataTransfer(ev.dataTransfer);
    dragActive.value = true;
  }

  function onDragLeave() {
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) {
      dragActive.value = false;
      dragIntent.value = null;
    }
  }

  function onDrop(ev: DragEvent) {
    if (isTextInput(ev.target) && !ev.dataTransfer?.files.length) return;
    ev.preventDefault();
    dragDepth = 0;
    dragActive.value = false;
    dragIntent.value = null;
    const dt = ev.dataTransfer;
    if (!dt) return;
    void (async () => {
      const files = await filesFromDataTransfer(dt);
      const fileItems = (
        await Promise.all(files.map((file) => readFileItems(file, "drop")))
      ).flat();
      const text = dt.getData("text/plain");
      const textItems = text.trim() ? itemsFromText(text, "drop") : [];
      setItems([...fileItems, ...textItems]);
    })();
  }

  async function saveCodexConfig() {
    const item = configItems.value[0];
    if (!item?.text) return;
    busy.value = true;
    error.value = null;
    try {
      await api.toolConfigs.saveRaw("codex", item.text);
      message.value = "Config saved";
      items.value = items.value.filter((x) => x.id !== item.id);
      void router.push({ path: route.path, query: { ...route.query, view: "codex" } });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      busy.value = false;
    }
  }

  async function importCcsProfile() {
    const item = ccsProfileItems.value[0];
    if (!item?.ccSwitchUrl && !item?.ccsBundle) return;
    busy.value = true;
    error.value = null;
    try {
      if (item.ccSwitchUrl) {
        await api.providers.importCcSwitchDeeplink({ url: item.ccSwitchUrl });
      } else if (item.ccsBundle) {
        await api.providers.importCcsBundle(item.ccsBundle);
      }
      message.value = "Profile imported";
      items.value = items.value.filter((x) => x.id !== item.id);
      void router.push({ path: "/ui/providers", query: { ...route.query } });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      busy.value = false;
    }
  }

  function goCodex() {
    void router.push({ path: route.path, query: { ...route.query, view: "codex" } });
  }

  function dismiss() {
    items.value = [];
    message.value = null;
    error.value = null;
    dragActive.value = false;
    dragIntent.value = null;
  }

  function onNativeOpenUrl(ev: Event) {
    const url = (ev as CustomEvent<string>).detail;
    if (typeof url === "string" && url.trim()) {
      setItems(itemsFromText(url, "deeplink", "deeplink"));
    }
  }

  onMounted(() => {
    window.addEventListener("paste", onPaste);
    window.addEventListener("dragenter", onDragEnter);
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("drop", onDrop);
    window.addEventListener("vibe:native-open-url", onNativeOpenUrl);
    (window as typeof window & { __vibeOpenUrls?: string[] }).__vibeOpenUrls
      ?.splice(0)
      .forEach((url) => {
        setItems(itemsFromText(url, "deeplink", "deeplink"));
      });
  });

  onUnmounted(() => {
    window.removeEventListener("paste", onPaste);
    window.removeEventListener("dragenter", onDragEnter);
    window.removeEventListener("dragover", onDragOver);
    window.removeEventListener("dragleave", onDragLeave);
    window.removeEventListener("drop", onDrop);
    window.removeEventListener("vibe:native-open-url", onNativeOpenUrl);
    stopClipboardWatch();
  });

  return {
    items,
    recognized,
    authItems,
    configItems,
    ccsProfileItems,
    dragActive,
    dragIntent,
    busy,
    message,
    error,
    clipboardWatch,
    clipboardWatchAvailable,
    hasPanel,
    readClipboard,
    toggleClipboardWatch,
    importCcsProfile,
    saveCodexConfig,
    goCodex,
    dismiss,
  };
}

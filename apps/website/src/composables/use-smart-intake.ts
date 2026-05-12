import { computed, onMounted, onUnmounted, ref, shallowRef } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  api,
  type CcsProfileExportBundle,
  type CredentialInput,
  type Provider,
} from "../api/client.ts";
import { hintsFromAuthJsonTokens } from "../utils/codex-oauth-hints.ts";
import { providerServesCodexCliRoute } from "../utils/client-tools.ts";

export type SmartIntakeKind =
  | "codex-auth"
  | "api-key"
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
  const accessRaw = t?.access_token;
  const refreshRaw = t?.refresh_token;
  const keyRaw = obj.OPENAI_API_KEY;
  const access = typeof accessRaw === "string" && accessRaw.trim() ? accessRaw.trim() : null;
  const apiKey =
    typeof keyRaw === "string" && keyRaw.trim() && keyRaw !== "PROXY_MANAGED"
      ? keyRaw.trim()
      : null;
  if (!access && !apiKey) return null;

  const expiresAt =
    typeof t?.expires_at === "number"
      ? t.expires_at
      : typeof t?.expiry === "number"
        ? t.expiry
        : null;
  const hints = hintsFromAuthJsonTokens(tokens);
  return {
    access,
    refresh: typeof refreshRaw === "string" && refreshRaw.trim() ? refreshRaw.trim() : null,
    exp: expiresAt ?? jwtExp(access),
    apiKey,
    email: hints.oauth_cached_email,
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

function textLooksUseful(raw: string): boolean {
  const trimmed = raw.trim();
  if (!trimmed) return false;
  if (looksLikeCodexConfig(trimmed)) return true;
  if (trimmed.toLowerCase().startsWith(CC_SWITCH_SCHEME)) return true;
  if (trimmed.startsWith("{") && trimmed.includes("schemaVersion") && trimmed.includes("settings"))
    return true;
  if (trimmed.startsWith("{") && AUTH_JSON_HINTS.some((hint) => trimmed.includes(hint)))
    return true;
  return /(?:sk-[A-Za-z0-9_-]{20,}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)/.test(
    trimmed,
  );
}

function parseBareSecret(raw: string): ParsedAuth | null {
  const trimmed = raw.trim();
  const apiKey = trimmed.match(/sk-[A-Za-z0-9_-]{20,}/)?.[0] ?? null;
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
  const jwt = trimmed.match(/eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/)?.[0] ?? null;
  if (!jwt) return null;
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

function itemFromText(raw: string, source: SmartIntakeSource, name = "clipboard"): SmartIntakeItem {
  const trimmed = raw.trim();
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

async function readFileItem(
  file: File,
  source: SmartIntakeSource,
): Promise<SmartIntakeItem | null> {
  if (!shouldReadFile(file)) return null;
  try {
    const text = await file.text();
    return itemFromText(text, source, file.name);
  } catch {
    return null;
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

function preferCodexProvider(providers: Provider[]): Provider | null {
  const codexProviders = providers.filter(providerServesCodexCliRoute);
  return (
    codexProviders.find((p) => p.kind === "openai-responses" && p.base_url.includes("codex")) ??
    codexProviders.find((p) => p.kind === "openai-responses") ??
    codexProviders[0] ??
    null
  );
}

export function useSmartIntake() {
  const route = useRoute();
  const router = useRouter();
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
    items.value.filter((item) => item.kind === "codex-auth" || item.kind === "api-key"),
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
      setItems([itemFromText(text, "clipboard")]);
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
      setItems([itemFromText(text, "clipboard")]);
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
        await Promise.all(files.map((file) => readFileItem(file, "paste")))
      ).filter((item): item is SmartIntakeItem => !!item);
      const textItems = text.trim() ? [itemFromText(text, "paste")] : [];
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
      const fileItems = (await Promise.all(files.map((file) => readFileItem(file, "drop")))).filter(
        (item): item is SmartIntakeItem => !!item,
      );
      const text = dt.getData("text/plain");
      const textItems = text.trim() ? [itemFromText(text, "drop")] : [];
      setItems([...fileItems, ...textItems]);
    })();
  }

  async function importCodexAuth() {
    const item = authItems.value[0];
    if (!item?.auth) return;
    busy.value = true;
    error.value = null;
    try {
      await api.providers.importLocal(["codex"]);
      const providers = await api.providers.list();
      const provider = preferCodexProvider(providers);
      if (!provider) throw new Error("No Codex provider");
      const auth = item.auth;
      const input: CredentialInput = {
        label: auth.email ?? (auth.access ? "Codex OAuth" : "OpenAI Key"),
        auth_ref: auth.apiKey ? `literal:${auth.apiKey}` : null,
        plan_type: auth.plan,
        notes: "smart-intake",
        enabled: true,
        priority: 10,
        oauth_access_token: auth.access,
        oauth_refresh_token: auth.refresh,
        oauth_expires_at: auth.exp,
        oauth_cached_email: auth.email,
        oauth_cached_subject: auth.subject,
        oauth_cached_plan_slug: auth.plan,
      };
      await api.credentials.create(provider.id, input);
      message.value = "Auth imported";
      items.value = items.value.filter((x) => x.id !== item.id);
      void router.push({ path: route.path, query: { ...route.query, view: "codex" } });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      busy.value = false;
    }
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
      void router.push({ path: "/providers", query: { ...route.query } });
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
      setItems([itemFromText(url, "deeplink", "deeplink")]);
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
        setItems([itemFromText(url, "deeplink", "deeplink")]);
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
    importCodexAuth,
    importCcsProfile,
    saveCodexConfig,
    goCodex,
    dismiss,
  };
}

function resolvePort(): number {
  const envRaw =
    typeof import.meta.env.VITE_VIBE_PORT === "string" ? import.meta.env.VITE_VIBE_PORT.trim() : "";
  if (envRaw) {
    const n = parseInt(envRaw, 10);
    if (Number.isInteger(n) && n > 0 && n < 65536) return n;
  }
  const location = globalThis.window?.location;
  const params = new URLSearchParams(location?.search ?? "");
  const raw =
    params.get("port") ?? new URLSearchParams(location?.hash.split("?")[1] ?? "").get("port");
  const n = raw ? parseInt(raw, 10) : NaN;
  return Number.isInteger(n) && n > 0 && n < 65536 ? n : 15917;
}

export const PORT = resolvePort();
const BASE = `http://127.0.0.1:${PORT}`;

export function apiUrl(path: string, base = BASE): string {
  return base + path;
}

async function req<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(apiUrl(path), {
    headers: { "content-type": "application/json" },
    ...options,
  });
  const bodyText = await res.text();
  if (!res.ok) throw new Error(`${res.status} ${bodyText}`);
  // DELETE / no-content responses have an empty body — do not call JSON.parse on ""
  const trimmed = bodyText.trim();
  if (trimmed === "") {
    return undefined as T;
  }
  return JSON.parse(trimmed) as T;
}

export type ProviderKind = "anthropic" | "openai-chat" | "openai-responses" | "gemini-native";

export interface ModelAlias {
  alias: string;
  upstream_model: string;
}

export interface ProviderProtocol {
  kind: ProviderKind;
  base_url: string;
  model_aliases: ModelAlias[];
}

export interface Upstream {
  id: string;
  provider_id: string;
  kind: ProviderKind;
  base_url: string;
  credential_id: string | null;
  cb_key: string;
  enabled: boolean;
  priority: number;
}

export interface ProviderUpstreamSummary {
  provider_id: string;
  total_upstreams: number;
  enabled_upstreams: number;
  endpoint_count: number;
  credential_count: number;
  sample_upstreams: Upstream[];
}

export interface Provider {
  id: string;
  name: string;
  group_name: string | null;
  avatar_url: string | null;
  upstreams: Upstream[];
  upstream_summary: ProviderUpstreamSummary | null;
  kind: ProviderKind;
  base_url: string;
  protocols?: ProviderProtocol[];
  host?: string | null;
  auth_ref: string | null;
  enabled: boolean;
  priority: number;
  supports_websocket: boolean | null;
  passthrough_mode: boolean;
  remote_models: string[];
  remote_models_fetched_at: number | null;
  last_speedtest: ProviderSpeedtestResult | null;
  model_aliases: ModelAlias[];
  created_at: number;
  updated_at: number;
}
export interface ProviderSpeedtestResult {
  url: string;
  ok: boolean;
  latency_ms: number | null;
  status: number | null;
  error: string | null;
  checked_at: number;
}
export interface ProviderInput {
  name: string;
  group_name: string | null;
  avatar_url: string | null;
  kind: ProviderKind;
  base_url: string;
  protocols?: ProviderProtocol[];
  host?: string | null;
  auth_ref: string | null;
  enabled: boolean;
  priority: number;
  supports_websocket: boolean | null;
  passthrough_mode: boolean;
  model_aliases: ModelAlias[];
}

export interface ProvidersOverview {
  rolling_hours: number;
  providers: Provider[];
  health: ProviderHealthSummary[];
  pools: ProviderAuthPoolSummary[];
  credentials: Record<string, Credential[]>;
  upstreams: Record<string, Upstream[]>;
  codex_plans: Record<string, ProviderCodexPlanItem[]>;
}
export type AppLogLevel = "debug" | "info" | "warn" | "error";

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface AppLogEvent {
  ts: number;
  level: AppLogLevel;
  event_type?: string;
  payload?: JsonValue;
  category: string;
  message: string;
  detail: string | null;
}
export interface WebCompatibility {
  api: number;
  min_web_api: number;
}

export interface Status {
  version: string;
  web_compatibility?: WebCompatibility;
  uptime_secs: number;
  port: number;
  providers_total: number;
  providers_enabled: number;
  requests_last_hour: number;
  codex_ws_active?: number;
  codex_ws_total?: number;
  codex_ws_requests_total?: number;
  codex_http_responses_total?: number;
  codex_last_transport?: string | null;
}
export interface ClientStatus {
  client: string;
  config_path: string;
  config_exists: boolean;
  taken_over: boolean;
  expected_base_url: string;
  configured_base_url: string | null;
  auth_proxy_managed: boolean | null;
  model_overrides_present: string[];
  notes: string[];
}
export interface ClientTakeoverResult {
  client: string;
  config_path: string;
  backup_path: string | null;
  status: ClientStatus;
}

export type CodexSummaryClientKind = "app" | "cli" | "unknown";
export type CodexSummaryStyle =
  | "formula_compact"
  | "plain_compact"
  | "inline_chips"
  | "status_bar"
  | "english_light"
  | "chinese_light"
  | "formula_labeled"
  | "ascii_plain";

export interface CodexSummaryClientConfig {
  enabled: boolean;
  style: CodexSummaryStyle;
  prefix?: string | null;
  suffix?: string | null;
}

export interface CodexSummaryLabelOverrides {
  speed?: string | null;
  input?: string | null;
  output?: string | null;
  cache?: string | null;
  latency?: string | null;
  first_token?: string | null;
}

export interface CodexSummaryConfig {
  enabled: boolean;
  show_speed: boolean;
  show_input: boolean;
  show_output: boolean;
  show_cache: boolean;
  show_latency: boolean;
  show_first_token: boolean;
  speed_decimal_places: number;
  separator: string;
  label_overrides: CodexSummaryLabelOverrides;
  clients: Record<CodexSummaryClientKind, CodexSummaryClientConfig>;
}

export interface RealtimeRequest {
  id: string;
  started_at: number;
  updated_at: number;
  app: string | null;
  provider_id: string | null;
  credential_id: string | null;
  requested_model: string | null;
  upstream_model: string | null;
  wire: string | null;
  route_prefix: string | null;
  client_transport: string | null;
  phase: string;
  status_code: number | null;
  error: string | null;
  active_output_tokens_per_sec: number | null;
  active_cost_usd_per_hour: number | null;
  active_upstream_bytes_per_sec: number;
  active_downstream_bytes_per_sec: number;
  output_tokens_so_far: number;
  upstream_bytes_so_far: number;
  client_bytes_so_far: number;
  upstream_first_byte_ms: number | null;
  client_first_write_ms: number | null;
}
export interface RealtimeProvider {
  provider_id: string;
  provider_name: string;
  active_requests: number;
  active_output_tokens_per_sec: number;
  active_cost_usd_per_hour: number | null;
  active_upstream_bytes_per_sec: number;
  active_downstream_bytes_per_sec: number;
  output_tokens_so_far: number;
  upstream_bytes_so_far: number;
  client_bytes_so_far: number;
}
export interface RealtimeSnapshot {
  now: number;
  active_requests: RealtimeRequest[];
  recent_requests: RealtimeRequest[];
  providers: RealtimeProvider[];
  active_count: number;
  active_output_tokens_per_sec: number;
  active_cost_usd_per_hour: number | null;
  active_upstream_bytes_per_sec: number;
  active_downstream_bytes_per_sec: number;
  codex_ws_active: number;
  codex_last_transport: string | null;
}
export interface UsageSummary {
  range: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  estimated_cost_usd: string;
}
export interface ProviderHealth {
  provider_id: string;
  is_healthy: boolean;
  circuit_state: string;
  consecutive_failures: number;
  total_requests: number;
  total_successes: number;
  total_failures: number;
  success_rate: number;
  last_success_at: number | null;
  last_failure_at: number | null;
  last_error: string | null;
  avg_latency_ms: number | null;
  updated_at: number;
}

/** `GET /_vp/providers/:id/health` — cumulative DB health and credential/provider counters. */
export interface ProviderHealthSummary {
  cumulative: ProviderHealth;
  rolling_hours: number;
  rolling: ProviderStat | null;
}

export interface CredentialPoolStatus {
  credential_id: string;
  label: string;
  enabled: boolean;
  auth_mode: string;
  circuit_state: string;
  circuit_open: boolean;
  circuit_open_remaining_secs: number | null;
  consecutive_failures: number;
  is_rate_limited: boolean;
  rl_requests_remaining: number | null;
  rl_requests_reset_at: number | null;
  rl_tokens_remaining: number | null;
  rl_tokens_reset_at: number | null;
  oauth_expires_at: number | null;
  last_error: string | null;
  last_used_at: number | null;
  rolling_requests: number;
  rolling_successes: number;
  rolling_failures: number;
  rolling_avg_latency_ms: number | null;
}

export interface ProviderAuthPoolSummary {
  provider_id: string;
  provider_name: string;
  kind: ProviderKind;
  rolling_hours: number;
  total_credentials: number;
  enabled_credentials: number;
  available_credentials: number;
  rate_limited_credentials: number;
  open_circuit_credentials: number;
  provider_circuit_open_remaining_secs: number | null;
  provider_circuit_state: string;
  provider_circuit_open: boolean;
  provider_last_error: string | null;
  credentials: CredentialPoolStatus[];
}

/**
 * Shape returned by `GET /_vp/providers/:id/health` on current gateways.
 * Older binaries returned a flat ProviderHealth without `cumulative` — reject those responses.
 */
export function isProviderHealthSummary(x: unknown): x is ProviderHealthSummary {
  if (typeof x !== "object" || x === null) return false;
  const o = x as Record<string, unknown>;
  if (!("cumulative" in o) || typeof o.cumulative !== "object" || o.cumulative === null)
    return false;
  const cum = o.cumulative as Record<string, unknown>;
  if (typeof cum.circuit_state !== "string") return false;
  if (!("rolling_hours" in o) || typeof o.rolling_hours !== "number") return false;
  return true;
}

/** Latest Codex Plan snapshot parsed from upstream `x-codex-*` headers. */
export interface CredentialPlanSnapshot {
  id: string;
  credential_id: string;
  captured_at: number;
  codex_5h_used_percent: number | null;
  codex_7d_used_percent: number | null;
  codex_5h_reset_after_seconds: number | null;
  codex_7d_reset_after_seconds: number | null;
  codex_primary_used_percent: number | null;
  codex_secondary_used_percent: number | null;
  summary: string | null;
  source: string;
}

/** Per-credential latest plan for an official ChatGPT Codex provider (`GET /_vp/providers/:id/codex-plan`). */
export interface ProviderCodexPlanItem {
  credential_id: string;
  label: string;
  plan: CredentialPlanSnapshot | null;
}

/** Result of `POST /_vp/providers/:id/codex-plan/refresh`. */
export interface CodexPlanRefreshResult {
  attempted: number;
  ok: number;
  errors: string[];
}

export interface CodexAppProcess {
  pid: number;
  role: string;
  command: string;
}

export interface CodexAppStatus {
  app_path: string;
  installed: boolean;
  running: boolean;
  main_pid: number | null;
  process_count: number;
  processes: CodexAppProcess[];
}

export interface CodexAppActionResult {
  action: string;
  status: CodexAppStatus;
}
export interface HealthSummary {
  providers: ProviderHealth[];
  total_providers: number;
  healthy_providers: number;
}
export interface ModelStat {
  model: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
}
export interface ProviderStat {
  provider_id: string;
  provider_name: string;
  requests: number;
  successes: number;
  failures: number;
  success_rate: number;
  avg_latency_ms: number;
  input_tokens: number;
  output_tokens: number;
  /** End-to-end: sum(out)/sum(latency) for successful requests with latency_ms > 0 in the window. */
  output_tokens_per_sec: number;
  /** Decode segment: sum(out)/sum(latency - first token) for requests with first token time and latency > first token. */
  decode_output_tokens_per_sec: number;
  /** Counts in the same rolling window as other provider fields */
  err_429?: number;
  err_503?: number;
  err_4xx_other?: number;
  err_5xx_other?: number;
}
export interface DashboardStats {
  /** Present on vibe ≥ build with windowed dashboard API */
  window_hours?: number;
  window_label?: string;
  requests_in_window?: number;
  success_rate_in_window?: number;
  input_tokens_in_window?: number;
  output_tokens_in_window?: number;
  output_tokens_per_sec_in_window?: number;
  decode_output_tokens_per_sec_in_window?: number;
  requests_last_hour: number;
  requests_last_24h: number;
  success_rate_last_hour: number;
  avg_latency_ms: number;
  p95_latency_ms: number;
  input_tokens_last_24h: number;
  output_tokens_last_24h: number;
  top_models: ModelStat[];
  per_provider: ProviderStat[];
}

export interface Credential {
  id: string;
  provider_id: string;
  label: string;
  /** auth_ref mode (keyring:, env:, literal:). File-based schemes removed — import Codex auth via UI. */
  auth_ref: string | null;
  plan_type: string | null;
  notes: string | null;
  enabled: boolean;
  priority: number;
  // OAuth direct-storage fields
  oauth_access_token: string | null;
  /** true when a refresh_token is stored (token itself is never returned). */
  oauth_has_refresh: boolean;
  oauth_expires_at: number | null;
  // Rate-limit state
  rl_requests_limit: number | null;
  rl_requests_remaining: number | null;
  rl_requests_reset_at: number | null;
  rl_tokens_limit: number | null;
  rl_tokens_remaining: number | null;
  rl_tokens_reset_at: number | null;
  last_used_at: number | null;
  last_error: string | null;
  consecutive_failures: number;
  created_at: number;
  updated_at: number;
  /** Stable hash for duplicate-import detection (`fp:…`). */
  auth_fingerprint?: string | null;
  /** Parsed from OAuth access JWT using the same claim namespace as Codex `parse_chatgpt_jwt_claims`; null when absent or undecodable. */
  oauth_account_email?: string | null;
  oauth_account_subject?: string | null;
  /** Raw JWT `https://api.openai.com/auth.chatgpt_plan_type` value, such as plus or pro; secondary display only. */
  oauth_chatgpt_plan_slug?: string | null;
  remote_models?: string[];
  remote_models_fetched_at?: number | null;
  balance?: ProviderBalanceSnapshot | null;
  usage?: ProviderBalanceSnapshot | null;
  balance_fetched_at?: number | null;
  // Upstream vendor / login / group
  upstream_vendor?: CredentialVendor | null;
  upstream_username?: string | null;
  /** true when a session token is cached (token itself never returned). */
  upstream_has_session?: boolean;
  upstream_session_expires_at?: number | null;
  upstream_group?: string | null;
  price_multiplier?: number;
  windows?: UsageWindow[];
}

export type CredentialVendor =
  | "generic"
  | "new-api"
  | "sub2-api"
  | "anthropic-payg"
  | "anthropic-plan";

export interface UsageWindow {
  label: string;
  used_usd: number;
  limit_usd: number | null;
  /** 0–100 or null when limit unknown */
  used_pct: number | null;
  reset_at: number | null;
}

export interface UpstreamGroupInfo {
  id: string;
  name: string;
  description: string | null;
  platform: string | null;
  rate_multiplier: number;
}

export interface CredentialLoginRequest {
  username: string;
  password: string;
}

export interface CredentialLoginResponse {
  ok: boolean;
  note: string | null;
}

export interface ProviderBalanceSnapshot {
  currency: string;
  balance: string | null;
  remaining: string | null;
  used: string | null;
  total: string | null;
  period: string | null;
  note: string | null;
}

export interface CredentialInput {
  label: string;
  auth_ref: string | null;
  plan_type: string | null;
  notes: string | null;
  enabled: boolean;
  priority: number;
  oauth_access_token: string | null;
  oauth_refresh_token: string | null;
  oauth_expires_at: number | null;
  oauth_cached_email?: string | null;
  oauth_cached_subject?: string | null;
  oauth_cached_plan_slug?: string | null;
  upstream_vendor?: CredentialVendor | null;
  upstream_username?: string | null;
  upstream_group?: string | null;
  price_multiplier?: number;
}

export interface ExtraCredential {
  label: string;
  source_path: string;
  token_ok: boolean;
}

export interface LocalCandidate {
  client: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  /** Runtime auth hint when not using DB OAuth (e.g. Claude literal). Null for Codex after scan. */
  auth_ref: string | null;
  token_ok: boolean;
  /** True when ANTHROPIC_AUTH_TOKEN = "PROXY_MANAGED" — vibe already handles auth for this client. */
  proxy_managed?: boolean;
  source_path: string;
  default_aliases: ModelAlias[];
  extra_credentials: ExtraCredential[];
}

export interface CcsProfileExportBundle {
  schemaVersion: 1;
  exportedAt?: string;
  profile: {
    name: string;
    target?: string;
  };
  settings: Record<string, unknown>;
}

export interface CcSwitchDeeplinkImport {
  url: string;
}

export type ToolConfigId = "codex" | "claude";

export interface ToolConfigRaw {
  tool: ToolConfigId;
  path: string;
  exists: boolean;
  mtime_ms: number | null;
  raw_text: string;
}

export interface CodexFileEntry {
  name: string;
  path: string;
  kind: "file" | "dir";
  size: number | null;
  mtime_ms: number | null;
}

export interface CodexFileList {
  root: string;
  path: string;
  abs_path: string;
  entries: CodexFileEntry[];
}

export interface CodexFile {
  root: string;
  path: string;
  abs_path: string;
  exists: boolean;
  mtime_ms: number | null;
  raw_text: string;
}

export interface CodexProviderSettings {
  id: string;
  name: string;
  base_url: string;
  wire_api: string;
  requires_openai_auth: boolean;
  supports_websockets: boolean;
  websocket_connect_timeout_ms: number;
  request_max_retries: number;
  stream_max_retries: number;
  stream_idle_timeout_ms: number;
}

export interface CodexFeatureSetting {
  key: string;
  enabled: boolean;
  default_enabled: boolean;
  stage: string;
}

export interface CodexFeatureSettingInput {
  key: string;
  enabled: boolean;
}

export interface CodexProviderSettingsInput {
  id?: string | null;
  name?: string | null;
  base_url: string;
  wire_api?: string | null;
  requires_openai_auth?: boolean | null;
  supports_websockets: boolean;
  websocket_connect_timeout_ms?: number | null;
  request_max_retries?: number | null;
  stream_max_retries?: number | null;
  stream_idle_timeout_ms?: number | null;
}

export interface CodexConfigSettingsInput {
  model_provider?: string | null;
  provider: CodexProviderSettingsInput;
  features?: CodexFeatureSettingInput[];
}

export interface CodexConfigSettings {
  tool: ToolConfigId;
  path: string;
  exists: boolean;
  mtime_ms: number | null;
  model_provider: string;
  provider: CodexProviderSettings;
  features: CodexFeatureSetting[];
}

export const api = {
  ping: () => req<{ ok: boolean }>("/health"),
  status: () => req<Status>("/status"),
  providers: {
    list: () => req<Provider[]>("/_vp/providers"),
    overview: (hours = 24) => req<ProvidersOverview>(`/_vp/providers/overview?hours=${hours}`),
    create: (input: ProviderInput) =>
      req<Provider>("/_vp/providers", {
        method: "POST",
        body: JSON.stringify(input),
      }),
    update: (id: string, input: ProviderInput) =>
      req<Provider>(`/_vp/providers/${id}`, {
        method: "PUT",
        body: JSON.stringify(input),
      }),
    delete: (id: string) => req<void>(`/_vp/providers/${id}`, { method: "DELETE" }),
    health: (id: string, hours = 24) =>
      req<ProviderHealthSummary>(`/_vp/providers/${id}/health?hours=${hours}`),
    healthAll: (hours = 24) => req<ProviderHealthSummary[]>(`/_vp/providers/health?hours=${hours}`),
    pool: (id: string, hours = 24) =>
      req<ProviderAuthPoolSummary>(`/_vp/providers/${id}/pool?hours=${hours}`),
    pools: (hours = 24) => req<ProviderAuthPoolSummary[]>(`/_vp/pools?hours=${hours}`),
    resetCircuit: (id: string) =>
      req<ProviderHealth>(`/_vp/providers/${id}/circuit/reset`, {
        method: "POST",
      }),
    refreshModels: (id: string) =>
      req<Provider>(`/_vp/providers/${id}/models/refresh`, { method: "POST" }),
    scanLocal: () => req<LocalCandidate[]>("/_vp/providers/import-local"),
    importLocal: (clients: string[]) =>
      req<Provider[]>("/_vp/providers/import-local", {
        method: "POST",
        body: JSON.stringify(clients),
      }),
    importCcsBundle: (bundle: CcsProfileExportBundle) =>
      req<Provider>("/_vp/providers/import-ccs", {
        method: "POST",
        body: JSON.stringify(bundle),
      }),
    importCcSwitchDeeplink: (input: CcSwitchDeeplinkImport) =>
      req<Provider>("/_vp/providers/import-ccswitch", {
        method: "POST",
        body: JSON.stringify(input),
      }),
    codexPlan: (providerId: string) =>
      req<ProviderCodexPlanItem[]>(`/_vp/providers/${providerId}/codex-plan`),
    codexPlans: () => req<Record<string, ProviderCodexPlanItem[]>>("/_vp/providers/codex-plan"),
    refreshCodexPlan: (providerId: string) =>
      req<CodexPlanRefreshResult>(`/_vp/providers/${providerId}/codex-plan/refresh`, {
        method: "POST",
      }),
  },
  health: {
    all: () => req<HealthSummary>("/_vp/health/providers"),
  },
  credentials: {
    all: () => req<Record<string, Credential[]>>("/_vp/credentials"),
    list: (providerId: string) => req<Credential[]>(`/_vp/providers/${providerId}/credentials`),
    create: (providerId: string, input: CredentialInput) =>
      req<Credential>(`/_vp/providers/${providerId}/credentials`, {
        method: "POST",
        body: JSON.stringify(input),
      }),
    update: (id: string, input: CredentialInput) =>
      req<Credential>(`/_vp/credentials/${id}`, {
        method: "PUT",
        body: JSON.stringify(input),
      }),
    delete: (id: string) => req<void>(`/_vp/credentials/${id}`, { method: "DELETE" }),
    plan: (id: string) => req<CredentialPlanSnapshot | null>(`/_vp/credentials/${id}/plan`),
    refreshPlan: (id: string) =>
      req<CredentialPlanSnapshot>(`/_vp/credentials/${id}/plan/refresh`, {
        method: "POST",
      }),
    refreshModels: (id: string) =>
      req<Credential>(`/_vp/credentials/${id}/models/refresh`, {
        method: "POST",
      }),
    refreshBalance: (id: string) =>
      req<Credential>(`/_vp/credentials/${id}/balance/refresh`, {
        method: "POST",
      }),
    login: (id: string, body: CredentialLoginRequest) =>
      req<CredentialLoginResponse>(`/_vp/credentials/${id}/login`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
    groups: (id: string) => req<UpstreamGroupInfo[]>(`/_vp/credentials/${id}/groups`),
  },
  appLogs: {
    list: (limit = 200, since?: number) =>
      req<AppLogEvent[]>(
        `/_vp/app-logs?limit=${limit}${since !== undefined ? `&since=${since}` : ""}`,
      ),
  },
  usage: (hours = 24) => req<UsageSummary>(`/_vp/usage/summary?hours=${hours}`),
  stats: (hours = 24) => req<DashboardStats>(`/_vp/stats/dashboard?hours=${hours}`),
  realtime: () => req<RealtimeSnapshot>("/_vp/realtime"),
  clients: {
    status: (client: string) =>
      req<ClientStatus>(`/_vp/clients/${encodeURIComponent(client)}/status`),
    takeover: (client: string) =>
      req<ClientTakeoverResult>(`/_vp/clients/${encodeURIComponent(client)}/takeover`, {
        method: "POST",
      }),
    restore: (client: string) =>
      req<ClientTakeoverResult>(`/_vp/clients/${encodeURIComponent(client)}/restore`, {
        method: "POST",
      }),
  },
  toolConfigs: {
    getRaw: (tool: ToolConfigId) => req<ToolConfigRaw>(`/_vp/tool-configs/${tool}/raw`),
    saveRaw: (tool: ToolConfigId, rawText: string) =>
      req<ToolConfigRaw>(`/_vp/tool-configs/${tool}/raw`, {
        method: "PUT",
        body: JSON.stringify({ raw_text: rawText }),
      }),
    getCodexSettings: () => req<CodexConfigSettings>("/_vp/tool-configs/codex/settings"),
    saveCodexSettings: (input: CodexConfigSettingsInput) =>
      req<CodexConfigSettings>("/_vp/tool-configs/codex/settings", {
        method: "PUT",
        body: JSON.stringify(input),
      }),
  },
  codexApp: {
    status: () => req<CodexAppStatus>("/_vp/codex-app/status"),
    open: () => req<CodexAppActionResult>("/_vp/codex-app/open", { method: "POST" }),
    quit: () => req<CodexAppActionResult>("/_vp/codex-app/quit", { method: "POST" }),
    restart: () => req<CodexAppActionResult>("/_vp/codex-app/restart", { method: "POST" }),
  },
};

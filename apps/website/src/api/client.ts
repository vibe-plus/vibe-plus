function resolvePort(): number {
  const envRaw =
    typeof import.meta.env.VITE_VIBE_PORT === "string" ? import.meta.env.VITE_VIBE_PORT.trim() : "";
  if (envRaw) {
    const n = parseInt(envRaw, 10);
    if (Number.isInteger(n) && n > 0 && n < 65536) return n;
  }
  const params = new URLSearchParams(window.location.search);
  const raw =
    params.get("port") ?? new URLSearchParams(window.location.hash.split("?")[1] ?? "").get("port");
  const n = raw ? parseInt(raw, 10) : NaN;
  return Number.isInteger(n) && n > 0 && n < 65536 ? n : 15917;
}

export const PORT = resolvePort();
const BASE = `http://127.0.0.1:${PORT}`;

async function req<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(BASE + path, {
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
export type RouteTier = "high" | "low" | "default";

export interface ModelAlias {
  alias: string;
  upstream_model: string;
}
export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  auth_ref: string | null;
  enabled: boolean;
  priority: number;
  model_aliases: ModelAlias[];
  created_at: number;
  updated_at: number;
}
export interface ProviderInput {
  name: string;
  kind: ProviderKind;
  base_url: string;
  auth_ref: string | null;
  enabled: boolean;
  priority: number;
  model_aliases: ModelAlias[];
}
export interface Route {
  id: string;
  name: string;
  match_model: string;
  target_provider_id: string | null;
  target_model: string | null;
  tier: RouteTier;
  priority: number;
}
export interface RequestLog {
  id: string;
  started_at: number;
  app: string | null;
  provider_id: string | null;
  requested_model: string | null;
  upstream_model: string | null;
  status_code: number | null;
  error: string | null;
  latency_ms: number | null;
  first_token_ms: number | null;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  estimated_cost_usd: string;
  wire?: string | null;
  route_prefix?: string | null;
  credential_id?: string | null;
  cb_key?: string | null;
  upstream_http_status?: number | null;
  upstream_error_preview?: string | null;
  dedupe_key?: string | null;
  /** Present on `GET /_vp/logs/:id`; omitted from list endpoint to save bandwidth. */
  request_body?: string | null;
  response_body?: string | null;
  /** Codex WS 等：网关把上游 Chat 转成 Responses 后实际发给客户端的帧（多行 JSON）。 */
  client_response_body?: string | null;
}
export interface LogPage {
  items: RequestLog[];
  total: number;
  limit: number;
  offset: number;
}
export interface Status {
  version: string;
  uptime_secs: number;
  port: number;
  providers_total: number;
  providers_enabled: number;
  requests_last_hour: number;
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

/** `GET /_vp/providers/:id/health` — cumulative DB health + optional rolling window from `request_logs`. */
export interface ProviderHealthSummary {
  cumulative: ProviderHealth;
  rolling_hours: number;
  rolling: ProviderStat | null;
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
}

export interface CredentialInput {
  label: string;
  auth_ref: string | null;
  plan_type: string | null;
  notes: string | null;
  enabled: boolean;
  priority: number;
  /** OAuth: access token stored directly in SQLite. */
  oauth_access_token: string | null;
  /** OAuth: refresh token (write-only; never returned by server). */
  oauth_refresh_token: string | null;
  oauth_expires_at: number | null;
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
  source_path: string;
  default_aliases: ModelAlias[];
  extra_credentials: ExtraCredential[];
}

export interface LogFilters {
  limit?: number;
  offset?: number;
  since?: number;
  provider_id?: string;
  status?: "ok" | "error";
}

export const api = {
  ping: () => req<{ ok: boolean }>("/health"),
  status: () => req<Status>("/status"),
  providers: {
    list: () => req<Provider[]>("/_vp/providers"),
    create: (input: ProviderInput) =>
      req<Provider>("/_vp/providers", { method: "POST", body: JSON.stringify(input) }),
    update: (id: string, input: ProviderInput) =>
      req<Provider>(`/_vp/providers/${id}`, { method: "PUT", body: JSON.stringify(input) }),
    delete: (id: string) => req<void>(`/_vp/providers/${id}`, { method: "DELETE" }),
    health: (id: string, hours = 24) =>
      req<ProviderHealthSummary>(`/_vp/providers/${id}/health?hours=${hours}`),
    resetCircuit: (id: string) =>
      req<ProviderHealth>(`/_vp/providers/${id}/circuit/reset`, { method: "POST" }),
    scanLocal: () => req<LocalCandidate[]>("/_vp/providers/import-local"),
    importLocal: (clients: string[]) =>
      req<Provider[]>("/_vp/providers/import-local", {
        method: "POST",
        body: JSON.stringify(clients),
      }),
    codexPlan: (providerId: string) =>
      req<ProviderCodexPlanItem[]>(`/_vp/providers/${providerId}/codex-plan`),
    refreshCodexPlan: (providerId: string) =>
      req<CodexPlanRefreshResult>(`/_vp/providers/${providerId}/codex-plan/refresh`, {
        method: "POST",
      }),
  },
  health: {
    all: () => req<HealthSummary>("/_vp/health/providers"),
  },
  credentials: {
    list: (providerId: string) => req<Credential[]>(`/_vp/providers/${providerId}/credentials`),
    create: (providerId: string, input: CredentialInput) =>
      req<Credential>(`/_vp/providers/${providerId}/credentials`, {
        method: "POST",
        body: JSON.stringify(input),
      }),
    update: (id: string, input: CredentialInput) =>
      req<Credential>(`/_vp/credentials/${id}`, { method: "PUT", body: JSON.stringify(input) }),
    delete: (id: string) => req<void>(`/_vp/credentials/${id}`, { method: "DELETE" }),
    plan: (id: string) => req<CredentialPlanSnapshot | null>(`/_vp/credentials/${id}/plan`),
    refreshPlan: (id: string) =>
      req<CredentialPlanSnapshot>(`/_vp/credentials/${id}/plan/refresh`, { method: "POST" }),
  },
  routes: { list: () => req<Route[]>("/_vp/routes") },
  logs: {
    list: (f: LogFilters = {}) => {
      const p = new URLSearchParams();
      if (f.limit) p.set("limit", String(f.limit));
      if (f.offset) p.set("offset", String(f.offset));
      if (f.since) p.set("since", String(f.since));
      if (f.provider_id) p.set("provider_id", f.provider_id);
      if (f.status) p.set("status", f.status);
      return req<LogPage>(`/_vp/logs?${p}`);
    },
    get: (id: string) => req<RequestLog>(`/_vp/logs/${encodeURIComponent(id)}`),
  },
  usage: (hours = 24) => req<UsageSummary>(`/_vp/usage/summary?hours=${hours}`),
  stats: (hours = 24) => req<DashboardStats>(`/_vp/stats/dashboard?hours=${hours}`),
};

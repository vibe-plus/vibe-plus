function resolvePort(): number {
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
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
  return res.json() as Promise<T>;
}

export type ProviderKind = "anthropic" | "openai-compat" | "openai-responses" | "gemini-native";
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
}
export interface DashboardStats {
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
    health: (id: string) => req<ProviderHealth>(`/_vp/providers/${id}/health`),
  },
  health: {
    all: () => req<HealthSummary>("/_vp/health/providers"),
  },
  routes: { list: () => req<Route[]>("/_vp/routes") },
  logs: (f: LogFilters = {}) => {
    const p = new URLSearchParams();
    if (f.limit) p.set("limit", String(f.limit));
    if (f.offset) p.set("offset", String(f.offset));
    if (f.since) p.set("since", String(f.since));
    if (f.provider_id) p.set("provider_id", f.provider_id);
    if (f.status) p.set("status", f.status);
    return req<LogPage>(`/_vp/logs?${p}`);
  },
  usage: (hours = 24) => req<UsageSummary>(`/_vp/usage/summary?hours=${hours}`),
  stats: (hours = 24) => req<DashboardStats>(`/_vp/stats/dashboard?hours=${hours}`),
};

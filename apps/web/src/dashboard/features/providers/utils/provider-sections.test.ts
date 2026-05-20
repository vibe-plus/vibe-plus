import { describe, expect, test } from "vite-plus/test";
import type {
  Provider,
  ProviderAuthPoolSummary,
  ProviderHealthSummary,
} from "../../../api/client.ts";
import { buildProviderSections } from "./provider-sections.ts";

function provider(
  partial: Partial<Provider> & Pick<Provider, "id" | "name" | "enabled">,
): Provider {
  return {
    id: partial.id,
    name: partial.name,
    group_name: partial.group_name ?? "Default",
    avatar_url: null,
    kind: partial.kind ?? "openai-responses",
    protocols: partial.protocols,
    base_url: partial.base_url ?? `https://${partial.id}.example.com`,
    host: null,
    auth_ref: null,
    enabled: partial.enabled,
    priority: partial.priority ?? 100,
    supports_websocket: null,
    passthrough_mode: false,
    model_aliases: [],
    remote_models: [],
    remote_models_fetched_at: null,
    upstreams: [],
    upstream_summary: null,
    last_speedtest: partial.last_speedtest ?? null,
    created_at: 0,
    updated_at: 0,
  };
}

function health(
  providerId: string,
  requests: number,
  successRate: number,
  avgLatencyMs: number,
): ProviderHealthSummary {
  return {
    rolling_hours: 24,
    rolling:
      requests > 0
        ? {
            provider_id: providerId,
            provider_name: providerId,
            requests,
            successes: Math.round(requests * successRate),
            failures: requests - Math.round(requests * successRate),
            success_rate: successRate,
            avg_latency_ms: avgLatencyMs,
            input_tokens: 0,
            output_tokens: 0,
            output_tokens_per_sec: 0,
            decode_output_tokens_per_sec: 0,
            err_429: 0,
            err_503: 0,
            err_4xx_other: 0,
            err_5xx_other: 0,
          }
        : null,
    cumulative: {
      provider_id: providerId,
      is_healthy: true,
      circuit_state: "closed",
      consecutive_failures: 0,
      total_requests: requests,
      total_successes: Math.round(requests * successRate),
      total_failures: requests - Math.round(requests * successRate),
      success_rate: requests > 0 ? successRate : 1,
      last_success_at: null,
      last_failure_at: null,
      last_error: null,
      avg_latency_ms: requests > 0 ? avgLatencyMs : null,
      updated_at: 0,
    },
  };
}

function pool(providerId: string, availableCredentials: number): ProviderAuthPoolSummary {
  return {
    provider_id: providerId,
    provider_name: providerId,
    kind: "openai-responses",
    rolling_hours: 24,
    total_credentials: availableCredentials,
    enabled_credentials: availableCredentials,
    available_credentials: availableCredentials,
    rate_limited_credentials: 0,
    open_circuit_credentials: 0,
    provider_circuit_open_remaining_secs: null,
    provider_circuit_state: "closed",
    provider_circuit_open: false,
    provider_last_error: null,
    credentials: [],
  };
}

describe("buildProviderSections", () => {
  test("keeps enabled providers before disabled providers and ranks enabled providers by quality", () => {
    const providers = [
      provider({ id: "disabled-fast", name: "A disabled", enabled: false }),
      provider({ id: "enabled-slow", name: "B enabled slow", enabled: true }),
      provider({ id: "enabled-fast", name: "C enabled fast", enabled: true }),
    ];

    const sections = buildProviderSections({
      providers,
      selectedTool: null,
      healthMap: {
        "disabled-fast": health("disabled-fast", 10, 1, 50),
        "enabled-slow": health("enabled-slow", 10, 0.8, 2500),
        "enabled-fast": health("enabled-fast", 10, 1, 100),
      },
      poolByProviderId: {
        "disabled-fast": pool("disabled-fast", 3),
        "enabled-slow": pool("enabled-slow", 1),
        "enabled-fast": pool("enabled-fast", 3),
      },
    });

    expect(sections[0].providers.map((card) => card.provider.id)).toEqual([
      "enabled-fast",
      "enabled-slow",
      "disabled-fast",
    ]);
  });

  test("does not report idle enabled providers as 100% successful", () => {
    const sections = buildProviderSections({
      providers: [provider({ id: "idle", name: "Idle", enabled: true })],
      selectedTool: null,
      healthMap: { idle: health("idle", 0, 1, 0) },
      poolByProviderId: { idle: pool("idle", 1) },
    });

    expect(sections[0].providers[0].sortReason).toContain("no traffic");
    expect(sections[0].providers[0].sortReason.includes("100% ok")).toBe(false);
  });
});

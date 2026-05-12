import { PORT, type VibeConfig } from "../api/client.ts";

export function defaultVibeConfig(): VibeConfig {
  return {
    server: { host: "127.0.0.1", port: PORT },
    failover: {
      failure_threshold: 3,
      success_threshold: 2,
      open_timeout_secs: 30,
      inject_cache: true,
    },
    log: {
      bodies: false,
      redact_sensitive_headers: true,
    },
    codex: {
      summary: {
        enabled: true,
        show_speed: true,
        show_input: true,
        show_output: true,
        show_cache: true,
        show_latency: false,
        speed_decimal_places: 1,
        clients: {
          app: { enabled: true, style: "formula_compact" },
          cli: { enabled: true, style: "plain_compact" },
          unknown: { enabled: false, style: "inline_chips" },
        },
      },
    },
    claude: {
      native: {
        manage_settings_json: true,
        proxy_env: true,
        clear_model_overrides_on_takeover: true,
        write_model_overrides_on_takeover: false,
        default_model: null,
        small_fast_model: null,
        haiku_model: null,
        sonnet_model: null,
        opus_model: null,
        max_output_tokens: null,
        disable_nonessential_traffic: false,
        enable_tool_search: false,
        experimental_agent_teams: false,
        effort: "default",
        disable_auto_updater: false,
        hide_attribution: false,
      },
      summary: {
        enabled: true,
        show_speed: true,
        show_input: true,
        show_output: true,
        show_cache: true,
        show_latency: false,
        speed_decimal_places: 1,
        clients: {
          app: { enabled: false, style: "formula_compact" },
          cli: { enabled: true, style: "plain_compact" },
          unknown: { enabled: false, style: "inline_chips" },
        },
      },
      routing: {
        enabled: true,
        default_model: "",
        background_model: "",
        think_model: "",
        long_context_model: "",
        long_context_threshold_tokens: 60000,
        web_search_model: "",
        image_model: "",
        route_haiku_to_background: true,
        enable_subagent_model_tag: true,
      },
      fallback: {
        enabled: true,
        default: [],
        background: [],
        think: [],
        long_context: [],
        web_search: [],
        image: [],
      },
      request: {
        api_timeout_ms: 600000,
        max_tokens_cap: null,
        default_max_tokens: null,
        disable_web_search: false,
        thinking_policy: "preserve",
        thinking_budget_tokens: null,
      },
      status_line: {
        enabled: false,
        style: "compact",
        show_provider: true,
        show_model: true,
        show_usage: true,
      },
    },
  };
}

export function normalizeVibeConfig(config: VibeConfig): VibeConfig {
  const fallback = defaultVibeConfig();
  return {
    ...fallback,
    ...config,
    server: { ...fallback.server, ...config.server },
    failover: { ...fallback.failover, ...config.failover },
    log: { ...fallback.log, ...config.log },
    codex: {
      summary: {
        ...fallback.codex!.summary,
        ...config.codex?.summary,
        clients: {
          app: { ...fallback.codex!.summary.clients.app, ...config.codex?.summary?.clients?.app },
          cli: { ...fallback.codex!.summary.clients.cli, ...config.codex?.summary?.clients?.cli },
          unknown: {
            ...fallback.codex!.summary.clients.unknown,
            ...config.codex?.summary?.clients?.unknown,
          },
        },
      },
    },
    claude: {
      native: {
        ...fallback.claude!.native,
        ...config.claude?.native,
      },
      summary: {
        ...fallback.claude!.summary,
        ...config.claude?.summary,
        clients: {
          app: { ...fallback.claude!.summary.clients.app, ...config.claude?.summary?.clients?.app },
          cli: { ...fallback.claude!.summary.clients.cli, ...config.claude?.summary?.clients?.cli },
          unknown: {
            ...fallback.claude!.summary.clients.unknown,
            ...config.claude?.summary?.clients?.unknown,
          },
        },
      },
      routing: {
        ...fallback.claude!.routing,
        ...config.claude?.routing,
      },
      fallback: {
        ...fallback.claude!.fallback,
        ...config.claude?.fallback,
        default: [...(config.claude?.fallback?.default ?? fallback.claude!.fallback.default)],
        background: [
          ...(config.claude?.fallback?.background ?? fallback.claude!.fallback.background),
        ],
        think: [...(config.claude?.fallback?.think ?? fallback.claude!.fallback.think)],
        long_context: [
          ...(config.claude?.fallback?.long_context ?? fallback.claude!.fallback.long_context),
        ],
        web_search: [
          ...(config.claude?.fallback?.web_search ?? fallback.claude!.fallback.web_search),
        ],
        image: [...(config.claude?.fallback?.image ?? fallback.claude!.fallback.image)],
      },
      request: {
        ...fallback.claude!.request,
        ...config.claude?.request,
      },
      status_line: {
        ...fallback.claude!.status_line,
        ...config.claude?.status_line,
      },
    },
  };
}

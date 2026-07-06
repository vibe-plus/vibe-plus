import { describe, expect, test } from "vite-plus/test";
import { existsSync, readFileSync } from "node:fs";
import type { Credential, Provider } from "../../../api/client.ts";
import {
  buildProviderClipboardBundle,
  parseProviderClipboardBundle,
  planProviderClipboardImport,
  serializeProviderClipboardBundle,
} from "./provider-clipboard-bundle.ts";

function sampleProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "p1",
    name: "Test",
    group_name: null,
    avatar_url: null,
    upstreams: [],
    upstream_summary: null,
    kind: "openai-responses",
    base_url: "https://api.example.com/v1",
    protocols: [],
    host: "api.example.com",
    auth_ref: null,
    enabled: true,
    priority: 100,
    supports_websocket: null,
    passthrough_mode: false,
    remote_models: [],
    remote_models_fetched_at: null,
    last_speedtest: null,
    model_aliases: [],
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

function sampleCredential(overrides: Partial<Credential> = {}): Credential {
  return {
    id: "c1",
    provider_id: "p1",
    label: "main",
    auth_ref: "literal:sk-test",
    plan_type: null,
    notes: null,
    enabled: true,
    priority: 100,
    oauth_access_token: null,
    oauth_has_refresh: false,
    oauth_expires_at: null,
    rl_requests_limit: null,
    rl_requests_remaining: null,
    rl_requests_reset_at: null,
    rl_tokens_limit: null,
    rl_tokens_remaining: null,
    rl_tokens_reset_at: null,
    last_used_at: null,
    last_error: null,
    consecutive_failures: 0,
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

describe("provider-clipboard-bundle", () => {
  test("round-trips export and parse", () => {
    const bundle = buildProviderClipboardBundle([sampleProvider()], { p1: [sampleCredential()] });
    const text = serializeProviderClipboardBundle(bundle);
    const parsed = parseProviderClipboardBundle(text);
    expect(parsed.providers.length).toBe(1);
    expect(parsed.providers[0].credentials[0].auth_ref).toBe("literal:sk-test");
  });

  test("plans merge when provider endpoint already exists", () => {
    const bundle = buildProviderClipboardBundle([sampleProvider({ id: "remote" })], {
      remote: [sampleCredential({ label: "second", auth_ref: "literal:sk-other" })],
    });
    const plan = planProviderClipboardImport(bundle, [sampleProvider()], {
      p1: [sampleCredential()],
    });
    expect(plan.totals.providersToCreate).toBe(0);
    expect(plan.totals.providersToMerge).toBe(1);
    expect(plan.totals.credentialsToCreate).toBe(1);
  });

  test("skips duplicate credentials on merge", () => {
    const bundle = buildProviderClipboardBundle([sampleProvider()], {
      p1: [sampleCredential()],
    });
    const plan = planProviderClipboardImport(bundle, [sampleProvider()], {
      p1: [sampleCredential()],
    });
    expect(plan.totals.credentialsToCreate).toBe(0);
    expect(plan.totals.credentialsToSkip).toBe(1);
    expect(plan.totals.inSync).toBe(true);
  });

  test("skips oauth credentials when access token string differs but jwt sub matches", () => {
    function fakeJwt(sub: string, tail: string): string {
      const payload = btoa(JSON.stringify({ sub }))
        .replace(/\+/g, "-")
        .replace(/\//g, "_")
        .replace(/=+$/, "");
      return `hdr.${payload}.${tail}`;
    }
    const exportedToken = fakeJwt("acct-1", "old");
    const liveToken = fakeJwt("acct-1", "new");
    const bundle = buildProviderClipboardBundle([sampleProvider()], {
      p1: [
        sampleCredential({
          auth_ref: null,
          oauth_access_token: exportedToken,
          auth_fingerprint: "fp:deadbeefcafebabe",
        }),
      ],
    });
    const plan = planProviderClipboardImport(bundle, [sampleProvider()], {
      p1: [
        sampleCredential({
          auth_ref: null,
          oauth_access_token: liveToken,
          auth_fingerprint: "fp:deadbeefcafebabe",
        }),
      ],
    });
    expect(plan.totals.credentialsToCreate).toBe(0);
    expect(plan.totals.inSync).toBe(true);
  });

  test("rejects invalid bundle", () => {
    let message = "";
    try {
      parseProviderClipboardBundle("{}");
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    }
    expect(message).toBe("invalid_bundle");
  });

  test("matches provider by name when anthropic protocol keys collide", () => {
    const passthrough = sampleProvider({
      id: "p-pass",
      name: "anthropic-passthrough",
      kind: "anthropic",
      base_url: "https://api.anthropic.com",
      auth_ref: "passthrough",
    });
    const official = sampleProvider({
      id: "p-off",
      name: "官方",
      kind: "anthropic",
      base_url: "https://api.anthropic.com",
    });
    const cred = sampleCredential({
      label: "1",
      auth_ref: "literal:sk-ant-official",
      auth_fingerprint: "fp:official001",
    });
    const bundle = buildProviderClipboardBundle([official], { "p-off": [cred] });
    const plan = planProviderClipboardImport(bundle, [passthrough, official], {
      "p-pass": [],
      "p-off": [cred],
    });
    expect(plan.totals.credentialsToCreate).toBe(0);
    expect(plan.totals.inSync).toBe(true);
  });

  test("round-trips live gateway overview when available", () => {
    const overviewPath = "/tmp/vp-overview.json";
    if (!existsSync(overviewPath)) return;
    const overview = JSON.parse(readFileSync(overviewPath, "utf8")) as {
      providers: Provider[];
      credentials: Record<string, Credential[]>;
    };
    const bundle = buildProviderClipboardBundle(overview.providers, overview.credentials);
    const plan = planProviderClipboardImport(bundle, overview.providers, overview.credentials);
    const pending = plan.items.flatMap((item) =>
      item.credentials
        .filter((row) => row.action === "create")
        .map((row) => `${item.entry.provider.name} / ${row.credential.label}`),
    );
    expect(pending.length).toBe(0);
    expect(plan.totals.inSync).toBe(true);
  });
});

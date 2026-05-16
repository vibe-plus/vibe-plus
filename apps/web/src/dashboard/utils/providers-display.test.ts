import { describe, expect, test } from "vite-plus/test";
import {
  authRefPreview,
  credentialAuthShort,
  credentialPlanTierHint,
  credentialPrimaryAccountLabel,
  displayProviderName,
  fingerprintDisplay,
  isDupFingerprint,
  lastErrorSummary,
  mapUpstreamUserMessage,
  mergedPoolStatus,
  planPctClass,
  primaryPlanPercent,
  rlClass,
  rlPercent,
  shouldHideDbPlanTypeChip,
} from "./providers-display.ts";

const credential = (overrides: Record<string, unknown> = {}) =>
  ({
    id: "cred-1",
    label: "Default label",
    enabled: true,
    auth_ref: "env:KEY",
    oauth_access_token: null,
    oauth_has_refresh: false,
    oauth_account_email: null,
    oauth_account_subject: null,
    oauth_chatgpt_plan_slug: null,
    plan_type: null,
    last_error: null,
    auth_fingerprint: null,
    ...overrides,
  }) as any;

const poolRow = (overrides: Record<string, unknown> = {}) =>
  ({
    auth_mode: "apikey",
    circuit_open: false,
    circuit_state: "closed",
    is_rate_limited: false,
    last_error: null,
    ...overrides,
  }) as any;

describe("provider and credential display helpers", () => {
  test("normalizes legacy provider names and fingerprints", () => {
    expect(displayProviderName("Codex (OpenAI) Primary")).toBe("Codex");
    expect(displayProviderName({ name: "Claude (Anthropic)" })).toBe("Claude");
    expect(displayProviderName(42)).toBe("42");
    expect(displayProviderName(null)).toBe("provider");

    expect(fingerprintDisplay(null)).toBe("—");
    expect(fingerprintDisplay("short-fp")).toBe("short-fp");
    expect(fingerprintDisplay("fp:abcdefghijklmnopqrstuvwxyz")).toBe("fp:abcdefghijklm…");
  });

  test("redacts literal auth refs while preserving useful non-literal refs", () => {
    expect(authRefPreview(credential({ auth_ref: null }))).toBe("—");
    expect(authRefPreview(credential({ auth_ref: "literal:abcdef" }))).toBe("literal:****");
    expect(authRefPreview(credential({ auth_ref: "literal:abcdefghijklmnopqrstuvwxyz" }))).toBe(
      "literal:abc…yz",
    );
    expect(authRefPreview(credential({ auth_ref: "env:OPENAI_API_KEY" }))).toBe(
      "env:OPENAI_API_KEY",
    );
  });

  test("derives auth/account/plan labels from credential and pool state", () => {
    expect(credentialAuthShort(credential({ oauth_has_refresh: true }), undefined)).toBe("OAuth");
    expect(credentialAuthShort(credential(), poolRow({ auth_mode: "oauth" }))).toBe("OAuth");
    expect(credentialAuthShort(credential(), poolRow({ auth_mode: "apikey" }))).toBe("API Key");
    expect(credentialAuthShort(credential({ auth_ref: null }), undefined)).toBe("unconfigured");

    expect(
      credentialPrimaryAccountLabel(credential({ oauth_account_email: "me@example.com" })),
    ).toBe("me@example.com");
    expect(credentialPrimaryAccountLabel(credential({ oauth_account_subject: "sub-1" }))).toBe(
      "sub-1",
    );
    expect(credentialPlanTierHint(credential({ oauth_chatgpt_plan_slug: " PLUS " }))).toBe("Plus");
    expect(credentialPlanTierHint(credential({ plan_type: "team" }))).toBe("team");
    expect(
      shouldHideDbPlanTypeChip(credential({ plan_type: "codex-pro", oauth_has_refresh: true })),
    ).toBe(true);
  });

  test("summarizes pool health in display order", () => {
    expect(mergedPoolStatus(credential({ enabled: false }), undefined)).toEqual({
      ok: false,
      text: "disabled",
      tone: "warn",
    });
    expect(mergedPoolStatus(credential(), poolRow({ circuit_open: true }))).toMatchObject({
      ok: false,
      text: "circuit:open",
      tone: "bad",
    });
    expect(mergedPoolStatus(credential(), poolRow({ circuit_state: "half-open" }))).toMatchObject({
      text: "circuit:half-open",
      tone: "warn",
    });
    expect(mergedPoolStatus(credential(), poolRow({ is_rate_limited: true }))).toMatchObject({
      text: "rate_limited",
      tone: "bad",
    });
    expect(mergedPoolStatus(credential(), poolRow())).toMatchObject({ text: "ok", tone: "ok" });
  });

  test("computes plan and rate-limit percentages with clamping and css classes", () => {
    const planSnapshot = (overrides: Record<string, unknown> = {}) =>
      ({
        id: "plan-1",
        credential_id: "cred-1",
        captured_at: 1,
        codex_5h_used_percent: null,
        codex_7d_used_percent: null,
        codex_5h_reset_after_seconds: null,
        codex_7d_reset_after_seconds: null,
        codex_primary_used_percent: null,
        codex_secondary_used_percent: null,
        summary: null,
        source: "test",
        ...overrides,
      }) as any;

    expect(primaryPlanPercent(undefined)).toEqual({ pct: null, windowLabel: null });
    expect(primaryPlanPercent(planSnapshot({ codex_primary_used_percent: 120 }))).toEqual({
      pct: 100,
      windowLabel: "W",
    });
    expect(primaryPlanPercent(planSnapshot({ codex_5h_used_percent: -5 }))).toEqual({
      pct: 0,
      windowLabel: "5h",
    });
    expect(primaryPlanPercent(planSnapshot({ codex_7d_used_percent: 42 }))).toEqual({
      pct: 42,
      windowLabel: "7d",
    });

    expect(planPctClass(null)).toBe("bg-gray-600");
    expect(planPctClass(59)).toBe("bg-emerald-500");
    expect(planPctClass(84)).toBe("bg-yellow-500");
    expect(planPctClass(85)).toBe("bg-red-500");

    expect(rlPercent(null, 100)).toBe(100);
    expect(rlPercent(25, 100)).toBe(25);
    expect(rlClass(51)).toBe("bg-emerald-500");
    expect(rlClass(21)).toBe("bg-yellow-500");
    expect(rlClass(20)).toBe("bg-red-500");
  });

  test("maps backend errors and duplicate fingerprints to concise UI labels", () => {
    expect(mapUpstreamUserMessage("No such credential abc")).toBe("credential:not_found");
    expect(mapUpstreamUserMessage("provider credential pool is empty")).toBe("pool:empty");
    expect(mapUpstreamUserMessage("same fingerprint duplicate conflict")).toBe(
      "fingerprint:duplicate",
    );
    expect(mapUpstreamUserMessage("custom upstream error")).toBe("custom upstream error");

    expect(
      lastErrorSummary(
        credential({ last_error: "No such credential" }),
        poolRow({ last_error: "pool empty" }),
      ),
    ).toBe("pool:empty · credential:not_found");

    expect(
      isDupFingerprint(credential({ auth_fingerprint: "fp:1" }), [
        credential({ id: "a", auth_fingerprint: "fp:1" }),
        credential({ id: "b", auth_fingerprint: "fp:1" }),
      ] as any),
    ).toBe(true);
  });
});

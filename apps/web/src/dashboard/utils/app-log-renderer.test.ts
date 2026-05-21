import { describe, expect, test } from "vite-plus/test";
import { renderAppLogEvent } from "./app-log-renderer.ts";

const t = ((key: string, vars?: Record<string, unknown>) => {
  if (key === "events.provider") return "provider";
  if (key === "events.credential") return "credential";
  if (key === "events.providerUpdated") return "updated";
  return vars ? `${key}:${JSON.stringify(vars)}` : key;
}) as any;

describe("app log renderer", () => {
  test("uses live provider names when available", () => {
    const providerById = new Map([["uuid-1", { id: "uuid-1", name: "New Name" } as any]]);
    const rendered = renderAppLogEvent(
      {
        ts: 1,
        level: "info",
        event_type: "provider.updated",
        message: "",
        detail: null,
        payload: { provider: { id: "uuid-1", name: "Old Name" } } as any,
        category: "provider",
      },
      t,
      providerById,
    );
    const text = rendered.title
      .map((part) => (part.type === "text" ? part.text : part.text))
      .join("");
    expect(text).toContain("New Name");
    expect(text).not.toContain("Old Name");
  });

  test("keeps structured circuit payloads from being rewritten by legacy text", () => {
    const rendered = renderAppLogEvent(
      {
        ts: 2,
        level: "warn",
        event_type: "circuit.opened",
        message: "Circuit opened: API Key",
        detail: null,
        payload: {
          schema: 1,
          circuit: {
            consecutive_failures: 3,
            open_timeout_secs: 300,
          },
          subject: {
            kind: "credential",
            id: "cred-1",
            label: "API Key",
          },
          provider: {
            id: "prov-1",
            name: "OpenAI",
          },
          credential: {
            id: "cred-1",
            label: "API Key",
            provider_id: "prov-1",
          },
        } as any,
        category: "circuit",
      },
      t,
    );
    const text = rendered.title.map((part) => part.text).join("");
    expect(text).toContain("credential");
    expect(text).toContain("API Key");
    expect(text).not.toContain("provider “API Key”");
  });
});

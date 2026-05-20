import { describe, expect, test } from "vite-plus/test";
import { entityToken, resolveEntityRoute } from "./entity-links.ts";

describe("entity links", () => {
  test("only links UUID-backed entities", () => {
    const uuid = "550e8400-e29b-41d4-a716-446655440000";
    expect(resolveEntityRoute({ kind: "provider", id: uuid })).toEqual({
      path: "/ui/providers",
      query: { provider: uuid },
    });
    expect(resolveEntityRoute({ kind: "provider", id: "ai98pro" })).toBe(null);
  });

  test("renders non-UUID entities as plain text", () => {
    expect(entityToken({ kind: "provider", id: "ai98pro", label: "ai98pro" })).toEqual({
      type: "text",
      text: "ai98pro",
    });
  });
});

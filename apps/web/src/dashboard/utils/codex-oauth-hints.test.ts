import { describe, expect, test } from "vite-plus/test";
import { chatgptHintsFromJwt, hintsFromAuthJsonTokens } from "./codex-oauth-hints.ts";

function jwt(payload: unknown): string {
  const encoded = Buffer.from(JSON.stringify(payload), "utf8").toString("base64url");
  return `header.${encoded}.signature`;
}

describe("chatgpt oauth hint extraction", () => {
  test("extracts email, subject, and simple plan slug from ChatGPT JWT claims", () => {
    const token = jwt({
      email: " user@example.com ",
      sub: " user-sub ",
      "https://api.openai.com/auth": {
        chatgpt_plan_type: " Plus ",
      },
    });

    expect(chatgptHintsFromJwt(token)).toEqual({
      oauth_cached_email: "user@example.com",
      oauth_cached_subject: "user-sub",
      oauth_cached_plan_slug: "plus",
    });
  });

  test("falls back to profile email, auth user id, and object plan values", () => {
    const token = jwt({
      "https://api.openai.com/profile": { email: "profile@example.com" },
      "https://api.openai.com/auth": {
        user_id: "auth-user-id",
        chatgpt_plan_type: { primary: " Pro " },
      },
    });

    expect(chatgptHintsFromJwt(token)).toEqual({
      oauth_cached_email: "profile@example.com",
      oauth_cached_subject: "auth-user-id",
      oauth_cached_plan_slug: "pro",
    });
  });

  test("returns empty hints for missing, malformed, or non-object payloads", () => {
    const empty = {
      oauth_cached_email: null,
      oauth_cached_subject: null,
      oauth_cached_plan_slug: null,
    };

    expect(chatgptHintsFromJwt(null)).toEqual(empty);
    expect(chatgptHintsFromJwt("not-a-jwt")).toEqual(empty);
    expect(chatgptHintsFromJwt("a.broken-json.c")).toEqual(empty);
    expect(hintsFromAuthJsonTokens({ access_token: "x" })).toEqual(empty);
  });

  test("reads id_token from auth.json token object", () => {
    const token = jwt({ email: "token@example.com" });
    expect(hintsFromAuthJsonTokens({ id_token: token }).oauth_cached_email).toBe(
      "token@example.com",
    );
  });
});

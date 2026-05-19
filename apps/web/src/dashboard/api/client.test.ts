import { describe, expect, test } from "vite-plus/test";
import { ApiError, apiUrl } from "./client.ts";

describe("dashboard API URL helpers", () => {
  test("builds HTTP API URLs from the selected gateway base", () => {
    expect(apiUrl("/status", "http://127.0.0.1:15917")).toBe("http://127.0.0.1:15917/status");
  });

  test("ApiError keeps the HTTP status, body, and URL for UI localization", () => {
    const err = new ApiError(
      503,
      "Service Unavailable",
      "http://127.0.0.1:15917/codex/v1/responses",
    );

    expect(err.message).toBe("503 Service Unavailable");
    expect(err.status).toBe(503);
    expect(err.bodyText).toBe("Service Unavailable");
    expect(err.url).toContain("/codex/v1/responses");
  });
});

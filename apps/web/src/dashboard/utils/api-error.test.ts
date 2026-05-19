import { describe, expect, test } from "vite-plus/test";
import { ApiError } from "../api/client.ts";
import { formatApiError } from "./api-error.ts";

function t(key: string, params?: Record<string, unknown>): string {
  if (key === "errors.circuitBreakerBlocked") return "供应商已被熔断器暂时阻断";
  if (key === "errors.requestFailed") return `请求失败 ${params?.status}`;
  return key;
}

describe("formatApiError", () => {
  test("localizes all-providers circuit breaker 503 responses", () => {
    const err = new ApiError(
      503,
      "all providers blocked by circuit breaker (3 skipped). reset via POST /_vp/providers/:id/circuit/reset",
      "http://127.0.0.1:15917/codex/v1/responses",
    );

    expect(formatApiError(err, t as never)).toBe("供应商已被熔断器暂时阻断");
  });

  test("preserves readable JSON details for other API errors", () => {
    const err = new ApiError(400, JSON.stringify({ detail: "bad provider config" }), "/_vp/x");

    expect(formatApiError(err, t as never)).toBe("bad provider config");
  });
});

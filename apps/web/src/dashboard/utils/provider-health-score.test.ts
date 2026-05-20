import { describe, expect, test } from "vite-plus/test";
import { providerSuccessScore } from "./provider-health-score.ts";

describe("providerSuccessScore", () => {
  test("excludes ordinary 4xx from provider health by default", () => {
    expect(
      providerSuccessScore({
        requests: 10,
        successes: 6,
        failures: 4,
        success_rate: 0.6,
        err_4xx_other: 3,
        err_429: 1,
      }),
    ).toBe(1);
  });

  test("excludes 429 and 503 but keeps other 5xx in the denominator", () => {
    expect(
      providerSuccessScore({
        requests: 10,
        successes: 6,
        failures: 4,
        success_rate: 0.6,
        err_429: 1,
        err_503: 1,
        err_5xx_other: 2,
      }),
    ).toBe(6 / 8);
  });

  test("returns null when all traffic is neutral client errors or rate limits", () => {
    expect(
      providerSuccessScore({
        requests: 4,
        successes: 0,
        failures: 4,
        success_rate: 0,
        err_4xx_other: 3,
        err_429: 1,
      }),
    ).toBe(null);
  });
});

test("excludes unknown failures by default", () => {
  expect(
    providerSuccessScore({
      requests: 5,
      successes: 3,
      failures: 2,
      success_rate: 0.6,
    }),
  ).toBe(1);
});

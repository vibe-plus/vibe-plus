import { describe, expect, it } from "vitest";
import {
  DEFAULT_DASHBOARD_PERFORMANCE_BUDGET,
  evaluateDashboardPerformanceBudget,
} from "./performance-budget.ts";

describe("dashboard performance budget", () => {
  it("accepts overview requests within the web performance budget", () => {
    expect(
      evaluateDashboardPerformanceBudget({
        overviewLoadMs: DEFAULT_DASHBOARD_PERFORMANCE_BUDGET.overviewLoadMs,
        providersOverviewMs: 120,
        statsMs: 80,
        logsMs: 20,
      }),
    ).toEqual([]);
  });

  it("fails loudly when overview dependencies regress", () => {
    expect(
      evaluateDashboardPerformanceBudget({
        overviewLoadMs: 1_900,
        providersOverviewMs: 900,
        statsMs: 480,
        logsMs: 260,
      }),
    ).toEqual([
      "overviewLoadMs 1900.0ms > 1200.0ms",
      "providersOverviewMs 900.0ms > 800.0ms",
      "statsMs 480.0ms > 450.0ms",
      "logsMs 260.0ms > 250.0ms",
    ]);
  });
});

export interface DashboardPerformanceSample {
  overviewLoadMs: number;
  providersOverviewMs: number;
  statsMs: number;
  logsMs: number;
}

export interface DashboardPerformanceBudget {
  overviewLoadMs: number;
  providersOverviewMs: number;
  statsMs: number;
  logsMs: number;
}

export const DEFAULT_DASHBOARD_PERFORMANCE_BUDGET: DashboardPerformanceBudget = {
  overviewLoadMs: 1_200,
  providersOverviewMs: 800,
  statsMs: 450,
  logsMs: 250,
};

export function evaluateDashboardPerformanceBudget(
  sample: DashboardPerformanceSample,
  budget: DashboardPerformanceBudget = DEFAULT_DASHBOARD_PERFORMANCE_BUDGET,
): string[] {
  const failures: string[] = [];
  for (const key of Object.keys(budget) as Array<keyof DashboardPerformanceBudget>) {
    if (sample[key] > budget[key]) {
      failures.push(`${key} ${sample[key].toFixed(1)}ms > ${budget[key].toFixed(1)}ms`);
    }
  }
  return failures;
}

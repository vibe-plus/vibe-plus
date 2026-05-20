import type { ProviderStat } from "../api/client.ts";

export interface ProviderSuccessScoreConfig {
  /** Treat ordinary client-side 4xx (400/401/403/404...) as neutral instead of provider failures. */
  includeClient4xxInDenominator: boolean;
  /** Treat 429 as neutral for success rate; rate limits are surfaced separately as capacity signals. */
  includeRateLimit429InDenominator: boolean;
  /** Treat 503 as neutral by default: many proxy panels use it for no-channel/model/config states. */
  includeUnavailable503InDenominator: boolean;
  /** Keep other 5xx as provider-affecting failures. */
  includeOther5xxInDenominator: boolean;
  /** Treat unclassified failures as neutral until backend can prove they are server-capacity faults. */
  includeUnknownFailuresInDenominator: boolean;
}

export const DEFAULT_PROVIDER_SUCCESS_SCORE_CONFIG: ProviderSuccessScoreConfig = {
  includeClient4xxInDenominator: false,
  includeRateLimit429InDenominator: false,
  includeUnavailable503InDenominator: false,
  includeOther5xxInDenominator: true,
  includeUnknownFailuresInDenominator: false,
};

export type ProviderSuccessScoreInput = Pick<
  ProviderStat,
  | "requests"
  | "successes"
  | "failures"
  | "success_rate"
  | "err_429"
  | "err_503"
  | "err_4xx_other"
  | "err_5xx_other"
>;

function count(value: number | null | undefined): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
}

/**
 * Algorithmic provider success score.
 *
 * This is intentionally not raw `successes / requests`: the denominator is policy-driven so
 * product can decide which error classes should affect provider health. By default ordinary 4xx
 * errors, 429 rate limits, 503 no-channel/unavailable states, and unclassified failures are
 * neutral for the success-rate number. Only clearer server-pressure failures such as 500/502/504
 * reduce the score by default. Rate limits and config/auth/quota issues are surfaced separately.
 */
export function providerSuccessScore(
  input: ProviderSuccessScoreInput,
  config: ProviderSuccessScoreConfig = DEFAULT_PROVIDER_SUCCESS_SCORE_CONFIG,
): number | null {
  const successes = count(input.successes);
  const failures = count(input.failures);
  const err429 = count(input.err_429);
  const err503 = count(input.err_503);
  const err4xxOther = count(input.err_4xx_other);
  const err5xxOther = count(input.err_5xx_other);
  const bucketedFailures = err429 + err503 + err4xxOther + err5xxOther;
  const unknownFailures = Math.max(0, failures - bucketedFailures);
  const scoredFailures =
    (config.includeClient4xxInDenominator ? err4xxOther : 0) +
    (config.includeRateLimit429InDenominator ? err429 : 0) +
    (config.includeUnavailable503InDenominator ? err503 : 0) +
    (config.includeOther5xxInDenominator ? err5xxOther : 0) +
    (config.includeUnknownFailuresInDenominator ? unknownFailures : 0);
  const denominator = successes + scoredFailures;
  if (denominator <= 0) return null;
  return successes / denominator;
}

export function providerSuccessScoreOrRaw(input: ProviderSuccessScoreInput): number {
  return providerSuccessScore(input) ?? input.success_rate;
}

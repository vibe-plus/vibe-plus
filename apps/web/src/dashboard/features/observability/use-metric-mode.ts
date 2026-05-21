import { ref, watch } from "vue";

export type MetricMode = "usd" | "tokens";

const STORAGE_KEY = "vp.observability.metric";

function read(): MetricMode {
  if (typeof window === "undefined") return "usd";
  const raw = window.localStorage.getItem(STORAGE_KEY);
  return raw === "tokens" ? "tokens" : "usd";
}

const metric = ref<MetricMode>(read());

if (typeof window !== "undefined") {
  watch(
    metric,
    (next) => {
      window.localStorage.setItem(STORAGE_KEY, next);
    },
    { flush: "post" },
  );
}

export function useMetricMode() {
  function toggle() {
    metric.value = metric.value === "usd" ? "tokens" : "usd";
  }
  function set(next: MetricMode) {
    metric.value = next;
  }
  function formatUsd(usd: number): string {
    if (!Number.isFinite(usd) || usd <= 0) return "";
    if (usd < 0.01) return `$${usd.toFixed(4)}`;
    if (usd < 1) return `$${usd.toFixed(3)}`;
    if (usd < 100) return `$${usd.toFixed(2)}`;
    return `$${Math.round(usd)}`;
  }
  function formatTokens(n: number): string {
    if (!Number.isFinite(n) || n <= 0) return "";
    if (n < 1000) return `${n}`;
    if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}K`;
    if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 1 : 0)}M`;
    return `${(n / 1_000_000_000).toFixed(2)}B`;
  }
  /** Pick the right metric to display in a single chip, given USD and tokens. */
  function formatMetric(usd: number, tokens: number): string {
    if (metric.value === "tokens") return formatTokens(tokens);
    return formatUsd(usd);
  }
  return { metric, toggle, set, formatUsd, formatTokens, formatMetric };
}

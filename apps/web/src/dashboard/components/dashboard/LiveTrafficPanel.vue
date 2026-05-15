<script setup lang="ts">
import { computed } from "vue";
import VpIcon from "../vp-icon.vue";
import ProviderLogo from "../provider-logo.vue";
import MetricTicker from "./MetricTicker.vue";
import type { ProviderKind } from "../../api/client.ts";

export type LiveTrafficRequest = {
  id: string;
  providerName: string;
  providerKind?: ProviderKind;
  model: string;
  tokensPerSec: number;
  decodeTokensPerSec: number | null;
  emitTokensPerSec: number | null;
  outputTokens: number;
  upstreamBytes: number;
  clientBytes: number;
  upstreamBytesPerSec: number;
  clientBytesPerSec: number;
  estimatedCostUsd: number;
  estimatedCostUsdPerMin: number;
  firstByteMs: number | null;
  firstWriteMs: number | null;
  updatedAt: number;
};

const props = defineProps<{
  activeCount: number;
  heatLevel: number;
  trafficState: "offline" | "quiet" | "warm" | "hot";
  providerIssueCount: number;
  readinessLabel: string;
  totalTokensPerSec: number;
  totalBytesPerSec: number;
  totalCostUsd: number;
  totalCostUsdPerMin: number;
  outputTokensSoFar: number;
  upstreamBytesSoFar: number;
  clientBytesSoFar: number;
  requests: LiveTrafficRequest[];
}>();

const hottestRequest = computed(() => props.requests[0] ?? null);
const streamHeat = computed(() =>
  Math.min(100, Math.max(props.trafficState === "quiet" ? 6 : 12, Math.round(props.heatLevel))),
);
const hasTraffic = computed(
  () => props.activeCount > 0 || props.totalTokensPerSec > 0 || props.totalBytesPerSec > 0,
);
const showLiveMetrics = computed(() => hasTraffic.value || props.totalCostUsdPerMin > 0);
const isQuiet = computed(() => props.trafficState === "quiet" && !showLiveMetrics.value);
const panelClass = computed(() => `live-traffic--${props.trafficState}`);
const meterDuration = computed(() => {
  if (!hasTraffic.value) return "3.8s";
  if (props.trafficState === "hot") return "0.72s";
  return "1.35s";
});
const statusLabel = computed(() => {
  if (props.trafficState === "offline") return "offline";
  if (props.trafficState === "hot") return "hot";
  if (props.trafficState === "warm") return "warming";
  if (props.providerIssueCount > 0) return "quiet · attention";
  return "quiet · ready";
});
const estimatedTokensPerSec = computed(() =>
  props.totalTokensPerSec > 0 ? props.totalTokensPerSec : props.totalBytesPerSec / 1200,
);
const primaryBurn = computed(() => {
  if (props.totalCostUsdPerMin > 0) {
    return {
      value: formatUsd(props.totalCostUsdPerMin),
      suffix: "/min",
      precision: 0,
      tone: props.trafficState === "hot" ? ("hot" as const) : ("good" as const),
    };
  }
  if (props.totalCostUsd > 0) {
    return {
      value: formatUsd(props.totalCostUsd),
      suffix: "",
      precision: 0,
      tone: "good" as const,
    };
  }
  if (props.trafficState === "quiet") {
    return {
      value: "quiet",
      suffix: "",
      precision: 0,
      tone: "muted" as const,
    };
  }
  return {
    value: 0,
    suffix: "tok/s",
    precision: 1,
    tone: "muted" as const,
  };
});
const burnDetail = computed(() => {
  if (props.totalCostUsdPerMin > 0) {
    return props.totalCostUsd > 0 ? `${formatUsd(props.totalCostUsd)} spent` : "live estimate";
  }
  if (props.totalTokensPerSec > 0) return `${props.totalTokensPerSec.toFixed(1)} tok/s`;
  if (props.totalBytesPerSec > 0) return `${compactBytes(props.totalBytesPerSec)}/s`;
  return props.providerIssueCount > 0 ? "capacity attention" : "standing by";
});
const tokenMini = computed(() => {
  if (props.outputTokensSoFar > 0) {
    return {
      label: "Tokens",
      value: props.outputTokensSoFar,
      suffix: "tok",
      precision: 0,
      tone: "default" as const,
    };
  }
  if (estimatedTokensPerSec.value > 0) {
    return {
      label: "Token pace",
      value: estimatedTokensPerSec.value,
      suffix: "tok/s est",
      precision: 1,
      tone: "good" as const,
    };
  }
  return {
    label: "Tokens",
    value: "waiting",
    suffix: "",
    precision: 0,
    tone: "muted" as const,
  };
});
const hottestRate = computed(() => {
  const request = hottestRequest.value;
  if (!request) return { value: 0, suffix: "tok/s", precision: 1, tone: "muted" as const };
  if (request.tokensPerSec > 0) {
    return {
      value: request.tokensPerSec,
      suffix: "tok/s",
      precision: 1,
      tone: "good" as const,
    };
  }
  const bytesPerSec = Math.max(request.clientBytesPerSec, request.upstreamBytesPerSec);
  if (bytesPerSec > 0) {
    return {
      value: `${compactBytes(bytesPerSec)}/s`,
      suffix: "",
      precision: 0,
      tone: "good" as const,
    };
  }
  return { value: 0, suffix: "tok/s", precision: 1, tone: "muted" as const };
});

const hottestCost = computed(() => {
  const request = hottestRequest.value;
  if (!request || (request.estimatedCostUsd <= 0 && request.estimatedCostUsdPerMin <= 0)) {
    return null;
  }
  return {
    value:
      request.estimatedCostUsd > 0
        ? formatUsd(request.estimatedCostUsd)
        : `${formatUsd(request.estimatedCostUsdPerMin)}/min`,
    detail:
      request.estimatedCostUsd > 0
        ? `${formatUsd(request.estimatedCostUsdPerMin)}/min`
        : "live estimate",
  };
});

function compactBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0B";
  if (bytes < 1024) return `${Math.round(bytes)}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

function formatUsd(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "$0";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 10) return `$${n.toFixed(2)}`;
  return `$${n.toFixed(1)}`;
}

function fmtMs(ms: number | null): string {
  return ms == null ? "-" : `${ms}ms`;
}
</script>

<template>
  <section class="live-traffic card-base overflow-hidden" :class="panelClass">
    <div class="live-traffic__header border-b border-vp-border px-4 py-3">
      <div class="flex min-w-0 items-center gap-2">
        <span class="live-traffic__bolt grid size-8 shrink-0 place-items-center rounded-lg">
          <VpIcon name="zap" size-class="size-4" />
        </span>
        <span class="min-w-0">
          <span class="block text-sm font-semibold text-vp-text">Live burn</span>
          <span class="block truncate font-mono text-[11px] text-vp-muted">
            {{ statusLabel }} · {{ activeCount }} active
          </span>
        </span>
      </div>
      <span v-if="!isQuiet" class="min-w-0 text-right">
        <MetricTicker
          :value="primaryBurn.value"
          :suffix="primaryBurn.suffix"
          :precision="primaryBurn.precision"
          :tone="primaryBurn.tone"
          size="lg"
        />
        <span class="block truncate font-mono text-[11px] text-vp-muted">{{ burnDetail }}</span>
      </span>
      <span
        v-else
        class="rounded-full border border-sky-200 bg-sky-50 px-2.5 py-1 font-mono text-[11px] font-semibold text-sky-700"
      >
        standby
      </span>
    </div>

    <div class="relative overflow-hidden px-4" :class="isQuiet ? 'py-3' : 'py-4'">
      <div
        v-if="!isQuiet"
        class="live-traffic__meter"
        :class="hasTraffic ? 'live-traffic__meter--active' : 'live-traffic__meter--idle'"
        :style="{ '--live-traffic-scan-speed': meterDuration }"
      >
        <div class="live-traffic__meter-fill" :style="{ width: `${streamHeat}%` }" />
      </div>

      <div v-if="showLiveMetrics" class="mt-4 grid grid-cols-3 gap-2">
        <div class="live-traffic__mini">
          <span class="live-traffic__label">Burn rate</span>
          <span class="live-traffic__number">{{ formatUsd(totalCostUsdPerMin) }}/min</span>
        </div>
        <div class="live-traffic__mini">
          <span class="live-traffic__label">{{ tokenMini.label }}</span>
          <MetricTicker
            :value="tokenMini.value"
            :suffix="tokenMini.suffix"
            :precision="tokenMini.precision"
            :tone="tokenMini.tone"
            size="sm"
          />
        </div>
        <div class="live-traffic__mini">
          <span class="live-traffic__label">Flow</span>
          <span class="live-traffic__number">{{ compactBytes(totalBytesPerSec) }}/s</span>
        </div>
      </div>

      <div
        v-if="hottestRequest && !isQuiet"
        class="mt-4 rounded-lg border border-vp-border bg-vp-surface/78 p-3"
      >
        <div class="flex min-w-0 items-center gap-2">
          <ProviderLogo
            :kind="hottestRequest.providerKind"
            :active-request-count="1"
            :tokens-per-sec="
              hottestRequest.tokensPerSec ||
              Math.max(hottestRequest.clientBytesPerSec, hottestRequest.upstreamBytesPerSec) / 1200
            "
            :activity-label="
              hottestRequest.tokensPerSec > 0
                ? `${hottestRequest.tokensPerSec.toFixed(1)} tok/s`
                : `${compactBytes(Math.max(hottestRequest.clientBytesPerSec, hottestRequest.upstreamBytesPerSec))}/s`
            "
            size-class="size-8"
            icon-size-class="size-4"
          />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-semibold text-vp-text">
              {{ hottestRequest.providerName }}
            </span>
            <span class="block truncate font-mono text-[11px] text-vp-muted">
              {{ hottestRequest.model }}
            </span>
          </span>
          <span class="shrink-0 text-right">
            <MetricTicker
              :value="hottestCost?.value ?? hottestRate.value"
              :suffix="hottestCost ? '' : hottestRate.suffix"
              :precision="hottestRate.precision"
              size="sm"
              :tone="hottestRate.tone"
            />
            <span class="block font-mono text-[10px] text-vp-muted">
              {{
                hottestCost?.detail ??
                `${compactBytes(Math.max(hottestRequest.clientBytesPerSec, hottestRequest.upstreamBytesPerSec))}/s`
              }}
            </span>
          </span>
        </div>
        <div class="mt-3 grid grid-cols-3 gap-2 text-[11px]">
          <div>
            <span class="live-traffic__label">Decode</span>
            <span class="live-traffic__number">
              {{
                hottestRequest.decodeTokensPerSec != null && hottestRequest.decodeTokensPerSec > 0
                  ? hottestRequest.decodeTokensPerSec.toFixed(1)
                  : compactBytes(hottestRequest.upstreamBytesPerSec) + "/s"
              }}
            </span>
          </div>
          <div>
            <span class="live-traffic__label">Emit</span>
            <span class="live-traffic__number">
              {{
                hottestRequest.emitTokensPerSec != null && hottestRequest.emitTokensPerSec > 0
                  ? hottestRequest.emitTokensPerSec.toFixed(1)
                  : compactBytes(hottestRequest.clientBytesPerSec) + "/s"
              }}
            </span>
          </div>
          <div>
            <span class="live-traffic__label">First</span>
            <span class="live-traffic__number">
              {{ fmtMs(hottestRequest.firstWriteMs ?? hottestRequest.firstByteMs) }}
            </span>
          </div>
        </div>
      </div>

      <div v-else class="live-traffic__quiet rounded-lg border px-3 py-3 text-center text-sm">
        <span class="block font-semibold">{{ statusLabel }}</span>
        <span class="mt-1 block text-xs">{{ readinessLabel }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.live-traffic {
  background:
    linear-gradient(135deg, color-mix(in srgb, #38bdf8 6%, transparent), transparent 38%),
    var(--vp-surface);
  transition:
    background 360ms ease,
    box-shadow 360ms ease;
}

.live-traffic--warm {
  background:
    linear-gradient(135deg, color-mix(in srgb, #f59e0b 12%, transparent), transparent 42%),
    linear-gradient(180deg, color-mix(in srgb, #22c55e 5%, transparent), transparent 58%),
    var(--vp-surface);
}

.live-traffic--hot {
  background:
    linear-gradient(135deg, color-mix(in srgb, #f97316 18%, transparent), transparent 45%),
    linear-gradient(180deg, color-mix(in srgb, #ef4444 7%, transparent), transparent 62%),
    var(--vp-surface);
  box-shadow: 0 0 0 1px color-mix(in srgb, #fb923c 22%, transparent);
}

.live-traffic--offline {
  background:
    linear-gradient(135deg, color-mix(in srgb, #ef4444 9%, transparent), transparent 38%),
    var(--vp-surface);
}

.live-traffic__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
}

.live-traffic__bolt {
  background: color-mix(in srgb, #38bdf8 12%, var(--vp-surface));
  color: #0284c7;
  box-shadow: inset 0 0 0 1px color-mix(in srgb, #38bdf8 22%, transparent);
}

.live-traffic--warm .live-traffic__bolt {
  background: color-mix(in srgb, #f59e0b 15%, var(--vp-surface));
  color: #d97706;
  box-shadow: inset 0 0 0 1px color-mix(in srgb, #f59e0b 28%, transparent);
}

.live-traffic--hot .live-traffic__bolt {
  background: color-mix(in srgb, #f97316 18%, var(--vp-surface));
  color: #ea580c;
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, #f97316 32%, transparent),
    0 0 18px color-mix(in srgb, #fb923c 26%, transparent);
}

.live-traffic__meter {
  position: relative;
  height: 0.625rem;
  overflow: hidden;
  border-radius: 0.5rem;
  background: color-mix(in srgb, var(--vp-text) 8%, var(--vp-surface));
}

.live-traffic__meter::after {
  content: "";
  position: absolute;
  inset: 0;
  background-image: linear-gradient(
    90deg,
    transparent 0%,
    color-mix(in srgb, white 42%, transparent) 48%,
    transparent 100%
  );
  transform: translateX(-100%);
}

.live-traffic__meter--active::after {
  animation: live-traffic-scan var(--live-traffic-scan-speed, 1.15s) linear infinite;
}

.live-traffic__meter--idle::after {
  animation: live-traffic-scan var(--live-traffic-scan-speed, 3.8s) ease-in-out infinite;
  opacity: 0.28;
}

.live-traffic__meter-fill {
  height: 100%;
  min-width: 0.625rem;
  border-radius: inherit;
  background: linear-gradient(90deg, #38bdf8, #22c55e);
  box-shadow: 0 0 14px color-mix(in srgb, #38bdf8 20%, transparent);
  transition:
    width 240ms ease,
    background 360ms ease,
    box-shadow 360ms ease;
}

.live-traffic--warm .live-traffic__meter-fill {
  background: linear-gradient(90deg, #22c55e, #f59e0b, #fb923c);
  box-shadow: 0 0 18px color-mix(in srgb, #f59e0b 28%, transparent);
}

.live-traffic--hot .live-traffic__meter-fill {
  background: linear-gradient(90deg, #f59e0b, #fb923c, #ef4444);
  box-shadow: 0 0 24px color-mix(in srgb, #fb923c 36%, transparent);
}

.live-traffic__mini {
  min-width: 0;
  border-radius: 0.5rem;
  border: 1px solid var(--vp-border);
  background: color-mix(in srgb, var(--vp-surface) 80%, transparent);
  padding: 0.5rem;
}

.live-traffic__label {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.625rem;
  font-weight: 600;
  line-height: 1rem;
  color: var(--vp-muted);
  text-transform: uppercase;
}

.live-traffic__number {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 0.8125rem;
  font-weight: 650;
  color: var(--vp-text);
}

.live-traffic__quiet {
  border-color: color-mix(in srgb, #38bdf8 24%, var(--vp-border));
  color: var(--vp-muted);
  background: color-mix(in srgb, #38bdf8 5%, var(--vp-surface));
}

.live-traffic--quiet .live-traffic__quiet {
  color: color-mix(in srgb, var(--vp-text) 74%, var(--vp-muted));
}

.live-traffic--offline .live-traffic__quiet {
  border-color: color-mix(in srgb, #ef4444 30%, var(--vp-border));
  background: color-mix(in srgb, #ef4444 6%, var(--vp-surface));
}

@keyframes live-traffic-scan {
  to {
    transform: translateX(100%);
  }
}
</style>

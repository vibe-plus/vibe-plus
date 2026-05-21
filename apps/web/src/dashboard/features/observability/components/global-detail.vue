<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { Provider, RealtimeSnapshot } from "../../../api/client.ts";
import Card from "../../../components/ui/card.vue";
import Badge from "../../../components/ui/badge.vue";
import LogsPanel from "../../../components/logs-panel.vue";
import VpIcon from "../../../components/vp-icon.vue";
import Sparkline from "./sparkline.vue";

export type GlobalView = "logs" | "waveform" | "all";

const props = defineProps<{
  view: GlobalView;
  realtime: RealtimeSnapshot | null;
  realtimeTransport: string;
  history: RealtimeSnapshot[];
  providers: Provider[];
  pollMs: number;
}>();

const { t } = useI18n({ useScope: "global" });

const HISTORY_SIZE = 60;

function seriesOf(extract: (s: RealtimeSnapshot) => number): number[] {
  return props.history.map(extract);
}

const sparkActive = computed(() => seriesOf((s) => s.active_requests.length));
const sparkTok = computed(() =>
  seriesOf((s) =>
    s.active_requests.reduce(
      (sum, request) => sum + (request.active_output_tokens_per_sec ?? 0),
      0,
    ),
  ),
);
const sparkBytes = computed(() =>
  seriesOf((s) =>
    s.active_requests.reduce(
      (sum, request) =>
        sum + request.active_upstream_bytes_per_sec + request.active_downstream_bytes_per_sec,
      0,
    ),
  ),
);
const sparkUsd = computed(() =>
  seriesOf((s) =>
    s.active_requests.reduce((sum, request) => sum + (request.active_cost_usd_per_hour ?? 0), 0),
  ),
);

const kpiActive = computed(() => props.realtime?.active_requests.length ?? 0);
const kpiTokTps = computed(
  () =>
    props.realtime?.active_requests.reduce(
      (sum, r) => sum + (r.active_output_tokens_per_sec ?? 0),
      0,
    ) ?? 0,
);
const kpiBytesPerSec = computed(
  () =>
    props.realtime?.active_requests.reduce(
      (sum, r) => sum + r.active_upstream_bytes_per_sec + r.active_downstream_bytes_per_sec,
      0,
    ) ?? 0,
);
const kpiUsdPerHour = computed(() => {
  const v =
    props.realtime?.active_requests.reduce(
      (sum, r) => sum + (r.active_cost_usd_per_hour ?? 0),
      0,
    ) ?? 0;
  return v > 0 ? v : null;
});

function formatBytesPerSec(b: number): string {
  if (!Number.isFinite(b) || b <= 0) return "—";
  if (b >= 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB/s`;
  if (b >= 1024) return `${(b / 1024).toFixed(1)} KB/s`;
  return `${b.toFixed(0)} B/s`;
}

function formatUsdPerHour(v: number | null): string {
  if (v == null || !Number.isFinite(v) || v <= 0) return "—";
  if (v < 0.01) return `$${v.toFixed(4)}/h`;
  if (v < 1) return `$${v.toFixed(3)}/h`;
  return `$${v.toFixed(2)}/h`;
}

function headingFor(view: GlobalView): string {
  if (view === "logs") return t("obs.global.logsHeading");
  if (view === "waveform") return t("obs.global.waveformHeading");
  return t("obs.global.allHeading");
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="border-b border-vp-border bg-vp-surface px-4 py-3">
      <div class="flex items-center gap-2">
        <span
          class="size-2 rounded-full"
          :class="
            props.realtimeTransport === 'stream'
              ? 'bg-emerald-500'
              : props.realtimeTransport === 'polling'
                ? 'bg-amber-500'
                : props.realtimeTransport === 'connecting'
                  ? 'bg-sky-400'
                  : 'bg-red-500'
          "
        />
        <h2 class="text-sm font-semibold text-vp-text">{{ headingFor(props.view) }}</h2>
        <Badge variant="outline" class="font-mono text-[10px] uppercase tracking-wide">
          {{ t(`realtime.transport.${props.realtimeTransport}`) }}
        </Badge>
        <span class="ml-auto text-[11px] text-vp-muted">
          {{ t("obs.pollHint", { secs: props.pollMs / 1000 }) }}
        </span>
      </div>
    </div>

    <div class="flex-1 overflow-auto p-3">
      <!-- KPI strip (shown for all + waveform) -->
      <Card v-if="props.view !== 'logs'" class="mb-3 overflow-hidden">
        <div class="grid grid-cols-2 gap-px bg-vp-border lg:grid-cols-4">
          <div class="bg-vp-surface p-3">
            <div class="text-[11px] uppercase tracking-wide text-vp-muted">
              {{ t("obs.kpi.active") }}
            </div>
            <div class="mt-1 flex items-center justify-between gap-2">
              <span class="font-mono text-2xl font-semibold text-vp-text">{{ kpiActive }}</span>
              <Sparkline
                :values="sparkActive"
                color="rgb(16 185 129)"
                fill="rgba(16,185,129,0.15)"
                :label="t('obs.kpi.active')"
                :width="100"
                :height="32"
              />
            </div>
          </div>
          <div class="bg-vp-surface p-3">
            <div class="text-[11px] uppercase tracking-wide text-vp-muted">
              {{ t("obs.kpi.tokensPerSec") }}
            </div>
            <div class="mt-1 flex items-center justify-between gap-2">
              <span class="font-mono text-2xl font-semibold text-vp-text">
                {{ kpiTokTps > 0 ? kpiTokTps.toFixed(1) : "—" }}
              </span>
              <Sparkline
                :values="sparkTok"
                color="rgb(14 165 233)"
                fill="rgba(14,165,233,0.15)"
                :label="t('obs.kpi.tokensPerSec')"
                :width="100"
                :height="32"
              />
            </div>
          </div>
          <div class="bg-vp-surface p-3">
            <div class="text-[11px] uppercase tracking-wide text-vp-muted">
              {{ t("obs.kpi.network") }}
            </div>
            <div class="mt-1 flex items-center justify-between gap-2">
              <span class="font-mono text-lg font-semibold text-vp-text">
                {{ formatBytesPerSec(kpiBytesPerSec) }}
              </span>
              <Sparkline
                :values="sparkBytes"
                color="rgb(139 92 246)"
                fill="rgba(139,92,246,0.15)"
                :label="t('obs.kpi.network')"
                :width="100"
                :height="32"
              />
            </div>
          </div>
          <div class="bg-vp-surface p-3">
            <div class="text-[11px] uppercase tracking-wide text-vp-muted">
              {{ t("obs.kpi.cost") }}
            </div>
            <div class="mt-1 flex items-center justify-between gap-2">
              <span class="font-mono text-lg font-semibold text-vp-text">
                {{ formatUsdPerHour(kpiUsdPerHour) }}
              </span>
              <Sparkline
                :values="sparkUsd"
                color="rgb(245 158 11)"
                fill="rgba(245,158,11,0.15)"
                :label="t('obs.kpi.cost')"
                :width="100"
                :height="32"
              />
            </div>
          </div>
        </div>
      </Card>

      <!-- Logs -->
      <div v-if="props.view === 'logs' || props.view === 'all'" class="mb-3">
        <div
          class="flex items-center justify-between rounded-t-xl border border-vp-border bg-vp-surface px-4 py-3"
        >
          <div class="flex items-center gap-2">
            <VpIcon name="activity" size-class="size-4 text-vp-muted" />
            <span class="text-sm font-semibold text-vp-text">{{ t("obs.logs.title") }}</span>
          </div>
        </div>
        <Card class="overflow-hidden rounded-t-none border-t-0">
          <div class="max-h-[32rem] w-full overflow-auto">
            <LogsPanel :providers="props.providers" />
          </div>
        </Card>
      </div>

      <!-- Waveform -->
      <div
        v-if="props.view === 'waveform' || props.view === 'all'"
        class="grid gap-3 lg:grid-cols-2"
      >
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.waveform.activeRequests") }}
          </div>
          <div class="p-4">
            <Sparkline
              :values="sparkActive"
              color="rgb(16 185 129)"
              fill="rgba(16,185,129,0.18)"
              :width="640"
              :height="160"
              :label="t('obs.waveform.activeRequests')"
              class="w-full"
            />
            <div class="mt-2 text-xs text-vp-muted">
              {{ t("obs.waveform.windowHint", { secs: HISTORY_SIZE / 2 }) }}
            </div>
          </div>
        </Card>
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.kpi.tokensPerSec") }}
          </div>
          <div class="p-4">
            <Sparkline
              :values="sparkTok"
              color="rgb(14 165 233)"
              fill="rgba(14,165,233,0.18)"
              :width="640"
              :height="160"
              :label="t('obs.kpi.tokensPerSec')"
              class="w-full"
            />
          </div>
        </Card>
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.kpi.network") }}
          </div>
          <div class="p-4">
            <Sparkline
              :values="sparkBytes"
              color="rgb(139 92 246)"
              fill="rgba(139,92,246,0.18)"
              :width="640"
              :height="160"
              :label="t('obs.kpi.network')"
              class="w-full"
            />
          </div>
        </Card>
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.kpi.cost") }}
          </div>
          <div class="p-4">
            <Sparkline
              :values="sparkUsd"
              color="rgb(245 158 11)"
              fill="rgba(245,158,11,0.18)"
              :width="640"
              :height="160"
              :label="t('obs.kpi.cost')"
              class="w-full"
            />
          </div>
        </Card>
      </div>
    </div>
  </div>
</template>

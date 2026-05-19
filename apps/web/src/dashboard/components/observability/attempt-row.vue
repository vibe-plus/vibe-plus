<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { RealtimeAttempt, UpstreamAttemptLog } from "../../api/client.ts";
import Badge from "../ui/badge.vue";
import EntityChip from "../ui/entity-chip.vue";
import { protocolWireLetter } from "../../utils/protocol-label.ts";
import { credentialPrimaryAccountLabel } from "../../utils/providers-display.ts";

const props = defineProps<{
  attempt: UpstreamAttemptLog | RealtimeAttempt;
  providerName?: string | null;
  credentialLabel?: string | null;
  outcomeLabel?: string | null;
  phaseLabel?: string | null;
  waveLabel?: string | null;
  attemptLabel?: string | null;
  lifecycleLabels?: Partial<
    Record<
      "dispatch" | "upstreamFirstByte" | "clientFirstWrite" | "complete" | "terminal" | "elapsed",
      string
    >
  >;
  /** Optional highlight (matched the query param). */
  highlighted?: boolean;
  /** "compact" trims the secondary line; "detailed" shows headers/bytes. */
  density?: "compact" | "detailed";
}>();

const { t } = useI18n();

const outcomeTone = computed<"default" | "secondary" | "outline" | "destructive">(() => {
  switch (attemptOutcome.value) {
    case "success":
      return "secondary";
    case "race-aborted":
      return "outline";
    case "rate-limit":
    case "auth_error": // legacy
    case "payment_error": // legacy
      return "destructive";
    case "retryable-error":
    case "client-error":
    case "transport-error":
    case "fallback-abandon":
    case "circuit-skip":
    case "failure": // legacy
    case "server_error": // legacy
    case "not_found": // legacy
      return "destructive";
    default:
      return "outline";
  }
});

const outcomeClass = computed(() => {
  switch (attemptOutcome.value) {
    case "success":
      return "bg-emerald-100 text-emerald-700 hover:bg-emerald-100";
    case "race-aborted":
    case "fallback-abandon":
    case "circuit-skip":
      return "bg-slate-100 text-slate-600 hover:bg-slate-100";
    case "rate-limit":
      return "bg-amber-100 text-amber-800 hover:bg-amber-100";
    case "retryable-error":
    case "transport-error":
      return "bg-orange-100 text-orange-800 hover:bg-orange-100";
    case "client-error":
      return "bg-red-100 text-red-800 hover:bg-red-100";
    default:
      return "bg-red-100 text-red-700 hover:bg-red-100";
  }
});

const credentialLabel = computed(() => {
  const label = props.credentialLabel?.trim();
  if (label) return label;
  if (!props.attempt.credential_id) return null;
  return credentialPrimaryAccountLabel({
    id: props.attempt.credential_id,
    provider_id: props.attempt.provider_id ?? "",
    label: "credential",
    auth_ref: null,
    plan_type: null,
    notes: null,
    enabled: true,
    priority: 0,
    oauth_access_token: null,
    oauth_has_refresh: false,
    oauth_expires_at: null,
    rl_requests_limit: null,
    rl_requests_remaining: null,
    rl_requests_reset_at: null,
    rl_tokens_limit: null,
    rl_tokens_remaining: null,
    rl_tokens_reset_at: null,
    last_used_at: null,
    last_error: null,
    consecutive_failures: 0,
    created_at: 0,
    updated_at: 0,
  });
});

const attemptOutcome = computed(() =>
  "outcome" in props.attempt
    ? props.attempt.outcome
    : props.attempt.error
      ? "transport-error"
      : null,
);

const attemptPhase = computed(() => ("phase" in props.attempt ? props.attempt.phase : null));
const normalizedOutcome = computed<string>(() => attemptOutcome.value ?? "none");

const attemptOutputTokens = computed(() =>
  "output_tokens" in props.attempt
    ? props.attempt.output_tokens
    : props.attempt.output_tokens_so_far,
);

const attemptInputTokens = computed(() =>
  "input_tokens" in props.attempt ? props.attempt.input_tokens : 0,
);

const upstreamBytes = computed(() =>
  "upstream_bytes" in props.attempt
    ? props.attempt.upstream_bytes
    : props.attempt.upstream_bytes_so_far,
);

const clientBytes = computed(() =>
  "client_bytes" in props.attempt ? props.attempt.client_bytes : props.attempt.client_bytes_so_far,
);

const latencyMs = computed(() =>
  "latency_ms" in props.attempt
    ? props.attempt.latency_ms
    : Math.round(Math.max(0, (Date.now() / 1000 - props.attempt.started_at) * 1000)),
);

const firstTokenMs = computed(() =>
  "first_token_ms" in props.attempt ? props.attempt.first_token_ms : null,
);

const firstByteMs = computed(() =>
  "upstream_first_byte_ms" in props.attempt ? props.attempt.upstream_first_byte_ms : null,
);

const clientFirstWriteMs = computed(() =>
  "client_first_write_ms" in props.attempt ? props.attempt.client_first_write_ms : null,
);

const errorSummary = computed(() =>
  "error_summary" in props.attempt ? props.attempt.error_summary : props.attempt.error,
);

type TimelineStep = {
  key: string;
  label: string;
  value: string;
  active: boolean;
  tone: "ok" | "muted" | "warn" | "bad";
};

const timelineSteps = computed<TimelineStep[]>(() => {
  const firstByte = firstByteMs.value ?? firstTokenMs.value;
  const clientFirstWrite = clientFirstWriteMs.value ?? firstTokenMs.value;
  const finished = latencyMs.value != null && attemptOutcome.value !== null;
  return [
    {
      key: "dispatch",
      label: lifecycleLabel("dispatch"),
      value: "0ms",
      active: true,
      tone: "ok",
    },
    {
      key: "upstreamFirstByte",
      label: lifecycleLabel("upstreamFirstByte"),
      value: formatMs(firstByte),
      active: firstByte != null,
      tone: firstByte != null ? "ok" : "muted",
    },
    {
      key: "clientFirstWrite",
      label: lifecycleLabel("clientFirstWrite"),
      value: formatMs(clientFirstWrite),
      active: clientFirstWrite != null,
      tone: clientFirstWrite != null ? "ok" : "muted",
    },
    {
      key: "complete",
      label: lifecycleLabel(terminalStepKey.value),
      value: formatMs(latencyMs.value),
      active: finished,
      tone: attemptOutcome.value === "success" ? "ok" : attemptOutcome.value ? "bad" : "muted",
    },
  ];
});

const terminalStepKey = computed<"complete" | "terminal" | "elapsed">(() =>
  attemptOutcome.value === "success" ? "complete" : attemptOutcome.value ? "terminal" : "elapsed",
);

function lifecycleLabel(
  key: "dispatch" | "upstreamFirstByte" | "clientFirstWrite" | "complete" | "terminal" | "elapsed",
): string {
  return props.lifecycleLabels?.[key] ?? t(`obs.attemptLifecycle.${key}`);
}

function timelineStepClass(step: TimelineStep): string {
  if (!step.active) return "text-vp-muted/60";
  if (step.tone === "warn") return "text-amber-700";
  if (step.tone === "bad") return "text-red-700";
  return "text-vp-muted";
}

const currentTimelineIndex = computed(() => {
  const idx = timelineSteps.value.findLastIndex((step) => step.active);
  return idx < 0 ? 0 : idx;
});

const timelineProgress = computed(() => {
  const phase = attemptPhase.value ?? "";
  const outcome = normalizedOutcome.value;
  const hasOutcome = outcome !== "none";
  if (outcome === "success" || phase === "completed") return 100;
  if (hasOutcome && outcome !== "success") return 100;
  if (phase === "streaming") {
    if (clientBytes.value > 0) return 72;
    if (upstreamBytes.value > 0) return 48;
    return 34;
  }
  if (phase === "connecting" || phase === "routing") return 12;
  return firstTokenMs.value != null ? 24 : 8;
});

const timelineTone = computed(() => {
  const outcome = normalizedOutcome.value;
  if (outcome === "success" || attemptPhase.value === "completed") {
    return "bg-emerald-500";
  }
  if (outcome !== "none" && outcome !== "success") {
    return "bg-red-500";
  }
  if (attemptPhase.value === "streaming") return "bg-sky-500";
  if (attemptPhase.value === "connecting" || attemptPhase.value === "routing")
    return "bg-amber-500";
  return "bg-slate-400";
});

const currentTimelineLabel = computed(() => {
  const phase = attemptPhase.value;
  if (phase) return props.phaseLabel ?? phase;
  return props.outcomeLabel ?? "—";
});

const currentTimelineValue = computed(() => {
  if (currentTimelineIndex.value >= 3) return formatMs(latencyMs.value);
  if (currentTimelineIndex.value >= 2)
    return formatMs(clientFirstWriteMs.value ?? firstTokenMs.value);
  if (currentTimelineIndex.value >= 1) return formatMs(firstByteMs.value ?? firstTokenMs.value);
  return formatMs(0);
});

function formatBytes(b: number): string {
  if (!Number.isFinite(b) || b <= 0) return "0 B";
  if (b >= 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  if (b >= 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${b} B`;
}

function formatMs(ms: number | null): string {
  if (ms == null) return "—";
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString();
}
</script>

<template>
  <div
    class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-vp-border px-3 py-2 text-xs font-mono transition-colors"
    :class="highlighted ? 'bg-amber-50/40' : 'hover:bg-vp-bg-hover'"
  >
    <span class="shrink-0 text-vp-muted">{{ formatTime(attempt.started_at) }}</span>
    <EntityChip
      :kind="'provider'"
      :id="attempt.provider_id ?? ''"
      :label="providerName ?? attempt.provider_id ?? '—'"
      variant="inline"
    />
    <EntityChip
      v-if="attempt.credential_id"
      :kind="'credential'"
      :id="attempt.credential_id"
      :label="credentialLabel"
      variant="inline"
    />
    <EntityChip
      :kind="'wave'"
      :id="`${attempt.request_id}#${attempt.wave_index}`"
      :label="waveLabel ?? `Wave ${attempt.wave_index + 1}`"
      variant="inline"
    />
    <Badge :variant="outcomeTone" :class="outcomeClass">
      {{ outcomeLabel ?? phaseLabel ?? attempt.phase }}
    </Badge>
    <span class="text-vp-muted">
      HTTP {{ attempt.upstream_http_status ?? attempt.status_code ?? "—" }}
    </span>
    <span class="text-vp-muted">{{ formatMs(latencyMs) }}</span>
    <span class="text-vp-muted">
      ↓{{ formatBytes(upstreamBytes) }} · ↑{{ formatBytes(clientBytes) }}
    </span>
    <span v-if="attemptOutputTokens > 0" class="text-vp-muted">
      {{ attemptInputTokens }}↦{{ attemptOutputTokens }} tok
    </span>
    <span class="ml-auto flex shrink-0 items-center gap-2">
      <Badge variant="outline" class="font-mono text-[10px] uppercase tracking-wide">
        {{ protocolWireLetter(attempt.wire) }}
      </Badge>
      <EntityChip
        :kind="'request'"
        :id="attempt.request_id"
        :label="attempt.request_id.slice(0, 8)"
        variant="inline"
      />
      <EntityChip
        :kind="'attempt'"
        :id="attempt.attempt_id"
        :label="attemptLabel ?? `#${attempt.attempt_index + 1}`"
        variant="inline"
      />
    </span>
    <div class="basis-full pl-0 text-[11px]">
      <div class="mb-1 flex items-center gap-2 text-[10px] uppercase tracking-wide text-vp-muted">
        <span>{{ currentTimelineLabel }}</span>
        <span class="font-mono text-vp-text">{{ currentTimelineValue }}</span>
      </div>
      <div class="relative h-1.5 overflow-hidden rounded-full bg-vp-border/60">
        <div
          class="absolute inset-y-0 left-0 rounded-full transition-all duration-500 ease-out"
          :class="timelineTone"
          :style="{ width: `${timelineProgress}%` }"
        />
      </div>
      <div class="mt-1 flex items-center justify-between gap-1">
        <span
          v-for="(step, index) in timelineSteps"
          :key="step.key"
          class="flex min-w-0 flex-1 items-center gap-1"
          :class="timelineStepClass(step)"
        >
          <span
            class="size-2 rounded-full border"
            :class="[
              step.active ? timelineTone : 'border-vp-border bg-vp-surface',
              index === currentTimelineIndex ? 'ring-2 ring-offset-1 ring-offset-white' : '',
            ]"
          />
          <span class="truncate">{{ step.label }}</span>
        </span>
      </div>
    </div>
    <div v-if="density === 'detailed' && errorSummary" class="basis-full text-[11px] text-red-600">
      {{ errorSummary }}
    </div>
  </div>
</template>

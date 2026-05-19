<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { UpstreamAttemptLog } from "../../api/client.ts";
import Badge from "../ui/badge.vue";
import EntityChip from "../ui/entity-chip.vue";
import { protocolWireLetter } from "../../utils/protocol-label.ts";
import { credentialPrimaryAccountLabel } from "../../utils/providers-display.ts";

const props = defineProps<{
  attempt: UpstreamAttemptLog;
  providerName?: string | null;
  credentialLabel?: string | null;
  outcomeLabel?: string | null;
  waveLabel?: string | null;
  attemptLabel?: string | null;
  lifecycleLabels?: Partial<
    Record<"dispatch" | "upstreamFirstByte" | "clientFirstWrite" | "complete" | "terminal", string>
  >;
  /** Optional highlight (matched the query param). */
  highlighted?: boolean;
  /** "compact" trims the secondary line; "detailed" shows headers/bytes. */
  density?: "compact" | "detailed";
}>();

const { t } = useI18n();

const outcomeTone = computed<"default" | "secondary" | "outline" | "destructive">(() => {
  switch (props.attempt.outcome) {
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

const outcomeKey = computed(() => {
  switch (props.attempt.outcome) {
    case "success":
      return "success";
    case "race-aborted":
      return "raceAborted";
    case "rate-limit":
      return "rateLimit";
    case "retryable-error":
      return "retryableError";
    case "client-error":
      return "clientError";
    case "transport-error":
      return "transportError";
    case "fallback-abandon":
      return "fallbackAbandon";
    case "circuit-skip":
      return "circuitSkip";
    case "failure": // legacy
      return "failure";
    case "race_aborted": // legacy
      return "raceAborted";
    case "rate_limit": // legacy
      return "rateLimit";
    case "auth_error":
      return "authError";
    case "payment_error":
      return "paymentError";
    case "server_error":
      return "serverError";
    case "not_found":
      return "notFound";
    default:
      return props.attempt.outcome;
  }
});

const outcomeClass = computed(() => {
  switch (props.attempt.outcome) {
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

type TimelineStep = {
  key: string;
  label: string;
  value: string;
  active: boolean;
  tone: "ok" | "muted" | "warn";
};

const timelineSteps = computed<TimelineStep[]>(() => {
  const firstByte = props.attempt.upstream_first_byte_ms ?? props.attempt.first_token_ms;
  const clientFirstWrite = props.attempt.client_first_write_ms ?? props.attempt.first_token_ms;
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
      value: formatMs(props.attempt.latency_ms),
      active: props.attempt.latency_ms != null,
      tone: props.attempt.outcome === "success" ? "ok" : "warn",
    },
  ];
});

const terminalStepKey = computed<"complete" | "terminal">(() =>
  props.attempt.outcome === "success" ? "complete" : "terminal",
);

function lifecycleLabel(
  key: "dispatch" | "upstreamFirstByte" | "clientFirstWrite" | "complete" | "terminal",
): string {
  return props.lifecycleLabels?.[key] ?? t(`obs.attemptLifecycle.${key}`);
}

function timelineStepClass(step: TimelineStep): string {
  if (!step.active) return "text-vp-muted/60";
  if (step.tone === "warn") return "text-amber-700";
  return "text-vp-muted";
}

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
      {{ outcomeLabel ?? t(`obs.outcome.${outcomeKey}`) }}
    </Badge>
    <span class="text-vp-muted">
      HTTP {{ attempt.upstream_http_status ?? attempt.status_code ?? "—" }}
    </span>
    <span class="text-vp-muted">{{ formatMs(attempt.latency_ms) }}</span>
    <span class="text-vp-muted">
      ↓{{ formatBytes(attempt.upstream_bytes) }} · ↑{{ formatBytes(attempt.client_bytes) }}
    </span>
    <span v-if="attempt.output_tokens > 0" class="text-vp-muted">
      {{ attempt.input_tokens }}↦{{ attempt.output_tokens }} tok
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
      <span v-for="(step, index) in timelineSteps" :key="step.key" :class="timelineStepClass(step)">
        <span v-if="index > 0" class="mx-1 text-vp-muted/50">→</span>
        {{ step.label }}
        <span class="font-mono">{{ step.value }}</span>
      </span>
    </div>
    <div
      v-if="density === 'detailed' && attempt.error_summary"
      class="basis-full text-[11px] text-red-600"
    >
      {{ attempt.error_summary }}
    </div>
  </div>
</template>

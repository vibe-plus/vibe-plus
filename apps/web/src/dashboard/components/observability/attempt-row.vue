<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { UpstreamAttemptLog } from "../../api/client.ts";
import Badge from "../ui/badge.vue";
import EntityChip from "../ui/entity-chip.vue";

const props = defineProps<{
  attempt: UpstreamAttemptLog;
  providerName?: string | null;
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
    case "race_aborted":
      return "outline";
    case "rate_limit":
    case "auth_error":
    case "payment_error":
      return "destructive";
    case "server_error":
    case "failure":
    case "not_found":
      return "destructive";
    default:
      return "outline";
  }
});

const outcomeClass = computed(() => {
  switch (props.attempt.outcome) {
    case "success":
      return "bg-emerald-100 text-emerald-700 hover:bg-emerald-100";
    case "race_aborted":
      return "bg-slate-100 text-slate-600 hover:bg-slate-100";
    case "rate_limit":
      return "bg-amber-100 text-amber-800 hover:bg-amber-100";
    case "auth_error":
    case "payment_error":
      return "bg-red-100 text-red-800 hover:bg-red-100";
    default:
      return "bg-red-100 text-red-700 hover:bg-red-100";
  }
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
      class="!text-foreground"
    />
    <EntityChip
      v-if="attempt.credential_id"
      :kind="'credential'"
      :id="attempt.credential_id"
      :label="attempt.credential_id.slice(0, 6)"
      variant="inline"
    />
    <EntityChip
      :kind="'wave'"
      :id="`${attempt.request_id}#${attempt.wave_index}`"
      :label="`wave ${attempt.wave_index + 1}/${attempt.wave_size}`"
      variant="inline"
    />
    <Badge :variant="outcomeTone" :class="outcomeClass">
      {{ t(`observability.outcome.${attempt.outcome}`) }}
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
      <EntityChip
        :kind="'request'"
        :id="attempt.request_id"
        :label="attempt.request_id.slice(0, 8)"
        variant="inline"
      />
      <EntityChip
        :kind="'attempt'"
        :id="attempt.attempt_id"
        :label="`a${attempt.attempt_index}`"
        variant="inline"
      />
    </span>
    <div
      v-if="density === 'detailed' && attempt.error_summary"
      class="basis-full text-[11px] text-red-600"
    >
      {{ attempt.error_summary }}
    </div>
  </div>
</template>

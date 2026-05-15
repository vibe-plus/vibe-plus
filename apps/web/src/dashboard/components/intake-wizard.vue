<script setup lang="ts">
import { computed } from "vue";
import { useIntakeFlow } from "../composables/use-intake-flow.ts";
import VpIcon from "./vp-icon.vue";
import IntakeLootAnimation from "./intake-loot-animation.vue";
import ProviderLogo from "./provider-logo.vue";
import { displayProviderName } from "../utils/providers-display.ts";
import { protocolLabel } from "../utils/protocol-label.ts";
import { brandHintFromHost } from "../utils/brand-hint.ts";
import type { ProbeResult, ProviderBalanceSnapshot } from "../api/intake-types.ts";

const flow = useIntakeFlow();
const {
  open,
  mode,
  candidates,
  providers,
  probing,
  importing,
  loadingProviders,
  toast,
  error,
  shyPromptOpen,
  shyCandidates,
  successCount,
  selectedCount,
  remoteCandidate,
  remoteCandidates,
  remotePreviewByCandidate,
  remoteImportResultByCandidate,
  lootOpen,
  lootLabel,
  lootTargetProviderId,
  candidateSuccessMap,
} = flow;

const headerLabel = computed(() => {
  if (remoteCandidate.value) return "Remote provider import";
  if (mode.value === "aggressive") return "Quick credential import";
  if (mode.value === "shy") return "Importable credentials found";
  return "Smart import";
});

function candidatePrefix(idx: number): string {
  return String.fromCharCode(65 + idx);
}

function statusOf(candidateId: string, providerId: string) {
  const r = flow.resultFor(candidateId, providerId);
  if (probing.value && !r) return { tone: "pending" as const, label: "…" };
  if (!r) return { tone: "pending" as const, label: "…" };
  if (r.skipped) return { tone: "skip" as const, label: "skip", error: r.skip_reason ?? undefined };
  if (r.ok)
    return { tone: "ok" as const, label: r.latency_ms != null ? `${r.latency_ms}ms` : "ok" };
  return {
    tone: "bad" as const,
    label: r.status ? `${r.status}` : "err",
    error: r.error ?? r.skip_reason ?? undefined,
  };
}

function rowToggle(candidateId: string, providerId: string, disabled: boolean) {
  if (disabled) return;
  flow.toggleSelection(candidateId, providerId);
}

function statusClass(tone: "pending" | "ok" | "bad" | "skip"): string {
  switch (tone) {
    case "ok":
      return "border-emerald-200 bg-emerald-50 text-emerald-800";
    case "bad":
      return "border-red-200 bg-red-50 text-red-800";
    case "skip":
      return "border-amber-200 bg-amber-50 text-amber-800";
    default:
      return "border-slate-200 bg-slate-50 text-slate-500";
  }
}

function moneyLabel(snapshot: ProviderBalanceSnapshot | null | undefined): string {
  if (!snapshot) return "Not fetched";
  const currency = snapshot.currency || "USD";
  const primary = snapshot.balance ?? snapshot.remaining ?? snapshot.used ?? snapshot.total;
  if (!primary) return currency;
  return `${currency} ${primary}`;
}

/** Matches gateway `ProbeResult` serialization for log correlation and debugging. */
function probeResponseSummary(r: ProbeResult): string {
  const lines = [
    `candidate_id: ${r.candidate_id}`,
    `ok: ${r.ok}`,
    `skipped: ${r.skipped}`,
    `status: ${r.status ?? "null"}`,
    `latency_ms: ${r.latency_ms}`,
    `provider_id: ${r.provider_id}`,
    `provider_name: ${r.provider_name}`,
    `provider_kind: ${r.provider_kind}`,
  ];
  if (r.error) lines.push(`error: ${r.error}`);
  if (r.skip_reason) lines.push(`skip_reason: ${r.skip_reason}`);
  return lines.join("\n");
}

function probeDetailText(candidateId: string, providerId: string): string {
  const r = flow.resultFor(candidateId, providerId);
  return r ? probeResponseSummary(r) : "";
}

const remoteCandidateRows = computed(() =>
  remoteCandidates.value.map((cand) => ({
    cand,
    preview: remotePreviewByCandidate.value[cand.id] ?? null,
  })),
);
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="vp-modal-backdrop z-[130] overflow-y-auto py-6"
      role="dialog"
      aria-modal="true"
      aria-label="intake.wizard"
      @click.self="flow.closeWizard()"
    >
      <div class="vp-modal-panel max-w-5xl flex flex-col my-auto" @click.stop>
        <div class="vp-modal-header">
          <span
            class="grid size-10 shrink-0 place-items-center rounded-xl bg-violet-100 text-violet-700 ring-1 ring-violet-200"
            aria-hidden="true"
          >
            <VpIcon :name="remoteCandidate ? 'globe' : 'key'" size-class="size-5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 class="font-semibold text-lg text-vp-text">{{ headerLabel }}</h2>
            <p class="text-xs text-vp-muted font-mono mt-0.5">
              <template v-if="remoteCandidate">
                {{ remoteCandidates.length }} remote candidates · automatically fetch branding,
                models, balance, and usage
              </template>
              <template v-else>
                {{ candidates.length }} candidate · {{ providers.length }} provider ·
                {{ probing ? "probing…" : `${successCount} ok` }}
              </template>
            </p>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            aria-label="close"
            title="close"
            @click="flow.closeWizard()"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>

        <div class="px-4 sm:px-6 py-4 overflow-y-auto max-h-[min(42rem,76vh)] space-y-4">
          <div
            v-if="error"
            class="text-sm text-red-700 bg-red-50 border border-red-200 rounded-lg px-3 py-2"
          >
            {{ error }}
          </div>

          <div v-if="loadingProviders" class="text-sm text-slate-500 py-2 flex items-center gap-2">
            <VpIcon name="loader-2" size-class="size-4 animate-spin" /> providers:loading
          </div>

          <div v-if="remoteCandidate" class="grid gap-4 lg:grid-cols-2">
            <article
              v-for="{ cand, preview } in remoteCandidateRows"
              :key="cand.id"
              class="rounded-2xl border border-vp-border bg-[color-mix(in_srgb,var(--vp-primary)_3%,var(--vp-surface))] p-4 shadow-sm"
            >
              <template v-if="preview">
                <div class="flex items-start gap-3">
                  <ProviderLogo
                    :kind="preview.detected_kind as any"
                    :avatar-url="preview.avatar_url"
                    :provider-name="preview.display_name"
                    :host-hint="preview.detected_base_url"
                    :base-url="preview.detected_base_url"
                    :brand-hint="
                      brandHintFromHost(preview.detected_base_url) ??
                      brandHintFromHost(preview.display_name)
                    "
                    size-class="size-12"
                    icon-size-class="size-6"
                  />
                  <div class="min-w-0 flex-1">
                    <div class="flex flex-wrap items-center gap-1.5 min-w-0">
                      <h3 class="truncate text-base font-semibold text-vp-text">
                        {{ preview.display_name }}
                      </h3>
                      <span
                        v-for="proto in preview.detected_protocols?.length
                          ? preview.detected_protocols
                          : [
                              {
                                kind: preview.detected_kind,
                                label: protocolLabel(preview.detected_kind),
                              },
                            ]"
                        :key="`${proto.kind}-${proto.base_url ?? ''}`"
                        class="shrink-0 rounded-full border border-vp-border px-2 py-0.5 text-[10px] font-medium text-vp-muted"
                        :title="proto.base_url"
                      >
                        {{ proto.label ?? protocolLabel(proto.kind) }}
                      </span>
                    </div>
                    <p class="mt-1 break-all font-mono text-[11px] text-vp-muted">
                      {{ preview.detected_base_url }}
                    </p>
                    <p class="mt-1 text-xs text-vp-muted">{{ preview.note }}</p>
                  </div>
                </div>

                <div class="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                  <div class="rounded-xl border border-vp-border bg-vp-surface px-3 py-2">
                    <div class="text-[11px] uppercase tracking-wide text-vp-muted">Avatar</div>
                    <div class="mt-1 text-sm font-medium text-vp-text">
                      {{ preview.capabilities.can_fetch_branding ? "Detected" : "Not detected" }}
                    </div>
                  </div>
                  <div class="rounded-xl border border-vp-border bg-vp-surface px-3 py-2">
                    <div class="text-[11px] uppercase tracking-wide text-vp-muted">Models</div>
                    <div class="mt-1 text-sm font-medium text-vp-text">
                      {{ preview.remote_models.length }}
                    </div>
                  </div>
                  <div class="rounded-xl border border-vp-border bg-vp-surface px-3 py-2">
                    <div class="text-[11px] uppercase tracking-wide text-vp-muted">Balance</div>
                    <div class="mt-1 text-sm font-medium text-vp-text">
                      {{ moneyLabel(preview.balance) }}
                    </div>
                  </div>
                  <div class="rounded-xl border border-vp-border bg-vp-surface px-3 py-2">
                    <div class="text-[11px] uppercase tracking-wide text-vp-muted">Usage</div>
                    <div class="mt-1 text-sm font-medium text-vp-text">
                      {{ moneyLabel(preview.usage) }}
                    </div>
                  </div>
                </div>

                <div class="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
                  <section class="rounded-xl border border-vp-border bg-vp-surface p-3">
                    <div class="flex items-center justify-between gap-2">
                      <h4 class="text-sm font-semibold text-vp-text">Model list</h4>
                      <span class="text-[11px] font-mono text-vp-muted"
                        >{{ preview.remote_models.length }} models</span
                      >
                    </div>
                    <div v-if="preview.remote_models.length" class="mt-3 flex flex-wrap gap-1.5">
                      <span
                        v-for="model in preview.remote_models.slice(0, 24)"
                        :key="model"
                        class="rounded-full border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))] px-2 py-1 text-[11px] font-mono text-vp-text"
                      >
                        {{ model }}
                      </span>
                    </div>
                    <p v-else class="mt-3 text-xs text-vp-muted">No model list was detected.</p>
                  </section>

                  <section class="rounded-xl border border-vp-border bg-vp-surface p-3">
                    <h4 class="text-sm font-semibold text-vp-text">Import status</h4>
                    <div
                      v-if="remoteImportResultByCandidate[cand.id]"
                      class="mt-3 space-y-2 text-sm"
                    >
                      <div
                        class="rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-emerald-800"
                      >
                        Provider and credential created
                      </div>
                      <div class="text-xs text-vp-muted">
                        Provider ID: {{ remoteImportResultByCandidate[cand.id]?.provider.id }}
                      </div>
                    </div>
                    <div v-else class="mt-3 space-y-2 text-xs text-vp-muted">
                      <div>Alias sync: {{ preview.model_aliases.length }} items</div>
                      <div>Mode: {{ preview.passthrough_mode ? "passthrough" : "mapped" }}</div>
                      <div>
                        Fetched at: {{ new Date(preview.fetched_at * 1000).toLocaleString() }}
                      </div>
                    </div>
                  </section>
                </div>
              </template>

              <div v-else class="flex items-center gap-2 text-sm text-vp-muted">
                <VpIcon name="loader-2" size-class="size-4 animate-spin" />
                Fetching provider branding and quota details…
              </div>
            </article>
          </div>

          <div v-else class="space-y-4">
            <div
              v-for="(cand, idx) in candidates"
              :key="cand.id"
              class="rounded-lg border border-vp-border overflow-hidden"
            >
              <div
                class="px-3 py-2 bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))] flex items-center gap-2"
              >
                <span
                  class="inline-flex size-6 items-center justify-center rounded-md bg-violet-100 text-violet-700 text-xs font-semibold"
                  >{{ candidatePrefix(idx) }}</span
                >
                <div class="min-w-0 flex-1">
                  <div class="truncate text-sm font-medium text-vp-text">{{ cand.summary }}</div>
                  <div class="truncate text-[11px] font-mono text-vp-muted">{{ cand.preview }}</div>
                </div>
                <span
                  class="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium"
                  :class="
                    candidateSuccessMap[cand.id]
                      ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                      : 'bg-slate-100 text-slate-500 border border-slate-200'
                  "
                >
                  {{ candidateSuccessMap[cand.id] ? "ok" : "pending" }}
                </span>
              </div>

              <ul class="divide-y divide-vp-border">
                <li
                  v-for="p in providers"
                  :key="p.id"
                  class="flex flex-col px-3 py-2 transition-colors"
                  :class="
                    flow.isSelectable(cand.id, p.id)
                      ? 'cursor-pointer hover:bg-vp-bg-hover'
                      : 'opacity-70'
                  "
                  @click="rowToggle(cand.id, p.id, !flow.isSelectable(cand.id, p.id))"
                >
                  <div class="flex items-center gap-3">
                    <input
                      type="checkbox"
                      class="size-4 shrink-0 rounded border-vp-border text-violet-600 focus:ring-violet-500"
                      :checked="flow.isChecked(cand.id, p.id)"
                      :disabled="!flow.isSelectable(cand.id, p.id)"
                      @click.stop
                      @change="rowToggle(cand.id, p.id, !flow.isSelectable(cand.id, p.id))"
                    />
                    <ProviderLogo
                      :kind="p.kind"
                      :avatar-url="p.avatar_url ?? null"
                      :provider-name="displayProviderName(p.name)"
                      size-class="size-8"
                      icon-size-class="size-4"
                    />
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-sm font-medium text-vp-text">
                        {{ displayProviderName(p.name) }}
                      </div>
                      <div class="truncate text-[11px] font-mono text-vp-muted">
                        {{ p.base_url }}
                      </div>
                    </div>
                    <span
                      class="shrink-0 rounded-full border px-2 py-1 text-[11px] font-mono"
                      :class="statusClass(statusOf(cand.id, p.id).tone)"
                      :title="statusOf(cand.id, p.id).error"
                    >
                      <span
                        v-if="probing && !flow.resultFor(cand.id, p.id)"
                        class="inline-block h-3 w-8 rounded bg-slate-200 animate-pulse"
                      />
                      <span v-else>{{ statusOf(cand.id, p.id).label }}</span>
                    </span>
                  </div>
                  <details
                    v-if="flow.resultFor(cand.id, p.id)"
                    class="mt-1.5 ml-7 sm:ml-9 text-left"
                    @click.stop
                  >
                    <summary
                      class="list-none cursor-pointer select-none text-[10px] font-medium text-violet-600 hover:text-violet-800 [&::-webkit-details-marker]:hidden"
                    >
                      Probe details
                    </summary>
                    <pre
                      class="mt-1 max-h-36 overflow-y-auto rounded-md border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] p-2 text-[10px] font-mono leading-snug text-vp-text whitespace-pre-wrap break-all"
                      >{{ probeDetailText(cand.id, p.id) }}</pre
                    >
                  </details>
                </li>
              </ul>
            </div>
          </div>
        </div>

        <div
          class="flex items-center gap-3 px-4 sm:px-6 py-4 border-t border-vp-border justify-between bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
        >
          <p class="text-xs text-vp-muted font-mono">
            <template v-if="remoteCandidate">
              {{
                remoteCandidates.length
                  ? `${remoteCandidates.length} remote candidates pending`
                  : "Auto-detecting remote provider"
              }}
            </template>
            <template v-else
              >Selected {{ selectedCount }} combinations (candidate × provider)</template
            >
          </p>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="btn-ghost flex items-center gap-2 !px-3"
              aria-label="cancel"
              @click="flow.closeWizard()"
            >
              <span>cancel</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              :disabled="probing || importing || selectedCount === 0"
              aria-label="intake:confirm"
              @click="flow.confirmImport()"
            >
              <VpIcon
                v-if="importing || probing"
                name="loader-2"
                size-class="size-4 text-white animate-spin"
              />
              <VpIcon v-else name="check" size-class="size-4 text-white" />
              <span>{{
                remoteCandidate
                  ? importing
                    ? "Bulk import in progress"
                    : probing
                      ? "Auto-detecting"
                      : "Import provider"
                  : `Add to ${selectedCount} providers`
              }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>

  <IntakeLootAnimation
    :open="lootOpen"
    :label="lootLabel"
    :target-provider-id="lootTargetProviderId"
  />

  <Teleport to="body">
    <div
      v-if="shyPromptOpen && !open"
      class="fixed bottom-4 right-4 z-[120] w-80 max-w-[calc(100vw-2rem)] rounded-lg border border-vp-border bg-vp-surface shadow-xl p-3"
    >
      <div class="flex items-start gap-2">
        <span
          class="inline-flex size-8 shrink-0 items-center justify-center rounded-lg bg-violet-100 text-violet-700"
        >
          <VpIcon name="sparkles" size-class="size-4" />
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-xs font-semibold text-vp-text">
            Clipboard contains {{ shyCandidates.length }} credential candidates
          </p>
          <p class="text-[11px] text-vp-muted font-mono truncate mt-0.5">
            {{ shyCandidates.map((c) => c.preview).join(" · ") }}
          </p>
        </div>
      </div>
      <div class="mt-2.5 flex justify-end gap-2">
        <button
          type="button"
          class="btn-ghost flex items-center gap-2 !px-2.5 !py-1 text-xs"
          @click="flow.dismissShy()"
        >
          Dismiss
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-1.5 px-3 py-1 text-xs rounded-md bg-violet-600 hover:bg-violet-700 text-white font-medium"
          @click="flow.acceptShy()"
        >
          <VpIcon name="zap" size-class="size-3.5 text-white" />
          Open wizard
        </button>
      </div>
    </div>
  </Teleport>

  <Teleport to="body">
    <div
      v-if="toast"
      class="fixed bottom-4 right-4 z-[140] w-80 max-w-[calc(100vw-2rem)] rounded-lg border shadow-xl p-3"
      :class="{
        'border-emerald-200 bg-emerald-50': toast.tone === 'ok',
        'border-amber-200 bg-amber-50': toast.tone === 'warn',
        'border-red-200 bg-red-50': toast.tone === 'bad',
      }"
    >
      <div class="flex items-start gap-2">
        <VpIcon
          :name="toast.tone === 'ok' ? 'check' : toast.tone === 'warn' ? 'alert-triangle' : 'x'"
          size-class="size-4 shrink-0 mt-0.5"
        />
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium text-slate-900">{{ toast.text }}</p>
          <pre
            v-if="toast.detail"
            class="mt-1.5 text-[11px] font-mono text-slate-700 whitespace-pre-wrap break-all max-h-32 overflow-y-auto"
            >{{ toast.detail }}</pre
          >
        </div>
      </div>
    </div>
  </Teleport>
</template>

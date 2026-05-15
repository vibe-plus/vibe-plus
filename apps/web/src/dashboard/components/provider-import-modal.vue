<script setup lang="ts">
import { ref, watch } from "vue";
import { api, type LocalCandidate, type ProviderKind } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import ProviderLogo from "./provider-logo.vue";

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  close: [];
  imported: [];
}>();

type ScanState = "idle" | "scanning" | "ready" | "error";

const scanState = ref<ScanState>("idle");
const candidates = ref<LocalCandidate[]>([]);
const scanError = ref("");
const importingSet = ref<Set<string>>(new Set());
const importAllBusy = ref(false);
const importError = ref("");

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    candidates.value = [];
    scanError.value = "";
    importError.value = "";
    importingSet.value = new Set();
    scan();
  },
);

async function scan() {
  scanState.value = "scanning";
  try {
    const result = await api.providers.scanLocal();
    candidates.value = result.map((c) => ({ ...c, extra_credentials: c.extra_credentials ?? [] }));
    scanState.value = "ready";
  } catch (e) {
    scanError.value = String(e);
    scanState.value = "error";
  }
}

async function importOne(client: string) {
  importingSet.value = new Set([...importingSet.value, client]);
  importError.value = "";
  try {
    await api.providers.importLocal([client]);
    candidates.value = candidates.value.filter((c) => c.client !== client);
    if (candidates.value.length === 0) {
      emit("imported");
      emit("close");
    }
  } catch (e) {
    importError.value = String(e);
  } finally {
    const next = new Set(importingSet.value);
    next.delete(client);
    importingSet.value = next;
  }
}

async function importAll() {
  importAllBusy.value = true;
  importError.value = "";
  const allClients = candidates.value.map((c) => c.client);
  try {
    await api.providers.importLocal(allClients);
    emit("imported");
    emit("close");
  } catch (e) {
    importError.value = String(e);
  } finally {
    importAllBusy.value = false;
  }
}

function kindLabel(kind: ProviderKind): string {
  switch (kind) {
    case "openai-responses":
      return "OpenAI Responses";
    case "openai-chat":
      return "OpenAI Chat";
    case "anthropic":
      return "Anthropic";
    case "gemini-native":
      return "Gemini Native";
    default:
      return kind;
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="vp-modal-backdrop z-[110]"
      role="dialog"
      aria-modal="true"
      aria-labelledby="provider-import-title"
      @click.self="emit('close')"
    >
      <div
        class="vp-modal-panel flex max-h-[min(90dvh,36rem)] w-[min(100vw-1rem,36rem)] flex-col"
        @click.stop
      >
        <!-- Header -->
        <div class="vp-modal-header border-b border-vp-border/70">
          <span
            class="grid size-9 shrink-0 place-items-center rounded-xl bg-cyan-100 text-cyan-700 ring-1 ring-cyan-200"
          >
            <VpIcon name="download" size-class="size-4.5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="provider-import-title" class="text-base font-semibold text-vp-text">
              Local import
            </h2>
            <p class="mt-0.5 text-xs text-vp-muted">
              Scan locally installed AI tools and import their configuration in one click.
            </p>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            aria-label="Close"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>

        <!-- Body -->
        <div class="flex-1 overflow-y-auto px-5 py-4">
          <!-- Scanning -->
          <div
            v-if="scanState === 'scanning'"
            class="flex items-center justify-center gap-2 py-10 text-sm text-slate-500"
          >
            <VpIcon name="loader-2" size-class="size-4 animate-spin" />
            Scanning local tools…
          </div>

          <!-- Scan error -->
          <div
            v-else-if="scanState === 'error'"
            class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
          >
            <p class="font-medium">Scan failed</p>
            <p class="mt-1 text-xs font-mono">{{ scanError }}</p>
            <button
              type="button"
              class="mt-2 text-xs text-red-600 underline hover:no-underline"
              @click="scan"
            >
              Retry
            </button>
          </div>

          <!-- Empty -->
          <div
            v-else-if="scanState === 'ready' && candidates.length === 0"
            class="flex flex-col items-center justify-center gap-2 py-10 text-sm text-slate-500"
          >
            <VpIcon name="archive" size-class="size-8 text-slate-300" />
            <p>No importable local tools found</p>
            <p class="text-xs text-slate-400">
              Supports Codex CLI (~/.codex), Claude (~/.claude), and more
            </p>
          </div>

          <!-- Candidates list -->
          <div v-else-if="scanState === 'ready'" class="space-y-2.5">
            <p
              v-if="importError"
              class="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700"
            >
              {{ importError }}
            </p>

            <div
              v-for="c in candidates"
              :key="c.client"
              class="flex items-start gap-3 rounded-2xl border border-vp-border bg-white p-4 transition-shadow hover:shadow-sm"
            >
              <ProviderLogo
                :kind="c.kind"
                :avatar-url="null"
                :provider-name="c.name"
                size-class="size-10 shrink-0"
                icon-size-class="size-5"
              />
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="font-semibold text-slate-900">{{ c.name }}</span>
                  <span
                    class="rounded-full border border-slate-200 bg-slate-50 px-1.5 py-0.5 text-[10px] font-mono text-slate-600"
                  >
                    {{ kindLabel(c.kind) }}
                  </span>
                  <span
                    :class="
                      c.token_ok
                        ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                        : 'border-amber-200 bg-amber-50 text-amber-700'
                    "
                    class="rounded-full border px-1.5 py-0.5 text-[10px] font-medium"
                  >
                    {{ c.token_ok ? "Token OK" : "Token missing" }}
                  </span>
                </div>
                <p class="mt-1 truncate font-mono text-[11px] text-slate-400">
                  {{ c.source_path }}
                </p>
                <div
                  v-if="(c.extra_credentials?.length ?? 0) > 0"
                  class="mt-1.5 flex flex-wrap gap-1"
                >
                  <span class="text-[11px] text-slate-500">
                    +{{ c.extra_credentials.length }} extra account(s)
                  </span>
                </div>
              </div>

              <button
                type="button"
                class="shrink-0 rounded-lg bg-violet-600 p-2.5 text-white transition-colors hover:bg-violet-700 disabled:opacity-50"
                :disabled="importingSet.has(c.client) || importAllBusy"
                :title="importingSet.has(c.client) ? 'Importing…' : 'Import'"
                @click="importOne(c.client)"
              >
                <VpIcon
                  :name="importingSet.has(c.client) ? 'loader-2' : 'download'"
                  size-class="size-4"
                  :spin="importingSet.has(c.client)"
                />
              </button>
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex items-center justify-between gap-2 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-5 py-3"
        >
          <button
            type="button"
            class="btn-ghost inline-flex items-center gap-1.5 px-3 py-2 text-sm"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            v-if="candidates.length > 0"
            type="button"
            class="inline-flex items-center gap-2 rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-700 disabled:opacity-50"
            :disabled="importAllBusy || importingSet.size > 0"
            @click="importAll"
          >
            <VpIcon
              :name="importAllBusy ? 'loader-2' : 'download'"
              size-class="size-4"
              :spin="importAllBusy"
            />
            Import all ({{ candidates.length }})
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

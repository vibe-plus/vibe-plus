<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { api, type LocalCandidate, type Provider } from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderLogo from "../../../components/provider-logo.vue";

const { t } = useI18n();
import type { WorkspaceView } from "../../../utils/workspace-view.ts";
import {
  candidateProtocolKinds,
  protocolKeysForCandidate,
  protocolKeysForProvider,
} from "../../../utils/provider-protocols.ts";

const props = defineProps<{
  open: boolean;
  view?: WorkspaceView;
}>();

const emit = defineEmits<{
  close: [];
  imported: [];
}>();

type ScanState = "idle" | "scanning" | "ready" | "error";
type ProtocolFilter = "all" | "M" | "R";

const scanState = ref<ScanState>("idle");
const candidates = ref<LocalCandidate[]>([]);
const existingProviders = ref<Provider[]>([]);
const scanError = ref("");
const importingSet = ref<Set<string>>(new Set());
const importAllBusy = ref(false);
const importError = ref("");
const protocolFilter = ref<ProtocolFilter>("all");

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    candidates.value = [];
    existingProviders.value = [];
    scanError.value = "";
    importError.value = "";
    importingSet.value = new Set();
    protocolFilter.value = "all";
    scan();
  },
);

async function scan() {
  scanState.value = "scanning";
  try {
    const [result, provs] = await Promise.all([api.providers.scanLocal(), api.providers.list()]);
    candidates.value = result.map((c) => ({ ...c, extra_credentials: c.extra_credentials ?? [] }));
    existingProviders.value = provs;
    scanState.value = "ready";
  } catch (e) {
    scanError.value = String(e);
    scanState.value = "error";
  }
}

const existingKeys = computed(() => {
  const s = new Set<string>();
  for (const p of existingProviders.value) {
    for (const key of protocolKeysForProvider(p)) s.add(key);
  }
  return s;
});

function isAlreadyImported(c: LocalCandidate): boolean {
  const keys = protocolKeysForCandidate(c);
  return keys.some((key) => existingKeys.value.has(key));
}

function candidateKinds(c: LocalCandidate): string[] {
  return candidateProtocolKinds(c);
}

/** Candidates filtered by workspaceView and the active protocol chip. */
const visibleCandidates = computed(() => {
  let list = candidates.value;
  if (props.view === "claude") {
    list = list.filter((c) => candidateKinds(c).includes("anthropic"));
  } else if (props.view === "codex") {
    list = list.filter((c) => candidateKinds(c).includes("openai-responses"));
  } else {
    if (protocolFilter.value === "M") {
      list = list.filter((c) => candidateKinds(c).includes("anthropic"));
    } else if (protocolFilter.value === "R") {
      list = list.filter((c) => candidateKinds(c).includes("openai-responses"));
    }
  }
  return list;
});

/** In overview mode, show protocol filter chips only if both kinds are present. */
const showFilterChips = computed(() => {
  if (props.view && props.view !== "overview") return false;
  const hasM = candidates.value.some((c) => candidateKinds(c).includes("anthropic"));
  const hasR = candidates.value.some((c) => candidateKinds(c).includes("openai-responses"));
  return hasM && hasR;
});

async function importOne(client: string) {
  const row = candidates.value.find((c) => c.client === client);
  if (row && isAlreadyImported(row)) return;
  importingSet.value = new Set([...importingSet.value, client]);
  importError.value = "";
  try {
    await api.providers.importLocal([client]);
    // reload providers so isAlreadyImported updates
    existingProviders.value = await api.providers.list();
    if (visibleCandidates.value.every((c) => isAlreadyImported(c))) {
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
  const pending = visibleCandidates.value.filter((c) => !isAlreadyImported(c)).map((c) => c.client);
  try {
    await api.providers.importLocal(pending);
    emit("imported");
    emit("close");
  } catch (e) {
    importError.value = String(e);
  } finally {
    importAllBusy.value = false;
  }
}

const KIND_BADGE: Record<string, { letter: string; rest: string }> = {
  anthropic: { letter: "M", rest: "essages" },
  "openai-responses": { letter: "R", rest: "esponses" },
  "openai-chat": { letter: "C", rest: "hat" },
  "gemini-native": { letter: "G", rest: "emini" },
};

const pendingCount = computed(
  () => visibleCandidates.value.filter((c) => !isAlreadyImported(c)).length,
);
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
              {{ t("title") }}
            </h2>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            :aria-label="t('actions.close')"
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
            {{ t("scan.scanning") }}
          </div>

          <!-- Scan error -->
          <div
            v-else-if="scanState === 'error'"
            class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
          >
            <p class="font-medium">{{ t("scan.failed") }}</p>
            <p class="mt-1 text-xs font-mono">{{ scanError }}</p>
            <button
              type="button"
              class="mt-2 text-xs text-red-600 underline hover:no-underline"
              @click="scan"
            >
              {{ t("actions.retry") }}
            </button>
          </div>

          <template v-else-if="scanState === 'ready'">
            <!-- Protocol filter chips (overview only, when both kinds present) -->
            <div v-if="showFilterChips" class="mb-3 flex gap-1.5">
              <button
                v-for="chip in [
                  { id: 'all', label: t('filters.all') },
                  { id: 'M', label: t('filters.messages') },
                  { id: 'R', label: t('filters.responses') },
                ] as const"
                :key="chip.id"
                type="button"
                :class="[
                  'rounded-full border px-3 py-1 text-xs font-medium transition-colors',
                  protocolFilter === chip.id
                    ? 'border-violet-500 bg-violet-500 text-white'
                    : 'border-vp-border bg-white text-slate-600 hover:border-violet-300 hover:text-violet-600',
                ]"
                @click="protocolFilter = chip.id"
              >
                {{ chip.label }}
              </button>
            </div>

            <!-- Empty -->
            <div
              v-if="visibleCandidates.length === 0"
              class="flex flex-col items-center justify-center gap-2 py-10 text-sm text-slate-500"
            >
              <VpIcon name="archive" size-class="size-8 text-slate-300" />
              <p>{{ t("empty.title") }}</p>
              <p class="text-xs text-slate-400">
                {{ t("empty.description") }}
              </p>
            </div>

            <!-- Candidates list -->
            <div v-else class="space-y-2.5">
              <p
                v-if="importError"
                class="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700"
              >
                {{ importError }}
              </p>

              <div
                v-for="c in visibleCandidates"
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
                    <!-- Protocol badge: shows single letter, expands to full word on hover -->
                    <span
                      v-for="kind in candidateKinds(c)"
                      :key="`${c.client}-${kind}`"
                      class="group/kb inline-flex items-baseline overflow-hidden rounded-full border border-slate-200 bg-slate-50 px-1.5 py-0.5 text-[10px] font-mono text-slate-600"
                    >
                      {{ KIND_BADGE[kind]?.letter ?? kind }}
                      <span
                        class="max-w-0 overflow-hidden whitespace-nowrap transition-[max-width] duration-200 ease-out group-hover/kb:max-w-[5rem]"
                        >{{ KIND_BADGE[kind]?.rest ?? "" }}</span
                      >
                    </span>
                    <!-- Auth status badge -->
                    <span
                      v-if="c.proxy_managed"
                      class="rounded-full border border-violet-200 bg-violet-50 px-1.5 py-0.5 text-[10px] font-medium text-violet-700"
                    >
                      {{ t("badges.vibeManaged") }}
                    </span>
                    <span
                      v-else
                      :class="
                        c.token_ok
                          ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                          : 'border-amber-200 bg-amber-50 text-amber-700'
                      "
                      class="rounded-full border px-1.5 py-0.5 text-[10px] font-medium"
                    >
                      {{ c.token_ok ? t("badges.tokenOk") : t("badges.tokenMissing") }}
                    </span>
                    <!-- Already imported badge -->
                    <span
                      v-if="isAlreadyImported(c)"
                      class="rounded-full border border-slate-200 bg-slate-100 px-1.5 py-0.5 text-[10px] font-medium text-slate-500"
                    >
                      {{ t("badges.imported") }}
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
                      {{ t("badges.extraAccounts", { count: c.extra_credentials.length }) }}
                    </span>
                  </div>
                </div>

                <button
                  v-if="!isAlreadyImported(c)"
                  type="button"
                  class="shrink-0 rounded-lg bg-violet-600 p-2.5 text-white transition-colors hover:bg-violet-700 disabled:opacity-50"
                  :disabled="importingSet.has(c.client) || importAllBusy"
                  :title="importingSet.has(c.client) ? t('actions.importing') : t('actions.import')"
                  @click="importOne(c.client)"
                >
                  <VpIcon
                    :name="importingSet.has(c.client) ? 'loader-2' : 'download'"
                    size-class="size-4"
                    :spin="importingSet.has(c.client)"
                  />
                </button>
                <span
                  v-else
                  class="shrink-0 grid size-9 place-items-center rounded-lg bg-slate-100 text-slate-400"
                  :title="t('badges.imported')"
                >
                  <VpIcon name="check" size-class="size-4" />
                </span>
              </div>
            </div>
          </template>
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
            {{ t("actions.cancel") }}
          </button>
          <button
            v-if="pendingCount > 0"
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
            {{ t("actions.importAll", { count: pendingCount }) }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<i18n lang="json">
{
  "en": {
    "actions": {
      "cancel": "Cancel",
      "close": "Close",
      "import": "Import",
      "importAll": "Import all ({count})",
      "importing": "Importing…",
      "retry": "Retry"
    },
    "badges": {
      "extraAccounts": "+{count} extra account(s)",
      "imported": "Imported",
      "tokenMissing": "Token missing",
      "tokenOk": "Token OK",
      "vibeManaged": "Vibe managed"
    },
    "empty": {
      "description": "Supports Codex CLI (~/.codex), Claude (~/.claude), and more",
      "title": "No importable local tools found"
    },
    "filters": {
      "all": "All",
      "messages": "Messages",
      "responses": "Responses"
    },
    "scan": {
      "failed": "Scan failed",
      "scanning": "Scanning local tools…"
    },
    "title": "Local import"
  },
  "zh-CN": {
    "actions": {
      "cancel": "取消",
      "close": "关闭",
      "import": "导入",
      "importAll": "全部导入（{count}）",
      "importing": "导入中…",
      "retry": "重试"
    },
    "badges": {
      "extraAccounts": "+{count} 个额外账户",
      "imported": "已导入",
      "tokenMissing": "缺少 Token",
      "tokenOk": "Token 正常",
      "vibeManaged": "Vibe 管理"
    },
    "empty": {
      "description": "支持 Codex CLI（~/.codex）、Claude（~/.claude）等本地工具",
      "title": "未发现可导入的本地工具"
    },
    "filters": {
      "all": "全部",
      "messages": "消息",
      "responses": "响应"
    },
    "scan": {
      "failed": "扫描失败",
      "scanning": "正在扫描本地工具…"
    },
    "title": "本地导入"
  }
}
</i18n>

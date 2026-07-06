<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { api, type Credential, type Provider } from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import UiButton from "../../../components/ui/button.vue";
import { formatApiError } from "../../../utils/api-error.ts";
import {
  buildProviderClipboardBundle,
  bundleSummary,
  clipboardCredentialToInput,
  parseProviderClipboardBundle,
  planProviderClipboardImport,
  providerDisplayKey,
  serializeProviderClipboardBundle,
  type ProviderClipboardBundle,
  type ProviderClipboardImportPlan,
} from "../utils/provider-clipboard-bundle.ts";

const props = defineProps<{
  open: boolean;
  mode: "export" | "import";
  providers: Provider[];
  credsByProvider: Record<string, Credential[]>;
}>();

const emit = defineEmits<{
  close: [];
  imported: [];
}>();

const { t } = useI18n();

const parseError = ref("");
const pasteText = ref("");
const parsedBundle = ref<ProviderClipboardBundle | null>(null);
const importPlan = ref<ProviderClipboardImportPlan | null>(null);
const exportBundle = ref<ProviderClipboardBundle | null>(null);
const busy = ref(false);
const feedback = ref("");
const feedbackOk = ref(false);

const exportSummary = computed(() =>
  exportBundle.value ? bundleSummary(exportBundle.value) : null,
);

const importSummary = computed(() =>
  parsedBundle.value ? bundleSummary(parsedBundle.value) : null,
);

watch(
  () => [props.open, props.mode] as const,
  ([open, mode]) => {
    if (!open) return;
    parseError.value = "";
    pasteText.value = "";
    parsedBundle.value = null;
    importPlan.value = null;
    exportBundle.value = null;
    feedback.value = "";
    feedbackOk.value = false;
    busy.value = false;

    if (mode === "export") {
      exportBundle.value = buildProviderClipboardBundle(props.providers, props.credsByProvider);
      return;
    }
    void readClipboardForImport();
  },
);

async function readClipboardForImport() {
  try {
    const text = await navigator.clipboard.readText();
    if (!text.trim()) {
      parseError.value = t("errors.clipboardEmpty");
      return;
    }
    pasteText.value = text;
    applyPaste(text);
  } catch {
    parseError.value = t("errors.clipboardReadFailed");
  }
}

function applyPaste(text: string) {
  parseError.value = "";
  parsedBundle.value = null;
  importPlan.value = null;
  try {
    const bundle = parseProviderClipboardBundle(text);
    parsedBundle.value = bundle;
    importPlan.value = planProviderClipboardImport(bundle, props.providers, props.credsByProvider);
  } catch (e) {
    const code = e instanceof Error ? e.message : "";
    if (code === "clipboard_empty") parseError.value = t("errors.clipboardEmpty");
    else if (code === "invalid_json") parseError.value = t("errors.invalidJson");
    else if (code === "invalid_bundle") parseError.value = t("errors.invalidBundle");
    else parseError.value = formatApiError(e);
  }
}

function onPasteInput() {
  if (!pasteText.value.trim()) {
    parsedBundle.value = null;
    importPlan.value = null;
    parseError.value = "";
    return;
  }
  applyPaste(pasteText.value);
}

async function copyExportBundle() {
  if (!exportBundle.value) return;
  busy.value = true;
  feedback.value = "";
  try {
    const text = serializeProviderClipboardBundle(exportBundle.value);
    await navigator.clipboard.writeText(text);
    feedbackOk.value = true;
    feedback.value = t("export.copied");
  } catch {
    feedbackOk.value = false;
    feedback.value = t("export.copyFailed");
  } finally {
    busy.value = false;
  }
}

async function runImport() {
  if (!importPlan.value) return;
  busy.value = true;
  feedback.value = "";
  parseError.value = "";
  try {
    for (const item of importPlan.value.items) {
      let providerId = item.existingProviderId;
      if (item.action === "create") {
        const created = await api.providers.create(item.entry.provider);
        providerId = created.id;
      }
      if (!providerId) continue;

      for (const credPlan of item.credentials) {
        if (credPlan.action !== "create") continue;
        await api.credentials.create(providerId, clipboardCredentialToInput(credPlan.credential));
      }
    }
    feedbackOk.value = true;
    feedback.value = t("import.done", {
      providers: importPlan.value.totals.providersToCreate,
      credentials: importPlan.value.totals.credentialsToCreate,
    });
    emit("imported");
  } catch (e) {
    feedbackOk.value = false;
    parseError.value = formatApiError(e);
  } finally {
    busy.value = false;
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
      :aria-labelledby="
        mode === 'export' ? 'provider-export-title' : 'provider-import-clipboard-title'
      "
      @click.self="emit('close')"
    >
      <div
        class="vp-modal-panel flex max-h-[min(90dvh,40rem)] w-[min(100vw-1rem,36rem)] flex-col"
        @click.stop
      >
        <div class="vp-modal-header border-b border-vp-border/70">
          <span
            class="grid size-9 shrink-0 place-items-center rounded-xl ring-1"
            :class="
              mode === 'export'
                ? 'bg-violet-100 text-violet-700 ring-violet-200'
                : 'bg-cyan-100 text-cyan-700 ring-cyan-200'
            "
          >
            <VpIcon :name="mode === 'export' ? 'copy' : 'clipboard'" size-class="size-4.5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2
              :id="mode === 'export' ? 'provider-export-title' : 'provider-import-clipboard-title'"
              class="text-base font-semibold text-vp-text"
            >
              {{ mode === "export" ? t("export.title") : t("import.title") }}
            </h2>
            <p class="mt-0.5 text-xs text-muted-foreground">
              {{ mode === "export" ? t("export.subtitle") : t("import.subtitle") }}
            </p>
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

        <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <template v-if="mode === 'export'">
            <div
              v-if="exportSummary"
              class="rounded-2xl border border-vp-border bg-slate-50/80 px-4 py-3 text-sm"
            >
              <p class="font-medium text-vp-text">
                {{
                  t("export.summary", {
                    providers: exportSummary.providerCount,
                    credentials: exportSummary.credentialCount,
                  })
                }}
              </p>
              <p v-if="exportSummary.secretCount > 0" class="mt-2 text-xs text-amber-800">
                {{ t("export.secretWarning", { count: exportSummary.secretCount }) }}
              </p>
            </div>

            <p class="text-xs leading-relaxed text-muted-foreground">
              {{ t("export.hint") }}
            </p>

            <div
              v-if="feedback"
              class="rounded-xl border px-3 py-2 text-sm"
              :class="
                feedbackOk
                  ? 'border-emerald-200 bg-emerald-50 text-emerald-800'
                  : 'border-red-200 bg-red-50 text-red-700'
              "
            >
              {{ feedback }}
            </div>
          </template>

          <template v-else>
            <div class="flex flex-wrap gap-2">
              <UiButton
                type="button"
                variant="outline"
                size="sm"
                class="min-h-9"
                :disabled="busy"
                @click="readClipboardForImport"
              >
                <VpIcon name="clipboard" size-class="size-4 shrink-0" />
                {{ t("import.readClipboard") }}
              </UiButton>
            </div>

            <label class="block text-xs font-medium text-muted-foreground">
              {{ t("import.pasteLabel") }}
              <textarea
                v-model="pasteText"
                class="mt-1.5 min-h-[8rem] w-full resize-y rounded-xl border border-vp-border bg-white px-3 py-2 font-mono text-xs text-vp-text outline-none ring-violet-400 focus:ring-2"
                :placeholder="t('import.pastePlaceholder')"
                @input="onPasteInput"
              />
            </label>

            <div
              v-if="parseError"
              class="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
            >
              {{ parseError }}
            </div>

            <div
              v-if="importPlan && importSummary"
              class="space-y-2 rounded-2xl border px-4 py-3"
              :class="
                importPlan.totals.inSync
                  ? 'border-emerald-200 bg-emerald-50'
                  : 'border-vp-border bg-white'
              "
            >
              <p
                class="text-sm font-medium"
                :class="importPlan.totals.inSync ? 'text-emerald-900' : 'text-vp-text'"
              >
                {{
                  importPlan.totals.inSync
                    ? t("import.inSync")
                    : t("import.planSummary", {
                        createProviders: importPlan.totals.providersToCreate,
                        mergeProviders: importPlan.totals.providersToMerge,
                        createCredentials: importPlan.totals.credentialsToCreate,
                        skipCredentials: importPlan.totals.credentialsToSkip,
                      })
                }}
              </p>
              <p v-if="importPlan.totals.inSync" class="text-xs leading-relaxed text-emerald-800">
                {{ t("import.inSyncHint") }}
              </p>
              <ul class="max-h-40 space-y-1 overflow-y-auto text-xs text-muted-foreground">
                <li
                  v-for="(item, index) in importPlan.items"
                  :key="`${providerDisplayKey(item.entry.provider)}-${index}`"
                  class="truncate"
                >
                  <span class="font-medium text-vp-text">{{ item.entry.provider.name }}</span>
                  <span class="text-muted-foreground">
                    · {{ providerDisplayKey(item.entry.provider) }}
                    ·
                    {{
                      item.action === "create" ? t("import.actionCreate") : t("import.actionMerge")
                    }}
                  </span>
                </li>
              </ul>
            </div>

            <div
              v-if="feedback"
              class="rounded-xl border px-3 py-2 text-sm"
              :class="
                feedbackOk
                  ? 'border-emerald-200 bg-emerald-50 text-emerald-800'
                  : 'border-red-200 bg-red-50 text-red-700'
              "
            >
              {{ feedback }}
            </div>
          </template>
        </div>

        <div class="flex flex-wrap justify-end gap-2 border-t border-vp-border/70 px-5 py-4">
          <UiButton type="button" variant="outline" class="min-h-10" @click="emit('close')">
            {{ t("actions.close") }}
          </UiButton>
          <UiButton
            v-if="mode === 'export'"
            type="button"
            class="min-h-10"
            :disabled="busy || !exportBundle || exportSummary?.providerCount === 0"
            @click="copyExportBundle"
          >
            <VpIcon name="copy" size-class="size-4 shrink-0 text-white" />
            {{ t("export.copy") }}
          </UiButton>
          <UiButton
            v-else
            type="button"
            class="min-h-10"
            :disabled="busy || !importPlan || importPlan.totals.inSync"
            @click="runImport"
          >
            <VpIcon
              v-if="busy"
              name="loader-2"
              size-class="size-4 shrink-0 animate-spin text-white"
            />
            <VpIcon v-else name="upload" size-class="size-4 shrink-0 text-white" />
            {{ t("import.confirm") }}
          </UiButton>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<i18n lang="json">
{
  "en": {
    "actions": { "close": "Close" },
    "export": {
      "title": "Export providers",
      "subtitle": "Copy provider config and credentials to the clipboard for another device.",
      "summary": "{providers} providers, {credentials} credentials",
      "secretWarning": "Includes {count} secret(s) (API keys / OAuth tokens). Clipboard data is sensitive — paste only on trusted devices.",
      "hint": "On the other device, open Providers → Import from clipboard and paste.",
      "copy": "Copy to clipboard",
      "copied": "Copied to clipboard.",
      "copyFailed": "Could not write to clipboard."
    },
    "import": {
      "title": "Import from clipboard",
      "subtitle": "Paste a bundle exported from another Vibe Plus device.",
      "readClipboard": "Read clipboard",
      "pasteLabel": "Bundle JSON",
      "pastePlaceholder": "Paste exported JSON here…",
      "planSummary": "New {createProviders} · merge {mergeProviders} · add {createCredentials} cred(s) · skip {skipCredentials}",
      "inSync": "Already in sync with this device",
      "inSyncHint": "All providers and credentials in the bundle are already present. Nothing to import.",
      "actionCreate": "new",
      "actionMerge": "merge",
      "confirm": "Import",
      "done": "Imported {providers} provider(s) and {credentials} credential(s)."
    },
    "errors": {
      "clipboardEmpty": "Clipboard is empty.",
      "clipboardReadFailed": "Could not read clipboard — paste into the text box instead.",
      "invalidJson": "Invalid JSON.",
      "invalidBundle": "Unrecognized Vibe Plus provider bundle (need schemaVersion 1)."
    }
  },
  "zh-CN": {
    "actions": { "close": "关闭" },
    "export": {
      "title": "导出供应商",
      "subtitle": "将供应商配置与凭证复制到剪贴板，便于在另一台设备导入。",
      "summary": "{providers} 个供应商，{credentials} 条凭证",
      "secretWarning": "包含 {count} 项敏感信息（API Key / OAuth Token）。剪贴板内容请仅在可信设备间传递。",
      "hint": "在另一台设备打开「供应商 → 从剪贴板导入」并粘贴即可。",
      "copy": "复制到剪贴板",
      "copied": "已复制到剪贴板。",
      "copyFailed": "无法写入剪贴板。"
    },
    "import": {
      "title": "从剪贴板导入",
      "subtitle": "粘贴从另一台 Vibe Plus 导出的 JSON 包。",
      "readClipboard": "读取剪贴板",
      "pasteLabel": "Bundle JSON",
      "pastePlaceholder": "在此粘贴导出的 JSON…",
      "planSummary": "新建 {createProviders} · 合并 {mergeProviders} · 新增凭证 {createCredentials} · 跳过 {skipCredentials}",
      "inSync": "已与当前设备完全一致",
      "inSyncHint": "剪贴板中的供应商和凭证都已存在，无需导入。",
      "actionCreate": "新建",
      "actionMerge": "合并",
      "confirm": "开始导入",
      "done": "已导入 {providers} 个供应商、{credentials} 条凭证。"
    },
    "errors": {
      "clipboardEmpty": "剪贴板为空。",
      "clipboardReadFailed": "无法读取剪贴板，请直接粘贴到文本框。",
      "invalidJson": "JSON 格式无效。",
      "invalidBundle": "无法识别的 Vibe Plus 供应商包（需要 schemaVersion 1）。"
    }
  }
}
</i18n>

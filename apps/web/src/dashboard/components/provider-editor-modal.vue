<script setup lang="ts">
import { computed } from "vue";
import type {
  Credential,
  CredentialPoolStatus,
  ModelAlias,
  Provider,
  ProviderInput,
  ProviderKind,
} from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import ProviderLogo from "./provider-logo.vue";
import { credentialPrimaryAccountLabel } from "../utils/providers-display.ts";

const props = defineProps<{
  open: boolean;
  editTarget: Provider | null;
  providerLive: Provider | null;
  form: ProviderInput;
  providerKinds: ProviderKind[];
  creds: Credential[];
  loadingCreds: boolean;
  aliasBulkPaste: string;
  aliasBulkErr: string;
  providerFormImportPaste: string;
  providerFormImportErr: string;
  credToggleBusy: Record<string, boolean>;
  modelRefreshBusy: boolean;
  speedLabel: string;
  syncOpen: boolean;
  protocolSyncBusy: boolean;
  syncPreview: import("../api/client.ts").ProviderSyncPreview | null;
}>();

const emit = defineEmits<{
  close: [];
  save: [];
  refreshModels: [];
  addAliasRow: [];
  removeAliasRow: [index: number];
  pasteAliasBulk: [];
  parseAliasBulk: [];
  addCredential: [];
  reloadCreds: [];
  editCredential: [cred: Credential];
  removeCredential: [cred: Credential];
  toggleCredential: [cred: Credential];
  pasteProviderJson: [];
  applyProviderJson: [];
  /** Providers 页「同步」下拉由父级处理，此处仅声明以免 fragment 根节点下的 fallthrough 告警 */
  toggleSyncMenu: [];
  syncAll: [];
  syncBrand: [];
  syncProtocol: [];
  syncModels: [];
  syncUsage: [];
  "update:aliasBulkPaste": [value: string];
  "update:providerFormImportPaste": [value: string];
}>();

const modelCount = computed(() => props.providerLive?.remote_models?.length ?? 0);
const title = computed(() => (props.editTarget ? "Edit provider" : "Create provider"));
const capabilityRows = computed(() => [
  {
    key: "openai-responses",
    label: "OpenAI Responses",
    active:
      props.syncPreview?.supported_protocols?.includes("openai-responses") ??
      props.form.kind === "openai-responses",
  },
  {
    key: "openai-chat",
    label: "OpenAI Chat",
    active:
      props.syncPreview?.supported_protocols?.includes("openai-chat") ??
      props.form.kind === "openai-chat",
  },
  {
    key: "anthropic",
    label: "Anthropic",
    active:
      props.syncPreview?.supported_protocols?.includes("anthropic") ??
      props.form.kind === "anthropic",
  },
  {
    key: "gemini-native",
    label: "Gemini Native",
    active:
      props.syncPreview?.supported_protocols?.includes("gemini-native") ??
      props.form.kind === "gemini-native",
  },
]);

function providerKindLabel(kind: ProviderKind): string {
  switch (kind) {
    case "openai-responses":
      return "OPENAI RESPONSES";
    case "openai-chat":
      return "OPENAI CHAT";
    case "anthropic":
      return "ANTHROPIC";
    case "gemini-native":
      return "GEMINI";
    default:
      return kind;
  }
}

function summarizeAuthRefHint(ref: string | null): string {
  if (!ref) return "—";
  if (ref.startsWith("literal:")) return "literal:•••";
  return ref;
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="vp-modal-backdrop z-[110] overflow-y-auto py-6"
      role="dialog"
      aria-modal="true"
      aria-labelledby="provider-form-title"
      @click.self="emit('close')"
    >
      <div
        class="vp-modal-panel my-auto flex max-h-[min(92dvh,58rem)] w-[min(100vw-1rem,72rem)] flex-col"
        @click.stop
      >
        <div class="vp-modal-header border-b border-vp-border/70">
          <div class="flex min-w-0 flex-1 items-start gap-3">
            <ProviderLogo
              :kind="form.kind"
              :avatar-url="form.avatar_url ?? providerLive?.avatar_url ?? null"
              :provider-name="form.name || providerLive?.name || 'provider'"
              size-class="size-14"
              icon-size-class="size-7"
            />
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <h2 id="provider-form-title" class="truncate text-lg font-semibold text-vp-text">
                  {{ title }}
                </h2>
                <span
                  class="rounded-full border border-vp-border bg-white px-2 py-0.5 text-[10px] font-mono text-vp-muted"
                >
                  {{ providerKindLabel(form.kind) }}
                </span>
              </div>
              <p class="mt-1 truncate text-base font-semibold text-slate-900">
                {{ form.name || providerLive?.name || "Untitled provider" }}
              </p>
              <p class="mt-1 break-all font-mono text-[11px] text-vp-muted">
                {{ form.base_url || providerLive?.base_url || "No base URL" }}
              </p>
            </div>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            aria-label="close"
            title="close"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>

        <div
          class="grid flex-1 gap-4 overflow-y-auto px-4 py-4 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)] sm:px-6"
        >
          <section class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-4">
              <div
                class="rounded-2xl border border-vp-border bg-[color-mix(in_srgb,var(--vp-primary)_4%,white)] px-3 py-3"
              >
                <div class="text-[10px] uppercase tracking-wide text-vp-muted">Avatar</div>
                <div class="mt-1 text-sm font-medium text-slate-900">
                  {{ (form.avatar_url ?? providerLive?.avatar_url) ? "Configured" : "Missing" }}
                </div>
              </div>
              <div class="rounded-2xl border border-vp-border bg-white px-3 py-3">
                <div class="text-[10px] uppercase tracking-wide text-vp-muted">Models</div>
                <div class="mt-1 text-sm font-medium text-slate-900">{{ modelCount }}</div>
              </div>
              <div class="rounded-2xl border border-vp-border bg-white px-3 py-3">
                <div class="text-[10px] uppercase tracking-wide text-vp-muted">Aliases</div>
                <div class="mt-1 text-sm font-medium text-slate-900">
                  {{ form.model_aliases.length }}
                </div>
              </div>
              <div class="rounded-2xl border border-vp-border bg-white px-3 py-3">
                <div class="text-[10px] uppercase tracking-wide text-vp-muted">Speed</div>
                <div class="mt-1 text-sm font-medium text-slate-900">{{ speedLabel }}</div>
              </div>
            </div>

            <section class="rounded-2xl border border-vp-border bg-white p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Brand & endpoint</h3>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-900 hover:bg-emerald-100"
                  :disabled="modelRefreshBusy"
                  @click="emit('refreshModels')"
                >
                  <VpIcon name="refresh-cw" size-class="size-3.5" :spin="modelRefreshBusy" />
                  Refresh metadata
                </button>
              </div>
              <div class="grid gap-3 sm:grid-cols-2">
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">Name</span>
                  <input
                    v-model="form.name"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm"
                  />
                </label>
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">Group</span>
                  <input
                    v-model="form.group_name"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm"
                    placeholder="e.g. official / maintainer A"
                  />
                </label>
                <label class="sm:col-span-2">
                  <span class="mb-1 block text-xs font-medium text-slate-600">Avatar URL</span>
                  <input
                    v-model="form.avatar_url"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm"
                    placeholder="https://example.com/logo.png"
                  />
                </label>
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">Protocol</span>
                  <select
                    v-model="form.kind"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm"
                  >
                    <option v-for="kind in providerKinds" :key="kind" :value="kind">
                      {{ kind }}
                    </option>
                  </select>
                </label>
                <label>
                  <span class="mb-1 block text-xs font-medium text-slate-600">Priority</span>
                  <input
                    v-model.number="form.priority"
                    type="number"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm"
                  />
                </label>
                <label class="sm:col-span-2">
                  <span class="mb-1 block text-xs font-medium text-slate-600">Base URL</span>
                  <input
                    v-model="form.base_url"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm font-mono"
                  />
                </label>
                <label class="sm:col-span-2">
                  <span class="mb-1 block text-xs font-medium text-slate-600"
                    >Default auth_ref</span
                  >
                  <input
                    v-model="form.auth_ref"
                    class="w-full rounded-md border border-slate-300 px-3 py-2 text-sm font-mono"
                    placeholder="env:MY_API_KEY / keyring:name / literal:..."
                  />
                </label>
                <label
                  class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.enabled" type="checkbox" />
                  <span>Enable this provider</span>
                </label>
                <label
                  class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm"
                >
                  <input v-model="form.passthrough_mode" type="checkbox" />
                  <span>Pass through model names by default</span>
                </label>
              </div>
            </section>

            <section class="rounded-2xl border border-vp-border bg-white p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Model aliases</h3>
                <div class="flex flex-wrap gap-2">
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-900 hover:bg-emerald-100"
                    :disabled="modelRefreshBusy"
                    @click="emit('refreshModels')"
                  >
                    <VpIcon name="book-open" size-class="size-3.5" :spin="modelRefreshBusy" />
                    Fetch remote models
                  </button>
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-md border border-slate-200 bg-white px-2.5 py-1 text-xs text-slate-700 hover:bg-slate-50"
                    @click="emit('addAliasRow')"
                  >
                    <VpIcon name="plus" size-class="size-3.5" />
                    Add row
                  </button>
                </div>
              </div>
              <div class="space-y-2">
                <div
                  v-if="!form.model_aliases.length"
                  class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
                >
                  No aliases yet. Keep passthrough on unless upstream model IDs differ.
                </div>
                <div
                  v-for="(alias, index) in form.model_aliases"
                  :key="`${index}-${alias.alias}-${alias.upstream_model}`"
                  class="grid gap-2 rounded-xl border border-slate-200 bg-slate-50 p-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
                >
                  <input
                    v-model="alias.alias"
                    class="rounded-md border border-slate-300 px-3 py-2 text-sm"
                    placeholder="Client alias"
                  />
                  <input
                    v-model="alias.upstream_model"
                    class="rounded-md border border-slate-300 px-3 py-2 text-sm font-mono"
                    placeholder="Upstream model ID"
                  />
                  <button
                    type="button"
                    class="rounded-md border border-red-200 px-3 py-2 text-xs text-red-700 hover:bg-red-50"
                    @click="emit('removeAliasRow', index)"
                  >
                    Delete
                  </button>
                </div>
                <div class="rounded-xl border border-violet-200 bg-violet-50/50 p-3">
                  <div class="mb-2 flex items-center justify-between gap-2">
                    <span class="text-xs font-medium text-slate-700">Bulk paste aliases</span>
                    <div class="flex gap-2">
                      <button
                        type="button"
                        class="rounded-md border border-slate-200 bg-white px-2 py-1 text-[11px] text-slate-700 hover:bg-slate-50"
                        @click="emit('pasteAliasBulk')"
                      >
                        Read from clipboard
                      </button>
                      <button
                        type="button"
                        :disabled="!aliasBulkPaste.trim()"
                        class="rounded-md bg-violet-600 px-2 py-1 text-[11px] text-white hover:bg-violet-700 disabled:opacity-40"
                        @click="emit('parseAliasBulk')"
                      >
                        Parse and append
                      </button>
                    </div>
                  </div>
                  <textarea
                    :value="aliasBulkPaste"
                    rows="3"
                    class="w-full resize-y rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-[11px] text-slate-900"
                    @input="
                      $emit('update:aliasBulkPaste', ($event.target as HTMLTextAreaElement).value)
                    "
                  />
                  <p v-if="aliasBulkErr" class="mt-1 text-[11px] text-red-600">
                    {{ aliasBulkErr }}
                  </p>
                </div>
              </div>
            </section>
          </section>

          <section class="space-y-4">
            <section class="rounded-2xl border border-vp-border bg-white p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Protocol capabilities</h3>
                <span class="text-[11px] text-slate-500">{{
                  syncPreview?.platform_guess ?? "Multi-protocol ready"
                }}</span>
              </div>
              <div class="grid gap-2">
                <div
                  v-for="row in capabilityRows"
                  :key="row.key"
                  class="flex items-center justify-between rounded-xl border px-3 py-2"
                  :class="
                    row.active ? 'border-emerald-200 bg-emerald-50' : 'border-slate-200 bg-slate-50'
                  "
                >
                  <div>
                    <div class="text-sm font-medium text-slate-900">{{ row.label }}</div>
                    <div class="text-[11px] text-slate-500">
                      {{ row.active ? "detected / active" : "not detected yet" }}
                    </div>
                  </div>
                  <span
                    class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                    :class="
                      row.active ? 'bg-emerald-100 text-emerald-800' : 'bg-slate-200 text-slate-600'
                    "
                    >{{ row.active ? "active" : "standby" }}</span
                  >
                </div>
              </div>
            </section>

            <section class="rounded-2xl border border-vp-border bg-white p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Remote inventory</h3>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-900 hover:bg-emerald-100"
                  :disabled="modelRefreshBusy"
                  @click="emit('refreshModels')"
                >
                  <VpIcon name="refresh-cw" size-class="size-3.5" :spin="modelRefreshBusy" />
                  Refresh models
                </button>
              </div>
              <div
                v-if="providerLive?.remote_models?.length"
                class="flex max-h-48 flex-wrap gap-1.5 overflow-y-auto"
              >
                <span
                  v-for="model in providerLive.remote_models"
                  :key="model"
                  class="rounded-full border border-slate-200 bg-slate-50 px-2 py-1 text-[11px] font-mono text-slate-800"
                  >{{ model }}</span
                >
              </div>
              <div
                v-else
                class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
              >
                No remote models cached yet.
              </div>
            </section>

            <section class="rounded-2xl border border-vp-border bg-white p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Credentials</h3>
                <div class="flex gap-2">
                  <button
                    type="button"
                    class="rounded-md border border-slate-200 bg-white px-2.5 py-1 text-xs text-slate-700 hover:bg-slate-50 disabled:opacity-50"
                    :disabled="loadingCreds"
                    @click="emit('reloadCreds')"
                  >
                    Refresh
                  </button>
                  <button
                    type="button"
                    class="rounded-md bg-teal-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-teal-700"
                    @click="emit('addCredential')"
                  >
                    Add credential
                  </button>
                </div>
              </div>
              <div
                v-if="!editTarget"
                class="rounded-xl border border-amber-200 bg-amber-50/60 px-3 py-3 text-xs text-amber-950"
              >
                Save the provider first, then configure multiple credentials here.
              </div>
              <div v-else-if="loadingCreds" class="py-4 text-center text-xs text-slate-500">
                Loading credentials…
              </div>
              <div
                v-else-if="!creds.length"
                class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-3 text-xs text-slate-500"
              >
                No credentials yet.
              </div>
              <ul v-else class="space-y-2">
                <li
                  v-for="cred in creds"
                  :key="cred.id"
                  class="flex flex-col gap-2 rounded-xl border border-slate-200 bg-slate-50 p-3"
                >
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="truncate text-sm font-medium text-slate-900">{{
                      credentialPrimaryAccountLabel(cred)
                    }}</span>
                    <span
                      v-if="cred.oauth_access_token || cred.oauth_has_refresh"
                      class="rounded bg-violet-100 px-1.5 py-0.5 text-[10px] text-violet-800"
                      >OAuth</span
                    >
                    <span v-else class="font-mono text-[10px] text-slate-500">{{
                      summarizeAuthRefHint(cred.auth_ref)
                    }}</span>
                  </div>
                  <div class="flex flex-wrap gap-1.5">
                    <button
                      type="button"
                      class="rounded-md border border-slate-200 px-2 py-1 text-[11px] text-slate-700 hover:bg-slate-50 disabled:opacity-50"
                      :disabled="!!credToggleBusy[cred.id]"
                      @click="emit('toggleCredential', cred)"
                    >
                      {{ cred.enabled ? "Disable" : "Enable" }}
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-slate-200 px-2 py-1 text-[11px] text-slate-700 hover:bg-slate-50"
                      @click="emit('editCredential', cred)"
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      class="rounded-md border border-red-200 px-2 py-1 text-[11px] text-red-700 hover:bg-red-50"
                      @click="emit('removeCredential', cred)"
                    >
                      Delete
                    </button>
                  </div>
                </li>
              </ul>
            </section>

            <section class="rounded-2xl border border-violet-200 bg-violet-50/50 p-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-slate-900">Import fields from JSON</h3>
                <div class="flex gap-2">
                  <button
                    type="button"
                    class="rounded-md border border-slate-200 bg-white px-2 py-1 text-[11px] text-slate-700 hover:bg-slate-50"
                    @click="emit('pasteProviderJson')"
                  >
                    Read from clipboard
                  </button>
                  <button
                    type="button"
                    :disabled="!providerFormImportPaste.trim()"
                    class="rounded-md bg-violet-600 px-2 py-1 text-[11px] text-white hover:bg-violet-700 disabled:opacity-40"
                    @click="emit('applyProviderJson')"
                  >
                    Parse text below
                  </button>
                </div>
              </div>
              <textarea
                :value="providerFormImportPaste"
                rows="5"
                class="w-full resize-y rounded-md border border-violet-200 bg-white px-2 py-1.5 font-mono text-[11px] text-slate-900"
                @input="
                  $emit(
                    'update:providerFormImportPaste',
                    ($event.target as HTMLTextAreaElement).value,
                  )
                "
              />
              <p v-if="providerFormImportErr" class="mt-1 text-[11px] text-red-600">
                {{ providerFormImportErr }}
              </p>
            </section>
          </section>
        </div>

        <div
          class="flex flex-wrap gap-2 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-4 py-3 sm:justify-end sm:px-6 sm:py-4"
        >
          <button
            type="button"
            class="btn-ghost inline-flex flex-1 items-center justify-center gap-2 px-4 py-2 sm:flex-none"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-4" />
            Cancel
          </button>
          <button
            type="button"
            class="inline-flex flex-1 items-center justify-center gap-2 rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-700 sm:flex-none"
            @click="emit('save')"
          >
            <VpIcon name="check" size-class="size-4 text-white" />
            Save
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

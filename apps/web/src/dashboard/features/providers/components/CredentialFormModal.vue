<script setup lang="ts">
import { useI18n } from "vue-i18n";
import type { ComponentPublicInstance } from "vue";
import type {
  Credential,
  CredentialInput,
  Provider,
  UpstreamGroupInfo,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  editCred: Credential | null;
  editTarget: Provider | null;
  credForm: CredentialInput;
  credAuthMode: "apikey" | "oauth";
  authJsonPaste: string;
  authJsonPasteErr: string;
  authJsonDragActive: boolean;
  authJsonFileInputRef: HTMLInputElement | null;
  credLoginPassword: string;
  credLoginBusy: boolean;
  credLoginNote: string | null;
  credGroups: UpstreamGroupInfo[];
  credGroupsBusy: boolean;
}>();

const emit = defineEmits<{
  close: [];
  save: [];
  "update:credForm": [CredentialInput];
  "update:credAuthMode": ["apikey" | "oauth"];
  "update:authJsonPaste": [string];
  "update:authJsonFileInputRef": [HTMLInputElement | null];
  "update:credLoginPassword": [string];
  parseAuthJsonPaste: [];
  triggerAuthJsonFilePick: [];
  authJsonFileChange: [Event];
  authJsonDragOver: [DragEvent];
  authJsonDragLeave: [DragEvent];
  authJsonDrop: [DragEvent];
  refreshProviderModels: [providerId: string];
  doCredLogin: [];
  fetchCredGroups: [];
}>();

function patchCredForm(patch: Partial<CredentialInput>) {
  emit("update:credForm", { ...props.credForm, ...patch });
}

function setAuthMode(mode: "apikey" | "oauth") {
  emit("update:credAuthMode", mode);
}

function setFileInput(el: Element | ComponentPublicInstance | null) {
  emit("update:authJsonFileInputRef", el instanceof HTMLInputElement ? el : null);
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="vp-modal-backdrop z-[110] overflow-y-auto py-6"
      role="dialog"
      aria-modal="true"
      aria-labelledby="cred-form-title"
      @click.self="emit('close')"
    >
      <div class="vp-modal-panel max-w-lg flex flex-col my-auto" @click.stop>
        <div class="vp-modal-header">
          <span
            class="grid size-10 shrink-0 place-items-center rounded-xl bg-violet-100 text-violet-700 ring-1 ring-violet-200"
            aria-hidden="true"
          >
            <VpIcon name="key" size-class="size-5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="cred-form-title" class="font-semibold text-lg text-vp-text">
              {{ editCred ? t("title.edit") : t("title.add") }}
            </h2>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            :aria-label="t('actions.close')"
            :title="t('actions.close')"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-5" />
          </button>
        </div>
        <div class="px-6 py-4 space-y-3 overflow-y-auto max-h-[min(36rem,72vh)]">
          <label class="block">
            <span class="sr-only">{{ t("fields.label") }}</span>
            <input
              :value="credForm.label"
              :placeholder="t('fields.label')"
              class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900 focus:outline-none focus:border-violet-500"
              @input="patchCredForm({ label: ($event.target as HTMLInputElement).value })"
            />
          </label>

          <div class="flex gap-2">
            <button
              type="button"
              :class="
                credAuthMode === 'apikey'
                  ? 'bg-violet-600 text-white'
                  : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
              "
              class="flex-1 py-1.5 text-xs rounded-md transition-colors"
              @click="setAuthMode('apikey')"
            >
              auth_ref
            </button>
            <button
              type="button"
              :class="
                credAuthMode === 'oauth'
                  ? 'bg-violet-600 text-white'
                  : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
              "
              class="flex-1 py-1.5 text-xs rounded-md transition-colors"
              @click="setAuthMode('oauth')"
            >
              OAuth
            </button>
          </div>

          <template v-if="credAuthMode === 'apikey'">
            <label class="block">
              <span class="sr-only">auth_ref</span>
              <input
                :value="credForm.auth_ref ?? ''"
                :placeholder="t('auth.placeholder')"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm font-mono text-slate-900"
                @input="patchCredForm({ auth_ref: ($event.target as HTMLInputElement).value })"
              />
              <p class="mt-1 text-[11px] text-vp-muted font-mono">
                {{ t("auth.literalBefore") }} <code>literal:</code> {{ t("auth.literalAfter") }}
              </p>
            </label>
          </template>

          <template v-else>
            <input
              :ref="setFileInput"
              type="file"
              accept=".json,application/json"
              class="sr-only"
              @change="emit('authJsonFileChange', $event)"
            />
            <div
              class="rounded-lg border border-dashed border-violet-200 p-3 space-y-2 bg-violet-50/80 transition-colors"
              :class="
                authJsonDragActive
                  ? 'border-violet-500 ring-2 ring-violet-400/50 bg-violet-100'
                  : 'border-violet-200'
              "
              @dragover="emit('authJsonDragOver', $event)"
              @dragleave="emit('authJsonDragLeave', $event)"
              @drop="emit('authJsonDrop', $event)"
            >
              <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-2">
                <p class="text-xs text-slate-800 font-medium">
                  <code class="font-mono text-slate-600">auth*.json</code>
                </p>
                <button
                  type="button"
                  class="shrink-0 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-white border border-slate-200 hover:bg-slate-50 text-slate-800 transition-colors w-full sm:w-auto"
                  :aria-label="t('actions.pickFile')"
                  :title="t('actions.pickFile')"
                  @click="emit('triggerAuthJsonFilePick')"
                >
                  <VpIcon name="folder-input" size-class="size-4" />
                  <span class="sr-only">{{ t("actions.pickFile") }}</span>
                </button>
              </div>
              <textarea
                :value="authJsonPaste"
                rows="5"
                placeholder='{"auth_mode":"chatgpt","tokens":{"access_token":"eyJ…","refresh_token":"…"}}'
                class="w-full min-h-[7rem] bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900 resize-y"
                @input="emit('update:authJsonPaste', ($event.target as HTMLTextAreaElement).value)"
              />
              <p v-if="authJsonPasteErr" class="text-xs text-red-600">{{ authJsonPasteErr }}</p>
              <div class="flex flex-wrap gap-2">
                <button
                  v-if="editTarget"
                  type="button"
                  class="inline-flex items-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-900 hover:bg-emerald-100"
                  @click="emit('refreshProviderModels', editTarget.id)"
                >
                  <VpIcon name="refresh-cw" size-class="size-3.5" />
                  {{ t("actions.fetchRemoteModels") }}
                </button>
                <button
                  type="button"
                  :disabled="!authJsonPaste.trim()"
                  class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-violet-600 hover:bg-violet-700 text-white disabled:opacity-40 transition-colors"
                  :aria-label="t('actions.parseJson')"
                  @click="emit('parseAuthJsonPaste')"
                >
                  <VpIcon name="zap" size-class="size-4 text-white" />
                  <span class="sr-only">{{ t("actions.parse") }}</span>
                </button>
              </div>
            </div>
            <label class="block">
              <span class="sr-only">access_token</span>
              <input
                :value="credForm.oauth_access_token ?? ''"
                placeholder="eyJhbGciOiJSUzI1NiJ9…"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                @input="
                  patchCredForm({ oauth_access_token: ($event.target as HTMLInputElement).value })
                "
              />
            </label>
            <label class="block">
              <span class="sr-only">refresh_token</span>
              <input
                :value="credForm.oauth_refresh_token ?? ''"
                placeholder="refresh_token"
                type="password"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-xs font-mono text-slate-900"
                @input="
                  patchCredForm({ oauth_refresh_token: ($event.target as HTMLInputElement).value })
                "
              />
            </label>
            <p class="font-mono text-xs text-slate-600">
              exp
              {{
                credForm.oauth_expires_at
                  ? new Date(credForm.oauth_expires_at * 1000).toLocaleString()
                  : t("states.unknown")
              }}
            </p>
          </template>

          <label class="block">
            <span class="sr-only">plan_type</span>
            <input
              :value="credForm.plan_type ?? ''"
              placeholder="claude-pro · codex-plus · codex-pro · payg · …"
              class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              @input="patchCredForm({ plan_type: ($event.target as HTMLInputElement).value })"
            />
          </label>

          <label class="block">
            <span class="text-xs text-slate-500 font-medium">{{ t("fields.vendorType") }}</span>
            <select
              :value="credForm.upstream_vendor ?? ''"
              class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              @change="
                patchCredForm({
                  upstream_vendor: (($event.target as HTMLSelectElement).value ||
                    null) as CredentialInput['upstream_vendor'],
                })
              "
            >
              <option value="">{{ t("vendor.generic") }}</option>
              <option value="new-api">NewAPI / One-API</option>
              <option value="sub2-api">Sub2API</option>
              <option value="anthropic-payg">{{ t("vendor.anthropicPayg") }}</option>
              <option value="anthropic-plan">{{ t("vendor.anthropicPlan") }}</option>
            </select>
          </label>

          <template
            v-if="credForm.upstream_vendor === 'new-api' || credForm.upstream_vendor === 'sub2-api'"
          >
            <label class="block">
              <span class="text-xs text-slate-500 font-medium">{{ t("fields.username") }}</span>
              <input
                :value="credForm.upstream_username ?? ''"
                placeholder="user@example.com"
                autocomplete="username"
                class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
                @input="
                  patchCredForm({ upstream_username: ($event.target as HTMLInputElement).value })
                "
              />
            </label>

            <template v-if="editCred">
              <div class="rounded-lg border border-slate-200 bg-slate-50 p-3 space-y-2">
                <p class="text-xs font-medium text-slate-600">{{ t("login.title") }}</p>
                <div class="flex gap-2">
                  <input
                    :value="credLoginPassword"
                    type="password"
                    :placeholder="t('fields.password')"
                    autocomplete="current-password"
                    class="flex-1 bg-white border border-slate-200 rounded-lg px-3 py-1.5 text-sm text-slate-900"
                    @input="
                      emit('update:credLoginPassword', ($event.target as HTMLInputElement).value)
                    "
                    @keydown.enter="emit('doCredLogin')"
                  />
                  <button
                    type="button"
                    :disabled="
                      credLoginBusy ||
                      !credForm.upstream_username?.trim() ||
                      !credLoginPassword.trim()
                    "
                    class="shrink-0 px-3 py-1.5 text-xs rounded-lg bg-violet-600 hover:bg-violet-700 text-white disabled:opacity-40 transition-colors"
                    @click="emit('doCredLogin')"
                  >
                    {{ credLoginBusy ? t("login.busy") : t("login.submit") }}
                  </button>
                </div>
                <p
                  v-if="credLoginNote"
                  :class="
                    credLoginNote === t('login.success') ? 'text-emerald-600' : 'text-red-600'
                  "
                  class="text-xs"
                >
                  {{ credLoginNote }}
                </p>
                <p v-if="editCred.upstream_has_session" class="text-xs text-slate-500">
                  {{ t("login.sessionCached") }}
                  <template v-if="editCred.upstream_session_expires_at">
                    · {{ t("login.expires") }}
                    {{ new Date(editCred.upstream_session_expires_at * 1000).toLocaleString() }}
                  </template>
                </p>
              </div>
            </template>

            <div class="space-y-1.5">
              <div class="flex items-center gap-2">
                <span class="text-xs text-slate-500 font-medium">{{ t("fields.group") }}</span>
                <button
                  v-if="editCred"
                  type="button"
                  :disabled="credGroupsBusy"
                  class="text-[10px] px-2 py-0.5 rounded border border-slate-200 bg-white hover:bg-slate-50 text-slate-600 disabled:opacity-40"
                  @click="emit('fetchCredGroups')"
                >
                  {{ credGroupsBusy ? t("groups.fetching") : t("groups.fetch") }}
                </button>
              </div>
              <select
                v-if="credGroups.length"
                :value="credForm.upstream_group ?? ''"
                class="w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
                @change="
                  patchCredForm({
                    upstream_group: ($event.target as HTMLSelectElement).value || null,
                  })
                "
              >
                <option value="">{{ t("groups.unspecified") }}</option>
                <option v-for="g in credGroups" :key="g.id" :value="g.name">
                  {{ g.name }}<template v-if="g.description"> · {{ g.description }}</template> (×{{
                    g.rate_multiplier
                  }})
                </option>
              </select>
              <input
                v-else
                :value="credForm.upstream_group ?? ''"
                :placeholder="t('groups.placeholder')"
                class="w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
                @input="
                  patchCredForm({ upstream_group: ($event.target as HTMLInputElement).value })
                "
              />
            </div>
          </template>

          <label class="block">
            <span class="text-xs text-slate-500 font-medium">{{
              t("fields.priceMultiplier")
            }}</span>
            <div class="mt-1 flex items-center gap-2">
              <input
                :value="credForm.price_multiplier ?? 1"
                type="number"
                step="0.01"
                min="0"
                placeholder="1.0"
                class="w-28 bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
                @input="
                  patchCredForm({
                    price_multiplier: Number(($event.target as HTMLInputElement).value),
                  })
                "
              />
              <span class="text-xs text-slate-400">{{ t("fields.priceMultiplierHint") }}</span>
            </div>
          </label>

          <label class="block">
            <span class="sr-only">priority</span>
            <input
              :value="credForm.priority"
              type="number"
              class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              @input="
                patchCredForm({ priority: Number(($event.target as HTMLInputElement).value) })
              "
            />
          </label>
          <label class="block">
            <span class="sr-only">notes</span>
            <input
              :value="credForm.notes ?? ''"
              :placeholder="t('fields.notes')"
              class="mt-1 w-full bg-white border border-slate-200 rounded-lg px-3 py-2 text-sm text-slate-900"
              @input="patchCredForm({ notes: ($event.target as HTMLInputElement).value })"
            />
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input
              :checked="credForm.enabled"
              type="checkbox"
              class="rounded"
              @change="patchCredForm({ enabled: ($event.target as HTMLInputElement).checked })"
            />
            <span>{{ t("fields.enabled") }}</span>
          </label>
        </div>
        <div
          class="flex gap-3 px-6 py-4 border-t border-vp-border justify-end bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))]"
        >
          <button
            type="button"
            class="btn-ghost flex items-center gap-2 !px-3"
            :aria-label="t('actions.cancel')"
            @click="emit('close')"
          >
            <VpIcon name="x" size-class="size-4" />
            <span class="sr-only">{{ t("actions.cancel") }}</span>
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-violet-600 hover:bg-violet-700 text-white font-medium transition-colors"
            :aria-label="t('actions.save')"
            @click="emit('save')"
          >
            <VpIcon name="check" size-class="size-4 text-white" />
            <span class="sr-only">{{ t("actions.save") }}</span>
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
      "cancel": "cancel",
      "close": "close",
      "fetchRemoteModels": "Fetch remote models",
      "parse": "parse",
      "parseJson": "parse JSON",
      "pickFile": "pick file",
      "save": "save"
    },
    "auth": {
      "literalAfter": "before storing in SQLite.",
      "literalBefore": "Raw sk-/ck-/dk-* values are automatically wrapped with",
      "placeholder": "sk-… paste directly (advanced: env:MY_KEY / keyring:name)"
    },
    "fields": {
      "group": "Group",
      "label": "label",
      "notes": "notes",
      "password": "Password",
      "priceMultiplier": "Cost multiplier",
      "priceMultiplierHint": "× official price (1.0 = 1:1)",
      "username": "Username / email",
      "vendorType": "Provider type",
      "enabled": "Enabled"
    },
    "groups": {
      "fetch": "Fetch groups",
      "fetching": "Fetching…",
      "placeholder": "Group name (leave empty for auto)",
      "unspecified": "— Unspecified"
    },
    "login": {
      "busy": "Logging in…",
      "expires": "expires",
      "sessionCached": "✓ Session cached",
      "submit": "Log in",
      "title": "Log in to fetch Session Token"
    },
    "title": {
      "add": "Add credential",
      "edit": "Edit credential"
    },
    "states": { "unknown": "unknown" },
    "vendor": {
      "anthropicPayg": "Anthropic official API Key (PAYG)",
      "anthropicPlan": "Anthropic official subscription (Pro / Max)",
      "generic": "— Generic"
    }
  },
  "zh-CN": {
    "actions": {
      "cancel": "取消",
      "close": "关闭",
      "fetchRemoteModels": "获取远程模型",
      "parse": "解析",
      "parseJson": "解析 JSON",
      "pickFile": "选择文件",
      "save": "保存"
    },
    "auth": {
      "literalAfter": "后再存入 SQLite。",
      "literalBefore": "原始 sk-/ck-/dk-* 会自动包装为",
      "placeholder": "直接粘贴 sk-…（高级：env:MY_KEY / keyring:name）"
    },
    "fields": {
      "group": "分组",
      "label": "标签",
      "notes": "备注",
      "password": "密码",
      "priceMultiplier": "成本倍率",
      "priceMultiplierHint": "× 官方价格（1.0 = 1:1）",
      "username": "用户名 / 邮箱",
      "vendorType": "供应商类型",
      "enabled": "启用"
    },
    "groups": {
      "fetch": "获取分组",
      "fetching": "获取中…",
      "placeholder": "分组名称（留空自动）",
      "unspecified": "— 不指定"
    },
    "login": {
      "busy": "登录中…",
      "expires": "到期",
      "sessionCached": "✓ Session 已缓存",
      "submit": "登录",
      "title": "登录获取 Session Token"
    },
    "title": {
      "add": "添加凭证",
      "edit": "编辑凭证"
    },
    "states": { "unknown": "未知" },
    "vendor": {
      "anthropicPayg": "Anthropic 官方 API Key（PAYG）",
      "anthropicPlan": "Anthropic 官方订阅（Pro / Max）",
      "generic": "— 通用（Generic）"
    }
  }
}
</i18n>

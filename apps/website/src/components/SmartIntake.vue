<script setup lang="ts">
import VpIcon from "./vp-icon.vue";
import type { SmartDragIntent, SmartIntakeItem } from "../composables/use-smart-intake.ts";

const props = defineProps<{
  items: SmartIntakeItem[];
  authCount: number;
  configCount: number;
  ccsProfileCount: number;
  dragActive: boolean;
  dragIntent: SmartDragIntent;
  busy: boolean;
  message: string | null;
  error: string | null;
  clipboardWatch: boolean;
  clipboardWatchAvailable: boolean;
}>();

const emit = defineEmits<{
  readClipboard: [];
  toggleClipboardWatch: [];
  importAuth: [];
  importCcsProfile: [];
  saveConfig: [];
  goCodex: [];
  dismiss: [];
}>();

function kindLabel(kind: SmartIntakeItem["kind"]): string {
  if (kind === "codex-auth") return "Auth";
  if (kind === "api-key") return "Key";
  if (kind === "codex-config") return "Config";
  if (kind === "ccswitch-provider") return "CC Switch";
  if (kind === "ccs-profile") return "Profile";
  return "?";
}

function dragLabel(intent: SmartDragIntent): string {
  if (intent === "text") return "Text";
  if (intent === "files") return "Files";
  if (intent === "folder") return "Folder";
  if (intent === "mixed") return "Mixed";
  return "Drop";
}
</script>

<template>
  <div class="fixed bottom-4 right-4 z-50 flex max-w-[calc(100vw-2rem)] flex-col items-end gap-2">
    <div
      v-if="dragActive"
      class="pointer-events-none flex h-28 w-80 max-w-full items-center justify-center rounded-lg border border-dashed border-vp-primary bg-vp-surface/95 shadow-xl"
    >
      <div class="flex items-center gap-3 text-vp-primary">
        <VpIcon name="upload" size-class="size-6" />
        <span class="text-sm font-semibold">{{ dragLabel(props.dragIntent) }}</span>
      </div>
    </div>

    <div
      v-if="items.length || message || error"
      class="w-80 max-w-full rounded-lg border border-vp-border bg-vp-surface/95 p-2.5 shadow-xl backdrop-blur"
    >
      <div class="flex items-center gap-2">
        <span
          class="inline-flex size-8 shrink-0 items-center justify-center rounded-lg bg-[color-mix(in_srgb,var(--vp-primary)_10%,var(--vp-surface))] text-vp-primary"
        >
          <VpIcon name="sparkles" size-class="size-4" />
        </span>
        <div class="min-w-0 flex-1">
          <div class="truncate text-xs font-semibold text-vp-text">
            {{ error ?? message ?? "Intake" }}
          </div>
          <div class="truncate text-[10px] font-mono text-vp-muted">
            {{ items.map((item) => kindLabel(item.kind)).join(" / ") || "Ready" }}
          </div>
        </div>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="close"
          aria-label="close"
          @click="emit('dismiss')"
        >
          <VpIcon name="x" size-class="size-3.5" />
        </button>
      </div>

      <div v-if="items.length" class="mt-2 flex flex-wrap gap-1.5">
        <span
          v-for="item in items"
          :key="item.id"
          class="inline-flex max-w-full items-center gap-1 rounded-md border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))] px-2 py-1 text-[10px] font-medium text-vp-text"
          :title="item.name"
        >
          <VpIcon
            :name="
              item.kind === 'codex-config'
                ? 'file-code'
                : item.kind === 'ccs-profile' || item.kind === 'ccswitch-provider'
                  ? 'server'
                  : item.kind === 'unknown'
                    ? 'file-text'
                    : 'key'
            "
            size-class="size-3.5 text-vp-muted"
          />
          <span class="truncate">{{ item.summary }}</span>
        </span>
      </div>

      <div class="mt-2 flex items-center justify-end gap-1.5">
        <button
          class="vp-icon-btn !size-8"
          type="button"
          :title="clipboardWatch ? 'clipboard:stop' : 'clipboard:watch'"
          :aria-label="clipboardWatch ? 'clipboard:stop' : 'clipboard:watch'"
          :disabled="busy || !clipboardWatchAvailable"
          :class="clipboardWatch ? 'text-vp-primary' : ''"
          @click="emit('toggleClipboardWatch')"
        >
          <VpIcon name="activity" size-class="size-3.5" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="Codex"
          aria-label="Codex"
          @click="emit('goCodex')"
        >
          <VpIcon name="terminal" size-class="size-3.5" />
        </button>
        <button
          v-if="configCount"
          class="vp-icon-btn !size-8"
          type="button"
          title="config:save"
          aria-label="config:save"
          :disabled="busy"
          @click="emit('saveConfig')"
        >
          <VpIcon name="save" size-class="size-3.5" />
        </button>
        <button
          v-if="authCount"
          class="vp-icon-btn !size-8"
          type="button"
          title="auth:import"
          aria-label="auth:import"
          :disabled="busy"
          @click="emit('importAuth')"
        >
          <VpIcon name="folder-input" size-class="size-3.5" />
        </button>
        <button
          v-if="ccsProfileCount"
          class="vp-icon-btn !size-8"
          type="button"
          title="profile:import"
          aria-label="profile:import"
          :disabled="busy"
          @click="emit('importCcsProfile')"
        >
          <VpIcon name="server" size-class="size-3.5" />
        </button>
      </div>
    </div>

    <button
      class="inline-flex size-10 items-center justify-center rounded-lg border border-vp-border bg-vp-surface text-vp-muted shadow-lg hover:text-vp-text focus:outline-none focus-visible:ring-2 focus-visible:ring-vp-primary/30"
      :class="clipboardWatch ? 'border-vp-primary text-vp-primary' : ''"
      type="button"
      :title="clipboardWatch ? 'clipboard:stop' : 'clipboard:read'"
      :aria-label="clipboardWatch ? 'clipboard:stop' : 'clipboard:read'"
      :disabled="busy || !clipboardWatchAvailable"
      @click="clipboardWatch ? emit('toggleClipboardWatch') : emit('readClipboard')"
    >
      <VpIcon name="clipboard" size-class="size-4" />
    </button>
  </div>
</template>

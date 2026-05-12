<script setup lang="ts">
import { computed, onMounted, shallowRef } from "vue";
import { api, type ClientStatus } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";

const props = defineProps<{
  client: "claude" | "codex" | "opencode";
  title: string;
}>();

const status = shallowRef<ClientStatus | null>(null);
const loading = shallowRef(false);
const saving = shallowRef(false);
const error = shallowRef<string | null>(null);

const takenOver = computed(() => status.value?.taken_over ?? false);
const busy = computed(() => loading.value || saving.value);
const stateLabel = computed(() => (takenOver.value ? "On" : "Off"));
const titleText = computed(() => {
  if (error.value) return error.value;
  if (!status.value) return `Checking ${props.title}`;
  const target = status.value.configured_base_url ?? "not configured";
  return `${props.title}: ${stateLabel.value}. ${target}`;
});

async function refresh() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await api.clients.status(props.client);
  } catch (err) {
    error.value = (err as Error).message || "Could not read takeover status";
  } finally {
    loading.value = false;
  }
}

async function setTakeover(next: boolean) {
  saving.value = true;
  error.value = null;
  try {
    const result = next
      ? await api.clients.takeover(props.client)
      : await api.clients.restore(props.client);
    status.value = result.status;
  } catch (err) {
    error.value = (err as Error).message || "Could not update takeover";
    await refresh();
  } finally {
    saving.value = false;
  }
}

function onToggle(event: Event) {
  const input = event.target as HTMLInputElement;
  void setTakeover(input.checked);
}

onMounted(() => {
  void refresh();
});
</script>

<template>
  <label
    class="inline-flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-xl border px-2.5 text-xs font-semibold shadow-sm transition"
    :class="[
      takenOver
        ? 'border-emerald-200 bg-emerald-50 text-emerald-800 shadow-emerald-500/10'
        : 'border-vp-border bg-vp-surface text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text',
      busy ? 'cursor-wait opacity-70' : '',
      error ? 'border-red-200 bg-red-50 text-red-700' : '',
    ]"
    :title="titleText"
  >
    <VpIcon name="power" size-class="size-4" :class="takenOver ? 'text-emerald-600' : ''" />
    <span class="hidden sm:inline">{{ title }}</span>
    <span class="font-mono">{{ stateLabel }}</span>
    <input
      class="sr-only"
      type="checkbox"
      :checked="takenOver"
      :disabled="busy"
      @change="onToggle"
    />
  </label>
</template>

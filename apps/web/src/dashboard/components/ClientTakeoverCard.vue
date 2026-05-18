<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef } from "vue";
import { useI18n } from "vue-i18n";
import { api, type ClientStatus } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";

const { t } = useI18n();

const props = defineProps<{
  client: "claude" | "codex" | "opencode";
  title: string;
}>();

const emit = defineEmits<{
  status: [takenOver: boolean | null];
}>();

const status = shallowRef<ClientStatus | null>(null);
const loading = shallowRef(false);
const saving = shallowRef(false);
const error = shallowRef<string | null>(null);
const trafficPhase = shallowRef(0);
let trafficTimer: number | null = null;

const takenOver = computed(() => status.value?.taken_over ?? false);
const busy = computed(() => loading.value || saving.value);
const activeLabel = computed(() => {
  if (props.client === "codex") return t("active.codex");
  if (props.client === "claude") return t("active.claude");
  return t("active.default");
});
const animatedActiveChars = computed(() => {
  const chars = Array.from(activeLabel.value);
  const activeIndex = chars.length === 0 ? 0 : trafficPhase.value % chars.length;

  return chars.map((char, index) => ({
    char,
    index,
    key: `${props.client}-${index}-${char}`,
  }));
});
const stateLabel = computed(() => {
  if (error.value) return t("state.error");
  if (!status.value) return "...";
  return takenOver.value ? activeLabel.value : t("state.direct");
});
const titleText = computed(() => {
  if (error.value) return error.value;
  if (!status.value) return t("title.checking", { title: props.title });
  const target = takenOver.value
    ? status.value.expected_base_url
    : (status.value.configured_base_url ?? t("state.notConfigured"));
  return t("title.action", {
    title: props.title,
    state: stateLabel.value,
    action: takenOver.value ? t("actions.restore") : t("actions.takeOver"),
    target,
  });
});

async function refresh() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await api.clients.status(props.client);
    emit("status", status.value.taken_over);
  } catch (err) {
    error.value = (err as Error).message || t("errors.readStatus");
    emit("status", null);
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
    emit("status", status.value.taken_over);
  } catch (err) {
    error.value = (err as Error).message || t("errors.updateTakeover");
    await refresh();
  } finally {
    saving.value = false;
  }
}

function toggle() {
  if (busy.value) return;
  void setTakeover(!takenOver.value);
}

onMounted(() => {
  trafficTimer = window.setInterval(() => {
    trafficPhase.value += 1;
  }, 180);
  void refresh();
});

onBeforeUnmount(() => {
  if (trafficTimer !== null) {
    window.clearInterval(trafficTimer);
    trafficTimer = null;
  }
});
</script>

<template>
  <button
    type="button"
    class="group/takeover relative inline-flex h-9 shrink-0 items-center gap-1.5 overflow-hidden rounded-xl border px-2 text-xs font-semibold shadow-sm transition sm:gap-2 sm:px-2.5"
    :class="[
      takenOver
        ? 'glow-ring border-[color-mix(in_srgb,var(--vp-primary)_24%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-vp-text'
        : 'border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text',
      busy ? 'cursor-wait opacity-70' : '',
      error ? 'border-red-200 bg-red-50 text-red-700' : '',
    ]"
    :title="titleText"
    :aria-pressed="takenOver"
    :disabled="busy"
    @click="toggle"
  >
    <span
      v-if="takenOver"
      class="absolute inset-x-0 bottom-0 h-0.5 bg-gradient-to-r from-[var(--vp-primary)] via-[var(--vp-brand-light)] to-[var(--vp-success)]"
    />
    <span
      class="size-2 rounded-full"
      :class="
        takenOver
          ? 'live-dot bg-vp-primary shadow-sm shadow-vp-primary/30'
          : error
            ? 'bg-red-500'
            : 'bg-slate-300'
      "
    />
    <VpIcon name="power" size-class="size-4" :class="takenOver ? 'text-vp-primary' : ''" />
    <span class="hidden font-mono min-[380px]:inline">
      <span
        v-if="takenOver && busy && !error"
        class="traffic-wordmark traffic-wordmark--compact"
        aria-hidden="true"
      >
        <span
          v-for="part in animatedActiveChars"
          :key="part.key"
          class="traffic-wordmark__char traffic-wordmark__char--phase"
        >
          {{ part.char }}
        </span>
      </span>
      <span v-else>{{ stateLabel }}</span>
    </span>
  </button>
</template>

<i18n lang="json">
{
  "en": {
    "actions": { "restore": "restore", "takeOver": "take over" },
    "active": { "claude": "Clauding", "codex": "Codexing", "default": "Routing" },
    "errors": {
      "readStatus": "Could not read takeover status",
      "updateTakeover": "Could not update takeover"
    },
    "state": { "direct": "Direct", "error": "Error", "notConfigured": "not configured" },
    "title": {
      "action": "{title}: {state}. Click to {action}. {target}",
      "checking": "Checking {title}"
    }
  },
  "zh-CN": {
    "actions": { "restore": "恢复", "takeOver": "接管" },
    "active": { "claude": "Clauding", "codex": "Codexing", "default": "路由中" },
    "errors": { "readStatus": "无法读取接管状态", "updateTakeover": "无法更新接管状态" },
    "state": { "direct": "直连", "error": "错误", "notConfigured": "未配置" },
    "title": {
      "action": "{title}：{state}。点击以{action}。{target}",
      "checking": "正在检查 {title}"
    }
  }
}
</i18n>

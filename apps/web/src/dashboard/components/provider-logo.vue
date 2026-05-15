<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { ProviderKind } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import type { vp_icon_name } from "./vp-icon.vue";

const props = withDefaults(
  defineProps<{
    kind?: ProviderKind;
    avatarUrl?: string | null;
    providerName?: string | null;
    enabled?: boolean;
    circuitState?: string | null;
    activeRequestCount?: number;
    tokensPerSec?: number | null;
    activityLabel?: string | null;
    sizeClass?: string;
    iconSizeClass?: string;
  }>(),
  {
    enabled: true,
    circuitState: "closed",
    activeRequestCount: 0,
    tokensPerSec: null,
    activityLabel: null,
    sizeClass: "size-9",
    iconSizeClass: "size-5",
  },
);

function providerIconName(kind: ProviderKind | undefined): vp_icon_name {
  if (kind === "openai-chat") return "bot";
  return "server";
}

function providerBrandIconClass(kind: ProviderKind | undefined): string | null {
  if (kind === "openai-responses" || kind === "openai-chat") return "i-lobe-openai";
  if (kind === "anthropic") return "i-lobe-anthropic";
  if (kind === "gemini-native") return "i-lobe-gemini-color";
  return null;
}

const active = computed(() => props.enabled && props.activeRequestCount > 0);
const blocked = computed(() => props.circuitState === "open" || props.circuitState === "half-open");
const brandIconClass = computed(() => providerBrandIconClass(props.kind));
const fallbackInitial = computed(() => (props.providerName?.trim()?.[0] ?? "?").toUpperCase());
const fallbackIconName = computed(() => providerIconName(props.kind));
const motionEl = ref<HTMLElement | null>(null);
let frameId = 0;
let currentPlaybackRate = 1;

const targetPlaybackRate = computed(() => {
  if (!active.value) return 1;
  const tps = Math.max(0, Number(props.tokensPerSec ?? 0));
  if (tps <= 0) return 0.55;
  return Math.max(0.7, Math.min(4.6, 0.85 + Math.log10(tps + 1) * 1.15));
});

function setMotionEl(el: Element | null) {
  motionEl.value = el instanceof HTMLElement ? el : null;
  void nextTick(updateAnimationRate);
}

function updateAnimationRate() {
  if (!motionEl.value || !active.value) return;
  const animation = motionEl.value.getAnimations()[0];
  if (animation) animation.playbackRate = currentPlaybackRate;
}

function tickPlaybackRate() {
  const target = targetPlaybackRate.value;
  currentPlaybackRate += (target - currentPlaybackRate) * 0.18;
  updateAnimationRate();
  if (Math.abs(target - currentPlaybackRate) > 0.01) {
    frameId = window.requestAnimationFrame(tickPlaybackRate);
  } else {
    currentPlaybackRate = target;
    updateAnimationRate();
    frameId = 0;
  }
}

watch(
  [active, targetPlaybackRate],
  () => {
    if (frameId) window.cancelAnimationFrame(frameId);
    if (!active.value) {
      currentPlaybackRate = 1;
      frameId = 0;
      return;
    }
    void nextTick(() => {
      if (!frameId) frameId = window.requestAnimationFrame(tickPlaybackRate);
    });
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (frameId) window.cancelAnimationFrame(frameId);
});
const statusClass = computed(() => {
  if (!props.enabled) return "bg-slate-400";
  if (props.circuitState === "open") return "bg-red-500";
  if (props.circuitState === "half-open") return "bg-amber-500";
  if (active.value) return "bg-emerald-500";
  return "bg-sky-300";
});
const title = computed(() => {
  if (!props.enabled) return "provider:off";
  if (props.circuitState === "open") return "provider:circuit-open";
  if (props.circuitState === "half-open") return "provider:circuit-half-open";
  if (active.value)
    return `${props.activeRequestCount} active · ${props.activityLabel ?? `${(props.tokensPerSec ?? 0).toFixed(1)} tok/s`}`;
  return "provider:idle";
});
</script>

<template>
  <span
    class="provider-logo relative grid shrink-0 place-items-center overflow-hidden rounded-lg bg-gradient-to-br from-violet-100 to-cyan-50 ring-1 ring-vp-border"
    :class="[
      sizeClass,
      !enabled ? 'opacity-65 grayscale' : '',
      blocked ? 'ring-red-200 bg-red-50' : '',
    ]"
    :title="title"
  >
    <img
      v-if="avatarUrl"
      :src="avatarUrl"
      :alt="providerName ?? 'provider avatar'"
      class="h-full w-full object-cover"
      loading="lazy"
      referrerpolicy="no-referrer"
    />
    <span
      v-else-if="brandIconClass"
      :ref="setMotionEl"
      :class="[
        brandIconClass,
        iconSizeClass,
        active ? 'provider-logo__spin' : 'provider-logo__breathe',
      ]"
      aria-hidden="true"
    />
    <div
      v-else-if="providerName"
      :ref="setMotionEl"
      class="text-xs font-semibold text-slate-700"
      :class="active ? 'provider-logo__breathe' : ''"
      aria-hidden="true"
    >
      {{ fallbackInitial }}
    </div>
    <span
      v-else
      :ref="setMotionEl"
      :class="active ? 'provider-logo__spin' : 'provider-logo__breathe'"
      aria-hidden="true"
    >
      <VpIcon :name="fallbackIconName" :size-class="iconSizeClass" />
    </span>
    <span
      class="absolute bottom-1 right-1 size-1.5 rounded-full ring-1 ring-white"
      :class="statusClass"
    />
  </span>
</template>

<style scoped>
.provider-logo__spin {
  animation: provider-logo-spin 2.8s linear infinite;
  transform-origin: 50% 50%;
  will-change: transform;
}

.provider-logo__breathe {
  animation: provider-logo-breathe 3.4s ease-in-out infinite;
  transform-origin: 50% 50%;
  will-change: transform, opacity;
}

@keyframes provider-logo-spin {
  to {
    transform: rotate(360deg);
  }
}

@keyframes provider-logo-breathe {
  0%,
  100% {
    transform: scale(0.96);
    opacity: 0.72;
  }
  50% {
    transform: scale(1.04);
    opacity: 1;
  }
}
</style>

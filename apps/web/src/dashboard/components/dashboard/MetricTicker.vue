<script setup lang="ts">
import { computed, shallowRef, watch } from "vue";

const props = withDefaults(
  defineProps<{
    value: number | string | null | undefined;
    suffix?: string;
    precision?: number;
    tone?: "default" | "good" | "hot" | "muted";
    size?: "sm" | "md" | "lg";
  }>(),
  {
    suffix: "",
    precision: 0,
    tone: "default",
    size: "md",
  },
);

const pulse = shallowRef(false);

const displayValue = computed(() => {
  if (props.value === undefined || props.value === null) return "-";
  if (typeof props.value === "string") return props.value;
  if (!Number.isFinite(props.value)) return "-";
  return props.value.toLocaleString(undefined, {
    maximumFractionDigits: props.precision,
    minimumFractionDigits: props.precision,
  });
});

const toneClass = computed(() => {
  if (props.tone === "good") return "text-emerald-600";
  if (props.tone === "hot") return "text-rose-600";
  if (props.tone === "muted") return "text-vp-muted";
  return "text-vp-text";
});

const sizeClass = computed(() => {
  if (props.size === "lg") return "text-2xl sm:text-3xl";
  if (props.size === "sm") return "text-sm";
  return "text-xl";
});

watch(
  () => props.value,
  (next, prev) => {
    if (next === prev) return;
    pulse.value = false;
    window.requestAnimationFrame(() => {
      pulse.value = true;
    });
  },
);
</script>

<template>
  <span
    class="metric-ticker inline-flex min-w-0 items-baseline gap-1 font-mono font-semibold tabular-nums"
    :class="[toneClass, sizeClass, pulse ? 'metric-ticker--pulse' : '']"
    @animationend="pulse = false"
  >
    <span class="truncate">{{ displayValue }}</span>
    <span v-if="suffix" class="text-[0.58em] font-medium text-vp-muted">{{ suffix }}</span>
  </span>
</template>

<style scoped>
.metric-ticker {
  font-variant-numeric: tabular-nums;
}

.metric-ticker--pulse {
  animation: metric-ticker-pop 320ms ease-out;
}

@keyframes metric-ticker-pop {
  0% {
    transform: translateY(0);
  }
  45% {
    transform: translateY(-2px);
    text-shadow: 0 0 18px color-mix(in srgb, currentColor 28%, transparent);
  }
  100% {
    transform: translateY(0);
  }
}
</style>

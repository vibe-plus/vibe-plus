<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    /** 0–100 */
    pct: number | null | undefined;
    label: string;
    centerText?: string;
    size?: number;
    strokeWidth?: number;
  }>(),
  {
    size: 52,
    strokeWidth: 5,
  },
);

const r = computed(() => (props.size - props.strokeWidth) / 2);
const circ = computed(() => 2 * Math.PI * r.value);
const dashOffset = computed(() => {
  const p = Math.min(100, Math.max(0, props.pct ?? 0));
  return circ.value * (1 - p / 100);
});
const displayText = computed(() => {
  if (props.centerText) return props.centerText;
  if (props.pct == null) return "—";
  return `${Math.round(props.pct)}%`;
});
const arcColor = computed(() => {
  if (props.pct == null) return "#6b7280";
  if (props.pct >= 90) return "#ef4444";
  if (props.pct >= 70) return "#f59e0b";
  return "#10b981";
});
</script>

<template>
  <div class="flex flex-col items-center gap-1">
    <!-- ring + center text stacked -->
    <div class="relative" :style="{ width: `${size}px`, height: `${size}px` }">
      <svg
        :width="size"
        :height="size"
        :viewBox="`0 0 ${size} ${size}`"
        style="transform: rotate(-90deg)"
      >
        <circle
          :cx="size / 2"
          :cy="size / 2"
          :r="r"
          fill="none"
          stroke="rgba(255,255,255,0.1)"
          :stroke-width="strokeWidth"
        />
        <circle
          :cx="size / 2"
          :cy="size / 2"
          :r="r"
          fill="none"
          :stroke="arcColor"
          :stroke-width="strokeWidth"
          stroke-linecap="round"
          :stroke-dasharray="circ"
          :stroke-dashoffset="dashOffset"
          style="
            transition:
              stroke-dashoffset 0.4s ease,
              stroke 0.4s ease;
          "
        />
      </svg>
      <!-- center label -->
      <span
        class="absolute inset-0 flex items-center justify-center text-[10px] font-semibold tabular-nums text-white/90 leading-none pointer-events-none"
        >{{ displayText }}</span
      >
    </div>
    <span class="text-[9px] text-white/50 leading-none truncate max-w-full text-center">{{
      label
    }}</span>
  </div>
</template>

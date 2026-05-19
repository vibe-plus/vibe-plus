<script setup lang="ts">
import { computed } from "vue";
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    value?: number;
    max?: number;
    tone?: "default" | "success" | "warning" | "destructive";
    class?: string;
  }>(),
  {
    value: 0,
    max: 100,
    tone: "default",
    class: "",
  },
);

const pct = computed(() => {
  const v = Number.isFinite(props.value) ? props.value : 0;
  const m = Number.isFinite(props.max) && props.max > 0 ? props.max : 100;
  return Math.max(0, Math.min(100, (v / m) * 100));
});

const toneClass: Record<NonNullable<typeof props.tone>, string> = {
  default: "bg-primary",
  success: "bg-emerald-500",
  warning: "bg-amber-500",
  destructive: "bg-red-500",
};

const trackClass = computed(() =>
  cn("relative h-2 w-full overflow-hidden rounded-full bg-muted", props.class),
);
</script>

<template>
  <div
    :class="trackClass"
    role="progressbar"
    :aria-valuenow="value"
    :aria-valuemax="max"
    aria-valuemin="0"
  >
    <div
      class="h-full transition-[width] duration-500 ease-out"
      :class="toneClass[tone]"
      :style="{ width: `${pct}%` }"
    />
  </div>
</template>

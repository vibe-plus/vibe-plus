<script setup lang="ts">
import { computed } from "vue";
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    /** Series of numeric values, oldest first. Empty array renders nothing. */
    values: number[];
    /** SVG width in CSS pixels. The viewBox auto-fits. */
    width?: number;
    /** SVG height in CSS pixels. */
    height?: number;
    /** Stroke color (any CSS color). Defaults to current text color. */
    color?: string;
    /** Fill under the curve. Falsy → no fill. */
    fill?: string;
    /** Show a small dot at the latest sample. */
    showLastPoint?: boolean;
    /** Optional accessible label. */
    label?: string;
    class?: string;
  }>(),
  {
    width: 140,
    height: 36,
    color: "currentColor",
    fill: "",
    showLastPoint: true,
    label: "",
    class: "",
  },
);

const VIEW_W = 100;
const VIEW_H = 100;
const PAD_Y = 6; // keep stroke off the top/bottom edge

const shape = computed(() => {
  const vs = props.values.filter((v) => Number.isFinite(v));
  if (vs.length === 0) return { path: "", area: "", last: null as null | { x: number; y: number } };
  if (vs.length === 1) {
    const y = VIEW_H / 2;
    return {
      path: `M0,${y} L${VIEW_W},${y}`,
      area: "",
      last: { x: VIEW_W, y },
    };
  }
  const lo = Math.min(...vs);
  const hi = Math.max(...vs);
  const span = hi - lo || 1;
  const usableH = VIEW_H - PAD_Y * 2;
  const stepX = VIEW_W / (vs.length - 1);
  const points = vs.map((v, i) => {
    const x = stepX * i;
    const y = PAD_Y + (1 - (v - lo) / span) * usableH;
    return { x, y };
  });
  const path = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(2)},${p.y.toFixed(2)}`)
    .join(" ");
  const area = props.fill ? `${path} L${VIEW_W},${VIEW_H} L0,${VIEW_H} Z` : "";
  const last = points[points.length - 1] ?? null;
  return { path, area, last };
});

const accessibleLabel = computed(() => props.label || "sparkline");
</script>

<template>
  <svg
    :class="cn('block', props.class)"
    :width="width"
    :height="height"
    :viewBox="`0 0 ${VIEW_W} ${VIEW_H}`"
    preserveAspectRatio="none"
    role="img"
    :aria-label="accessibleLabel"
  >
    <path v-if="fill && shape.area" :d="shape.area" :fill="fill" stroke="none" />
    <path
      v-if="shape.path"
      :d="shape.path"
      :stroke="color"
      stroke-width="1.6"
      vector-effect="non-scaling-stroke"
      fill="none"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <circle
      v-if="showLastPoint && shape.last"
      :cx="shape.last.x"
      :cy="shape.last.y"
      r="2"
      :fill="color"
    />
  </svg>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { BRAND_NAME } from "../lib/brand.ts";

const props = withDefaults(
  defineProps<{
    /** Visual theme for the colored plus sign. */
    variant?: "dashboard" | "lander";
    class?: string;
  }>(),
  {
    variant: "dashboard",
  },
);

const plusClass = computed(() =>
  props.variant === "lander" ? "text-[#4dd4ad]" : "text-vp-primary",
);
</script>

<template>
  <span :class="props.class" :aria-label="BRAND_NAME"> Vibe<span :class="plusClass">+</span> </span>
</template>

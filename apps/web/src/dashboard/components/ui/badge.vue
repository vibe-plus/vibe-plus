<script setup lang="ts">
import { computed } from "vue";
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    variant?: "default" | "secondary" | "outline" | "destructive";
    class?: string;
  }>(),
  {
    variant: "default",
    class: "",
  },
);

const classes: Record<NonNullable<typeof props.variant>, string> = {
  default: "bg-primary text-primary-foreground hover:bg-primary/80",
  secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
  outline: "text-foreground border border-border bg-transparent",
  destructive: "bg-destructive text-destructive-foreground",
};

const badgeClass = computed(() =>
  cn(
    "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors",
    classes[props.variant],
    props.class,
  ),
);
</script>

<template>
  <span :class="badgeClass">
    <slot />
  </span>
</template>

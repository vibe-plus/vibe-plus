<script setup lang="ts">
import { computed } from "vue";
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    variant?: "default" | "secondary" | "ghost" | "outline" | "destructive";
    size?: "default" | "sm" | "lg" | "icon";
    class?: string;
    type?: "button" | "submit" | "reset";
  }>(),
  {
    variant: "default",
    size: "default",
    type: "button",
    class: "",
  },
);

const variantClasses: Record<NonNullable<typeof props.variant>, string> = {
  default: "bg-primary text-primary-foreground shadow hover:bg-primary/90",
  secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
  ghost: "hover:bg-accent hover:text-accent-foreground",
  outline: "border border-input bg-background hover:bg-accent hover:text-accent-foreground",
  destructive: "bg-destructive text-destructive-foreground shadow hover:bg-destructive/90",
};

const sizeClasses: Record<NonNullable<typeof props.size>, string> = {
  default: "h-10 px-4 py-2",
  sm: "h-9 rounded-md px-3",
  lg: "h-11 rounded-md px-8",
  icon: "size-10",
};

const buttonClass = computed(() =>
  cn(
    "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50",
    variantClasses[props.variant],
    sizeClasses[props.size],
    props.class,
  ),
);
</script>

<template>
  <button :type="type" :class="buttonClass">
    <slot />
  </button>
</template>

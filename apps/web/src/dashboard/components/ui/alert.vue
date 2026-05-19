<script setup lang="ts">
import { computed } from "vue";
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    variant?: "default" | "warning" | "destructive" | "info" | "success";
    class?: string;
  }>(),
  {
    variant: "default",
    class: "",
  },
);

const variantClasses: Record<NonNullable<typeof props.variant>, string> = {
  default: "bg-background text-foreground border-border",
  warning:
    "border-amber-300/70 bg-amber-50/95 text-amber-950 dark:bg-amber-950/40 dark:text-amber-100",
  destructive: "border-red-300/70 bg-red-50/95 text-red-950 dark:bg-red-950/40 dark:text-red-100",
  info: "border-sky-300/70 bg-sky-50/95 text-sky-950 dark:bg-sky-950/40 dark:text-sky-100",
  success:
    "border-emerald-300/70 bg-emerald-50/95 text-emerald-950 dark:bg-emerald-950/40 dark:text-emerald-100",
};

const alertClass = computed(() =>
  cn(
    "relative w-full rounded-2xl border px-4 py-3 text-sm shadow-sm [&>svg+div]:translate-y-[-3px] [&>svg]:absolute [&>svg]:left-4 [&>svg]:top-4 [&>svg~*]:pl-7",
    variantClasses[props.variant],
    props.class,
  ),
);
</script>

<template>
  <div role="alert" :class="alertClass">
    <slot />
  </div>
</template>

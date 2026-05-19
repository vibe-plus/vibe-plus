<script setup lang="ts">
import { cn } from "../../../lib/utils.ts";

const props = withDefaults(
  defineProps<{
    class?: string;
  }>(),
  {
    class: "",
  },
);
</script>

<template>
  <div
    role="tablist"
    :class="
      cn(
        'inline-flex h-9 items-center justify-start rounded-lg bg-muted p-1 text-muted-foreground gap-1 overflow-x-auto',
        props.class,
      )
    "
  >
    <slot />
  </div>
</template>

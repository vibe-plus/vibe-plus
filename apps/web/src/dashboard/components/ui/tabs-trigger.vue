<script setup lang="ts">
import { computed, inject } from "vue";
import { cn } from "../../../lib/utils.ts";
import { TABS_CONTEXT } from "./tabs-context.ts";

const props = withDefaults(
  defineProps<{
    value: string;
    class?: string;
  }>(),
  {
    class: "",
  },
);

const ctx = inject(TABS_CONTEXT);
const selected = computed(() => ctx?.active.value === props.value);

function activate() {
  ctx?.setActive(props.value);
}
</script>

<template>
  <button
    type="button"
    role="tab"
    :aria-selected="selected"
    :data-state="selected ? 'active' : 'inactive'"
    :class="
      cn(
        'inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-sm font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50',
        selected
          ? 'bg-background text-foreground shadow-sm'
          : 'text-muted-foreground hover:text-foreground',
        props.class,
      )
    "
    @click="activate"
  >
    <slot />
  </button>
</template>

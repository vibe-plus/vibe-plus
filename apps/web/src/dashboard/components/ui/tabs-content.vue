<script setup lang="ts">
import { computed, inject } from "vue";
import { cn } from "../../../lib/utils.ts";
import { TABS_CONTEXT } from "./tabs-context.ts";

const props = withDefaults(
  defineProps<{
    value: string;
    class?: string;
    /** Keep the slot mounted when inactive (for state preservation). Off by default. */
    keepMounted?: boolean;
  }>(),
  {
    class: "",
    keepMounted: false,
  },
);

const ctx = inject(TABS_CONTEXT);
const selected = computed(() => ctx?.active.value === props.value);
</script>

<template>
  <div
    v-if="selected || keepMounted"
    v-show="selected"
    role="tabpanel"
    :data-state="selected ? 'active' : 'inactive'"
    :class="cn('focus-visible:outline-none', props.class)"
  >
    <slot />
  </div>
</template>

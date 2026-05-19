<script setup lang="ts">
import { provide, computed } from "vue";
import { cn } from "../../../lib/utils.ts";
import { TABS_CONTEXT, type TabsContext } from "./tabs-context.ts";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    class?: string;
  }>(),
  {
    class: "",
  },
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const active = computed(() => props.modelValue);
function setActive(value: string) {
  emit("update:modelValue", value);
}

const ctx: TabsContext = { active, setActive };
provide(TABS_CONTEXT, ctx);
</script>

<template>
  <div :class="cn('flex flex-col gap-3', props.class)">
    <slot />
  </div>
</template>

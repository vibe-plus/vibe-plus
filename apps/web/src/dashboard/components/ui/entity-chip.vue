<script setup lang="ts">
import { computed } from "vue";
import { RouterLink } from "vue-router";
import { cn } from "../../../lib/utils.ts";
import {
  resolveEntityLabel,
  resolveEntityRoute,
  type EntityKind,
  type EntityRef,
} from "../../lib/entity-links.ts";

const props = withDefaults(
  defineProps<{
    kind: EntityKind;
    id: string;
    label?: string | null;
    fallback?: string;
    variant?: "link" | "chip" | "inline";
    class?: string;
  }>(),
  {
    variant: "link",
    class: "",
  },
);

const ref = computed<EntityRef>(() => ({ kind: props.kind, id: props.id, label: props.label }));
const route = computed(() => resolveEntityRoute(ref.value));
const text = computed(() => resolveEntityLabel(ref.value, props.fallback));

const linkClass =
  "text-sky-600 underline decoration-dotted underline-offset-2 transition-colors hover:text-sky-500 dark:text-sky-400";
const chipClass =
  "inline-flex items-center gap-1 rounded-full border border-border bg-muted/50 px-2 py-0.5 text-[11px] font-medium text-foreground hover:bg-muted";
const inlineClass = "font-mono text-[11px] text-muted-foreground";

const computedClass = computed(() =>
  cn(
    props.variant === "link" ? linkClass : props.variant === "chip" ? chipClass : inlineClass,
    props.class,
  ),
);
</script>

<template>
  <RouterLink v-if="route" :to="route" :class="computedClass">{{ text }}</RouterLink>
  <span v-else :class="computedClass">{{ text }}</span>
</template>

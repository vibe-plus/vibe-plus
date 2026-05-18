<script setup lang="ts">
defineProps<{
  class?: string;
}>();
</script>

<template>
  <div class="shrink-0 bg-border" :class="$props.class ?? 'h-px w-full'" />
</template>

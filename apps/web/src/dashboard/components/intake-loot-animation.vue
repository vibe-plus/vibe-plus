<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import VpIcon from "./vp-icon.vue";

const props = defineProps<{
  open: boolean;
  label: string;
  targetProviderId?: string | null;
}>();

const pos = ref({ left: "max(1rem,8vw)", top: "max(4rem,18vh)" });

function updateTarget() {
  const id = props.targetProviderId;
  if (!id) {
    pos.value = { left: "max(1rem,8vw)", top: "max(4rem,18vh)" };
    return;
  }
  const el = document.querySelector(`[data-provider-id="${id}"]`) as HTMLElement | null;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  pos.value = {
    left: `${Math.max(16, rect.left + rect.width * 0.5 - 96)}px`,
    top: `${Math.max(24, rect.top + rect.height * 0.35)}px`,
  };
}

onMounted(updateTarget);
watch(
  () => [props.targetProviderId, props.open] as const,
  () => {
    if (props.open) updateTarget();
  },
  { immediate: true },
);

const panelClass = computed(() =>
  props.open
    ? "opacity-100 translate-x-0 translate-y-0 scale-100"
    : "pointer-events-none opacity-0 translate-x-[-24vw] translate-y-[16vh] scale-70",
);
</script>

<template>
  <Teleport to="body">
    <div class="pointer-events-none fixed inset-0 z-[160] overflow-hidden">
      <div
        class="absolute transition-all duration-700 ease-[cubic-bezier(0.2,0.9,0.2,1)]"
        :class="panelClass"
        :style="{ left: pos.left, top: pos.top }"
      >
        <div
          class="flex items-center gap-2 rounded-2xl border border-amber-200 bg-[linear-gradient(135deg,rgba(255,251,235,0.98),rgba(254,240,138,0.92))] px-4 py-3 text-amber-950 shadow-[0_14px_44px_rgba(217,119,6,0.24)]"
        >
          <span
            class="grid size-9 place-items-center rounded-xl bg-white/80 text-amber-600 ring-1 ring-amber-200"
          >
            <VpIcon name="package" size-class="size-4.5" />
          </span>
          <div class="min-w-0">
            <div class="text-[11px] font-mono uppercase tracking-[0.18em] text-amber-700">
              collected
            </div>
            <div class="max-w-56 truncate text-sm font-semibold">
              {{ label }}
            </div>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

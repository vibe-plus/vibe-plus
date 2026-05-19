<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

// Active wave index: 0, 1, 2. Cycles to demonstrate "try 1, then 2 in parallel, then 3 in parallel".
const activeWave = ref(0);
let timer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  timer = setInterval(() => {
    activeWave.value = (activeWave.value + 1) % 3;
  }, 1500);
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
});

const waves: { label: string; nodes: string[] }[] = [
  { label: "1", nodes: ["a"] },
  { label: "2", nodes: ["b", "c"] },
  { label: "3", nodes: ["d", "e", "f"] },
];
</script>

<template>
  <div class="rounded-xl border border-[#dfe9e4] bg-[#f0f9f4] p-4">
    <div class="flex flex-col gap-2.5">
      <div
        v-for="(wave, i) in waves"
        :key="wave.label"
        class="flex items-center gap-3 transition-opacity duration-500"
        :class="activeWave === i ? 'opacity-100' : 'opacity-35'"
      >
        <div class="text-[10px] font-mono font-medium text-[#5a6b65] w-10 shrink-0">
          {{ t("wave.waveLabel", { n: wave.label }) }}
        </div>
        <div class="flex gap-1.5">
          <div
            v-for="node in wave.nodes"
            :key="node"
            class="relative h-7 w-7 rounded-full border-2 flex items-center justify-center text-[11px] font-semibold transition-colors duration-500"
            :class="
              activeWave === i
                ? 'border-[#4dd4ad] bg-[#e7f8ef] text-[#1f7a55]'
                : 'border-[#dfe9e4] bg-white text-[#8a9591]'
            "
          >
            <span
              v-if="activeWave === i"
              class="absolute inset-0 rounded-full border-2 border-[#4dd4ad] wave-ring"
            />
            {{ node }}
          </div>
        </div>
        <div
          class="ml-auto text-[10px] font-medium transition-colors duration-500"
          :class="activeWave === i ? 'text-[#1f7a55]' : 'text-[#8a9591]'"
        >
          {{ t("wave.parallel", { n: wave.nodes.length }) }}
        </div>
      </div>
    </div>

    <p
      class="mt-3 pt-2.5 border-t border-[#dfe9e4] text-[11px] text-[#5a6b65] text-center leading-relaxed"
    >
      {{ t("wave.caption") }}
    </p>
  </div>
</template>

<style scoped>
.wave-ring {
  animation: wave-ping 1.4s cubic-bezier(0, 0, 0.2, 1) infinite;
}
@keyframes wave-ping {
  0% {
    transform: scale(1);
    opacity: 0.55;
  }
  80%,
  100% {
    transform: scale(1.55);
    opacity: 0;
  }
}
</style>

<i18n lang="json">
{
  "en": {
    "wave": {
      "waveLabel": "wave {n}",
      "parallel": "{n} parallel",
      "caption": "Worst case: 3 waves to a reply — not 6 sequential failures"
    }
  },
  "zh-CN": {
    "wave": {
      "waveLabel": "第 {n} 波",
      "parallel": "{n} 个并行",
      "caption": "最坏 3 波拿到结果 —— 不是一个个等 6 次失败"
    }
  }
}
</i18n>

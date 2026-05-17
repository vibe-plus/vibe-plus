<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import VpIcon from "../components/vp-icon.vue";
import { useBrandLogo, type BrandLogoId } from "../composables/use-brand-logo.ts";
import { setUiLanguage, useUiLanguage } from "../composables/use-ui-language.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const { brandLogos, currentBrandLogo, selectedBrandLogoId, setBrandLogo } = useBrandLogo();
const { language, languageOptions } = useUiLanguage();

function selectBrandLogo(id: BrandLogoId) {
  setBrandLogo(id);
}
</script>

<template>
  <div class="mx-auto max-w-4xl space-y-4 sm:space-y-6">
    <div class="min-w-0 space-y-2 sm:space-y-3">
      <span :class="['text-xs uppercase', pa.kicker]">settings</span>
      <h1 :class="['text-2xl sm:text-3xl font-bold tracking-tight', pa.heading]">Settings</h1>
      <p class="max-w-2xl text-sm text-vp-muted">
        Only browser-local preferences live here. Theme and language are saved in localStorage.
      </p>
    </div>

    <section class="space-y-3">
      <section id="theme" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-3 sm:mb-4 flex items-center gap-2">
          <VpIcon name="palette" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">Theme</span>
        </div>
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-5">
          <button
            v-for="logo in brandLogos"
            :key="logo.id"
            type="button"
            class="group flex min-h-20 flex-col items-center justify-center gap-1.5 rounded-lg border px-2 py-2.5 transition hover:bg-vp-bg-hover sm:min-h-24 sm:gap-2 sm:px-3 sm:py-3"
            :class="
              selectedBrandLogoId === logo.id
                ? 'border-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-vp-text shadow-sm'
                : 'border-vp-border text-vp-muted'
            "
            :title="logo.label"
            :aria-pressed="selectedBrandLogoId === logo.id"
            @click="selectBrandLogo(logo.id)"
          >
            <span
              class="flex size-11 items-center justify-center rounded-xl border border-[color-mix(in_srgb,var(--vp-text)_8%,transparent)] bg-vp-surface shadow-sm sm:size-14"
              :style="{
                boxShadow: `0 10px 24px color-mix(in srgb, ${logo.accent} 24%, transparent)`,
              }"
            >
              <img :src="logo.src" alt="" class="size-9 rounded-lg sm:size-12" />
            </span>
            <span class="text-[11px] font-semibold leading-tight sm:text-xs">{{ logo.label }}</span>
          </button>
        </div>
        <p class="mt-3 text-xs text-vp-muted">
          Current theme: <span class="font-medium text-vp-text">{{ currentBrandLogo.label }}</span>
        </p>
      </section>

      <section id="language" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-3 sm:mb-4 flex items-center gap-2">
          <VpIcon name="languages" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">Language</span>
        </div>
        <p class="mb-3 text-xs text-vp-muted">
          This only stores the selected language for future use. It does not change UI copy yet.
        </p>
        <div class="grid gap-2 sm:grid-cols-2">
          <button
            v-for="option in languageOptions"
            :key="option.value"
            type="button"
            class="flex items-start justify-between gap-3 rounded-lg border px-4 py-3 text-left transition hover:bg-vp-bg-hover"
            :class="
              language === option.value
                ? 'border-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-vp-text shadow-sm'
                : 'border-vp-border text-vp-muted'
            "
            :aria-pressed="language === option.value"
            @click="setUiLanguage(option.value)"
          >
            <span>
              <span class="block text-sm font-medium">{{ option.label }}</span>
              <span class="mt-1 block text-xs">{{ option.hint }}</span>
            </span>
            <VpIcon
              v-if="language === option.value"
              name="check"
              size-class="size-4 shrink-0 text-vp-primary"
            />
          </button>
        </div>
      </section>
    </section>
  </div>
</template>

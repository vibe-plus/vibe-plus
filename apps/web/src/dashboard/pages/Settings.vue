<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import Badge from "../components/ui/badge.vue";
import Button from "../components/ui/button.vue";
import Card from "../components/ui/card.vue";
import Separator from "../components/ui/separator.vue";
import VpIcon from "../components/vp-icon.vue";
import { useBrandLogo, type BrandLogoId } from "../composables/use-brand-logo.ts";
import { setUiLanguage, useUiLanguage, type UiLanguage } from "../composables/use-ui-language.ts";

const { t } = useI18n();
const { brandLogos, currentBrandLogo, selectedBrandLogoId, setBrandLogo } = useBrandLogo();
const { language, languageOptions } = useUiLanguage();

const themeSummary = computed(() => t("theme.summary", { theme: currentBrandLogo.value.label }));
const languageSummary = computed(
  () => languageOptions.find((option) => option.value === language.value)?.label ?? language.value,
);

function isSelectedLogo(id: BrandLogoId) {
  return selectedBrandLogoId.value === id;
}

function isSelectedLanguage(value: UiLanguage) {
  return language.value === value;
}
</script>

<template>
  <div class="mx-auto max-w-3xl space-y-5 sm:space-y-6">
    <Card class="overflow-hidden">
      <section id="theme" class="scroll-mt-20 p-4 sm:p-5">
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-2">
            <VpIcon name="palette" size-class="size-4 text-muted-foreground" />
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ t("theme.title") }}</h2>
              <p class="text-xs text-muted-foreground">{{ themeSummary }}</p>
            </div>
          </div>
          <Badge variant="secondary">{{ t("badges.local") }}</Badge>
        </div>

        <div class="grid grid-cols-2 gap-2 sm:grid-cols-5">
          <Button
            v-for="logo in brandLogos"
            :key="logo.id"
            type="button"
            variant="outline"
            class="relative h-auto min-h-24 flex-col gap-2 rounded-xl px-3 py-3"
            :class="
              isSelectedLogo(logo.id)
                ? 'border-primary bg-accent text-foreground shadow-md ring-2 ring-primary/35 ring-offset-2 ring-offset-card'
                : 'border-border/70 bg-card/70 text-muted-foreground opacity-75 hover:border-primary/35 hover:bg-card hover:opacity-100 hover:text-foreground'
            "
            :title="logo.label"
            :aria-pressed="isSelectedLogo(logo.id)"
            @click="setBrandLogo(logo.id)"
          >
            <span
              v-if="isSelectedLogo(logo.id)"
              class="absolute right-2 top-2 inline-flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm"
              aria-hidden="true"
            >
              <VpIcon name="check" size-class="size-3.5" />
            </span>
            <span
              class="flex size-12 items-center justify-center rounded-xl border bg-card shadow-sm"
              :class="isSelectedLogo(logo.id) ? 'border-primary/25' : 'border-border'"
              :style="{
                boxShadow: `0 10px 24px color-mix(in srgb, ${logo.accent} ${isSelectedLogo(logo.id) ? 36 : 18}%, transparent)`,
              }"
            >
              <img :src="logo.src" alt="" class="size-10 rounded-lg" />
            </span>
            <span class="text-xs font-semibold leading-tight">{{ logo.label }}</span>
            <span
              v-if="isSelectedLogo(logo.id)"
              class="rounded-full bg-primary/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-primary"
            >
              {{ t("common.active") }}
            </span>
          </Button>
        </div>
      </section>

      <Separator />

      <section id="language" class="scroll-mt-20 p-4 sm:p-5">
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-2">
            <VpIcon name="languages" size-class="size-4 text-muted-foreground" />
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ t("language.title") }}</h2>
              <p class="text-xs text-muted-foreground">
                {{ t("language.selected", { language: languageSummary }) }}
              </p>
            </div>
          </div>
          <Badge variant="secondary">{{ t("badges.futureCopy") }}</Badge>
        </div>

        <div class="grid gap-2 sm:grid-cols-2">
          <Button
            v-for="option in languageOptions"
            :key="option.value"
            type="button"
            variant="outline"
            class="h-auto justify-between rounded-xl px-4 py-3 text-left"
            :class="
              isSelectedLanguage(option.value)
                ? 'border-primary/50 bg-accent text-foreground shadow-sm'
                : 'bg-card text-muted-foreground hover:text-foreground'
            "
            :aria-pressed="isSelectedLanguage(option.value)"
            @click="setUiLanguage(option.value)"
          >
            <span>
              <span class="block text-sm font-medium">{{ option.label }}</span>
              <span class="mt-1 block text-xs font-normal">{{ option.hint }}</span>
            </span>
            <VpIcon
              v-if="isSelectedLanguage(option.value)"
              name="check"
              size-class="size-4 shrink-0 text-primary"
            />
          </Button>
        </div>
      </section>
    </Card>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "badges": {
      "futureCopy": "Future copy",
      "local": "Local"
    },
    "common": {
      "active": "Active"
    },
    "language": {
      "selected": "Selected: {language}",
      "title": "Language"
    },
    "page": {
      "description": "Keep this page focused on browser-local preferences saved in localStorage.",
      "kicker": "settings",
      "title": "Settings"
    },
    "theme": {
      "summary": "Current theme: {theme}",
      "title": "Theme"
    }
  },
  "zh-CN": {
    "badges": {
      "futureCopy": "后续文案",
      "local": "本地"
    },
    "common": {
      "active": "已启用"
    },
    "language": {
      "selected": "当前：{language}",
      "title": "语言"
    },
    "page": {
      "description": "这个页面只管理保存在 localStorage 中的浏览器本地偏好。",
      "kicker": "设置",
      "title": "设置"
    },
    "theme": {
      "summary": "当前主题：{theme}",
      "title": "主题"
    }
  }
}
</i18n>

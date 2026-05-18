import { watch } from "vue";
import { createI18n } from "vue-i18n";
import { useUiLanguage } from "./composables/use-ui-language.ts";

export type DashboardMessageSchema = Record<string, unknown>;

export const i18n = createI18n<[DashboardMessageSchema], "en" | "zh-CN">({
  legacy: false,
  globalInjection: true,
  locale: "en",
  fallbackLocale: "en",
  messages: {
    en: {},
    "zh-CN": {},
  },
  missingWarn: import.meta.env.DEV,
  fallbackWarn: import.meta.env.DEV,
});

let started = false;

export function syncI18nWithUiLanguage() {
  if (started) return;
  started = true;

  const { language } = useUiLanguage();
  watch(
    language,
    (next) => {
      i18n.global.locale = next;
      if (typeof document !== "undefined") {
        document.documentElement.lang = next;
      }
    },
    { immediate: true },
  );
}

import { isRef, watch } from "vue";
import { createI18n } from "vue-i18n";
import { useUiLanguage, type UiLanguage } from "./composables/use-ui-language.ts";

function setGlobalI18nLocale(next: UiLanguage) {
  const locale = i18n.global.locale;
  if (isRef(locale)) {
    locale.value = next;
  }
}

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
    () => language.value,
    (next) => {
      setGlobalI18nLocale(next);
      if (typeof document !== "undefined") {
        document.documentElement.lang = next;
      }
    },
    { immediate: true },
  );
}

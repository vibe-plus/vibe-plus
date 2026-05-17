import { readonly, shallowRef, watch } from "vue";

export type UiLanguage = "en" | "zh-CN";

const STORAGE_KEY = "vibe-plus:ui-language";

const LANGUAGE_OPTIONS: readonly {
  value: UiLanguage;
  label: string;
  hint: string;
}[] = [
  {
    value: "en",
    label: "English",
    hint: "English UI copy",
  },
  {
    value: "zh-CN",
    label: "简体中文",
    hint: "简体中文界面",
  },
] as const;

function isUiLanguage(value: string | null): value is UiLanguage {
  return value === "en" || value === "zh-CN";
}

function readStoredLanguage(): UiLanguage {
  if (typeof window === "undefined") return "en";
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (isUiLanguage(stored)) return stored;
  const browserLanguage = window.navigator.language.toLowerCase();
  return browserLanguage.startsWith("zh") ? "zh-CN" : "en";
}

function writeStoredLanguage(language: UiLanguage) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(STORAGE_KEY, language);
}

const language = shallowRef<UiLanguage>(readStoredLanguage());
let syncStarted = false;

function ensureLanguageSync() {
  if (syncStarted) return;
  syncStarted = true;

  watch(
    language,
    (next) => {
      writeStoredLanguage(next);
    },
    { immediate: true },
  );

  if (typeof window === "undefined") return;
  window.addEventListener("storage", (event) => {
    if (event.key !== STORAGE_KEY || !isUiLanguage(event.newValue)) return;
    language.value = event.newValue;
  });
}

export function setUiLanguage(next: UiLanguage) {
  language.value = next;
}

export function useUiLanguage() {
  ensureLanguageSync();
  return {
    language: readonly(language),
    languageOptions: LANGUAGE_OPTIONS,
    setUiLanguage,
  };
}

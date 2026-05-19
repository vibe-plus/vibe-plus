import { isRef } from "vue";
import type { RouteLocationNormalized } from "vue-router";
import { i18n } from "../dashboard/i18n.ts";

const BRAND = "Vibe+";

const HOME_TITLE = {
  en: `${BRAND} · The companion for vibe coding`,
  "zh-CN": `${BRAND} · Vibe Coding 最佳伴侣`,
} as const;

const ROUTE_PAGE_TITLE: Record<string, { en: string; "zh-CN": string }> = {
  overview: { en: "Overview", "zh-CN": "概览" },
  providers: { en: "Providers", "zh-CN": "供应商" },
  settings: { en: "Settings", "zh-CN": "设置" },
};

function currentLocale(): keyof typeof HOME_TITLE {
  const locale = i18n.global.locale;
  const value = isRef(locale) ? locale.value : locale;
  return value === "zh-CN" ? "zh-CN" : "en";
}

export function syncDocumentTitle(to: RouteLocationNormalized) {
  if (typeof document === "undefined") return;

  const locale = currentLocale();

  if (to.path === "/") {
    document.title = HOME_TITLE[locale];
    return;
  }

  const routeName = typeof to.name === "string" ? to.name : null;
  const page = routeName ? ROUTE_PAGE_TITLE[routeName] : null;
  document.title = page ? `${page[locale]} · ${BRAND}` : HOME_TITLE[locale];
}

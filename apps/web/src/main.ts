import "./dashboard/assets/tailwind.css";
import "./dashboard/assets/global.css";
import { createApp, watch } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { redirectToFastestCDN } from "./lib/cdn-probe.ts";
import { router } from "./router.ts";
import { useBrandLogo } from "./dashboard/composables/use-brand-logo.ts";
import { useUiLanguage } from "./dashboard/composables/use-ui-language.ts";
import { i18n, syncI18nWithUiLanguage } from "./dashboard/i18n.ts";
import { syncDocumentTitle } from "./lib/page-title.ts";

const { hostname } = window.location;
if (hostname === "vibe-plus.github.io" || hostname === "vibe-plus.cheez.tech") {
  void redirectToFastestCDN();
}

syncI18nWithUiLanguage();
useBrandLogo();

const { language } = useUiLanguage();
watch(
  () => language.value,
  () => {
    syncDocumentTitle(router.currentRoute.value);
  },
);

createApp(App).use(createPinia()).use(router).use(i18n).mount("#app");
router.isReady().then(() => {
  syncDocumentTitle(router.currentRoute.value);
});

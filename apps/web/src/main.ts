import "./dashboard/assets/tailwind.css";
import "./dashboard/assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { redirectToFastestCDN } from "./lib/cdn-probe.ts";
import { router } from "./router.ts";
import { i18n, syncI18nWithUiLanguage } from "./dashboard/i18n.ts";

const { hostname } = window.location;
if (hostname === "vibe-plus.github.io" || hostname === "vibe-plus.cheez.tech") {
  void redirectToFastestCDN();
}

syncI18nWithUiLanguage();

createApp(App).use(createPinia()).use(router).use(i18n).mount("#app");

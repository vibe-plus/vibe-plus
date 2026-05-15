import "@unocss/reset/tailwind.css";
import "uno.css";
import "./dashboard/assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { redirectToFastestCDN } from "./lib/cdn-probe.ts";
import { router } from "./router.ts";

const { hostname } = window.location;
if (hostname === "vibe-plus.github.io" || hostname === "vibe-plus.cheez.tech") {
  void redirectToFastestCDN();
}

createApp(App).use(createPinia()).use(router).mount("#app");

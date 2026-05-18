import "./assets/tailwind.css";
import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "../router.ts";
import { i18n, syncI18nWithUiLanguage } from "./i18n.ts";

syncI18nWithUiLanguage();

createApp(App).use(createPinia()).use(router).use(i18n).mount("#app");

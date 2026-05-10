import "@unocss/reset/tailwind.css";
import "uno.css";
import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router.ts";

createApp(App).use(createPinia()).use(router).mount("#app");

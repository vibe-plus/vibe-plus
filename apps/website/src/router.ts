import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/dashboard" },
    { path: "/dashboard", component: () => import("./pages/Dashboard.vue") },
    { path: "/providers", component: () => import("./pages/Providers.vue") },
    { path: "/routes", component: () => import("./pages/Routes.vue") },
    { path: "/logs", component: () => import("./pages/Logs.vue") },
    { path: "/usage", component: () => import("./pages/Usage.vue") },
    { path: "/settings", component: () => import("./pages/Settings.vue") },
  ],
});

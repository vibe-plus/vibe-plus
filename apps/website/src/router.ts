import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/dashboard" },
    {
      path: "/dashboard",
      name: "dashboard",
      component: () => import("./pages/Dashboard.vue"),
    },
    {
      path: "/providers",
      name: "providers",
      component: () => import("./pages/Providers.vue"),
    },
    { path: "/routes", name: "routes", component: () => import("./pages/Routes.vue") },
    { path: "/logs", name: "logs", component: () => import("./pages/Logs.vue") },
    { path: "/usage", name: "usage", component: () => import("./pages/Usage.vue") },
    {
      path: "/settings",
      name: "settings",
      component: () => import("./pages/Settings.vue"),
    },
  ],
});

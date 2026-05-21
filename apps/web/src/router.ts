import { createRouter, createWebHistory } from "vue-router";
import LanderPage from "./lander/LanderPage.vue";
import DashboardShell from "./dashboard/App.vue";
import { syncDocumentTitle } from "./lib/page-title.ts";

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: "/", component: LanderPage },
    { path: "/providers", redirect: "/ui/providers" },
    { path: "/logs", redirect: "/ui/overview" },
    { path: "/monitor", redirect: "/ui/overview" },
    { path: "/settings", redirect: "/ui/settings" },
    { path: "/overview", redirect: "/ui/overview" },
    { path: "/routes", redirect: "/ui/providers" },
    {
      path: "/ui",
      component: DashboardShell,
      redirect: "/ui/overview",
      children: [
        {
          path: "overview",
          name: "overview",
          component: () => import("./dashboard/pages/Overview.vue"),
        },
        { path: "codex", redirect: { path: "/ui/overview", query: { view: "codex" } } },
        { path: "claude", redirect: { path: "/ui/overview", query: { view: "claude" } } },
        {
          path: "providers",
          name: "providers",
          component: () => import("./dashboard/pages/Providers.vue"),
        },
        {
          path: "observability",
          name: "observability",
          component: () => import("./dashboard/features/observability/Observability.vue"),
        },
        { path: "routes", redirect: "/ui/providers" },
        { path: "monitor", redirect: "/ui/observability" },
        { path: "logs", redirect: "/ui/observability" },
        {
          path: "settings",
          name: "settings",
          component: () => import("./dashboard/pages/Settings.vue"),
        },
      ],
    },
  ],
});

router.afterEach((to) => {
  syncDocumentTitle(to);
});

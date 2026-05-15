import { createRouter, createWebHistory } from "vue-router";
import LanderPage from "./lander/LanderPage.vue";
import DashboardShell from "./dashboard/App.vue";

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: "/", component: LanderPage },
    { path: "/providers", redirect: "/ui/providers" },
    { path: "/statistics", redirect: "/ui/statistics" },
    { path: "/usage", redirect: "/ui/statistics" },
    { path: "/logs", redirect: "/ui/monitor" },
    { path: "/monitor", redirect: "/ui/monitor" },
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
        { path: "routes", redirect: "/ui/providers" },
        {
          path: "statistics",
          name: "statistics",
          component: () => import("./dashboard/pages/Usage.vue"),
        },
        { path: "usage", redirect: "/ui/statistics" },
        {
          path: "monitor",
          name: "monitor",
          component: () => import("./dashboard/pages/Monitor.vue"),
        },
        { path: "logs", redirect: "/ui/monitor" },
        {
          path: "settings",
          name: "settings",
          component: () => import("./dashboard/pages/Settings.vue"),
        },
      ],
    },
  ],
});

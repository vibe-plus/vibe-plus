<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { RouterView, RouterLink, useRoute, useRouter } from "vue-router";
import { useProxyStatus } from "./composables/useProxy.ts";
import { useBrandLogo } from "./composables/use-brand-logo.ts";
import VpIcon from "./components/vp-icon.vue";
import type { vp_icon_name } from "./components/vp-icon.vue";
import { workspaceViewFromQuery, type WorkspaceView } from "./utils/workspace-view.ts";
import { useWebCompatibility } from "./composables/use-web-compatibility.ts";
import BrandWordmark from "../components/brand-wordmark.vue";

const { online, status } = useProxyStatus();
const route = useRoute();
const router = useRouter();
const { currentBrandLogo } = useBrandLogo();
const { t } = useI18n();

type SidebarView = {
  id: WorkspaceView;
  labelKey: string;
  icon: vp_icon_name;
  iconClass?: string;
  badge?: string;
};

const views: SidebarView[] = [
  { id: "overview", labelKey: "views.all", icon: "layout-dashboard" },
  {
    id: "codex",
    labelKey: "views.codex",
    icon: "terminal",
    iconClass: "i-[lobe--codex-color]",
  },
  {
    id: "claude",
    labelKey: "views.claude",
    icon: "sparkles",
    iconClass: "i-[lobe--claude-color]",
    badge: "EXP",
  },
];

const topTabs: { to: string; labelKey: string; icon: vp_icon_name; scoped?: boolean }[] = [
  { to: "/ui/overview", labelKey: "tabs.overview", icon: "layout-dashboard", scoped: true },
  { to: "/ui/providers", labelKey: "tabs.providers", icon: "server", scoped: true },
  { to: "/ui/settings", labelKey: "tabs.settings", icon: "settings", scoped: true },
];

const currentView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const viewPrefix = computed(() => {
  if (currentView.value === "codex") return "Codex";
  if (currentView.value === "claude") return "Claude";
  return "";
});

const webCompatibility = useWebCompatibility(online, status);

const allLabel = computed(() => t("views.all"));
const codexLabel = computed(() => t("views.codex"));
const claudeLabel = computed(() => t("views.claude"));
const viewLabel = computed<Record<WorkspaceView, string>>(() => ({
  overview: allLabel.value,
  codex: codexLabel.value,
  claude: claudeLabel.value,
}));
const viewTitle = computed<Record<WorkspaceView, string>>(() => ({
  overview: viewLabel.value.overview,
  codex: viewLabel.value.codex,
  claude: t("views.claudeExperimental", { label: viewLabel.value.claude }),
}));

function isActive(to: string): boolean {
  return route.path === to || route.path.startsWith(to + "/");
}

function tabTo(item: (typeof topTabs)[number]) {
  if (!item.scoped) return item.to;
  return currentView.value === "overview"
    ? item.to
    : { path: item.to, query: { view: currentView.value } };
}

function setView(view: WorkspaceView) {
  const query = { ...route.query };
  if (view === "overview") delete query.view;
  else query.view = view;
  void router.push({ path: "/ui/overview", query });
}

function tabLabel(item: (typeof topTabs)[number]) {
  if (viewPrefix.value && item.labelKey === "tabs.overview") return viewPrefix.value;
  return t(item.labelKey);
}
</script>

<template>
  <div class="layout-shell">
    <div
      v-if="!webCompatibility.ok"
      class="fixed left-1/2 top-3 z-[80] w-[min(42rem,calc(100vw-1.5rem))] -translate-x-1/2 rounded-2xl border border-amber-300/70 bg-amber-50/95 px-4 py-3 text-sm text-amber-950 shadow-2xl shadow-amber-900/10 backdrop-blur"
      role="alert"
    >
      <div class="flex items-start gap-3">
        <VpIcon name="alert-triangle" class="mt-0.5 size-4 shrink-0 text-amber-600" />
        <div class="min-w-0">
          <p class="font-semibold">{{ t("compat.updateRequired") }}</p>
          <p class="mt-0.5 text-xs leading-5 text-amber-900/85">{{ webCompatibility.message }}</p>
          <code
            class="mt-2 inline-flex rounded-lg border border-amber-300/60 bg-white/70 px-2 py-1 text-[11px] text-amber-950"
            >npm install -g @vibe-plus/cli@latest</code
          >
        </div>
      </div>
    </div>
    <aside
      class="nav-aside noise-overlay w-full sm:w-[4.25rem] md:w-56 transition-[width] duration-200"
    >
      <div
        class="relative z-10 flex min-w-0 shrink-0 items-center justify-between gap-2 px-3 py-2 sm:hidden"
      >
        <RouterLink
          to="/"
          class="shrink-0 rounded-[0.6rem] outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
          :aria-label="t('nav.home')"
          :title="t('nav.home')"
        >
          <img
            class="brand-mark--image brand-mark--image-mobile shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]"
            :src="currentBrandLogo.src"
            :alt="currentBrandLogo.label"
          />
        </RouterLink>
        <div class="min-w-0 flex-1">
          <BrandWordmark class="brand-wordmark-mobile block truncate text-vp-text" />
        </div>
        <div
          class="flex items-center gap-1.5 rounded-full border border-vp-border/70 bg-vp-surface/80 px-2 py-1 text-[10px] text-vp-muted"
        >
          <span
            :class="
              online ? 'bg-emerald-500 shadow-emerald-500/30' : 'bg-red-500 shadow-red-500/30'
            "
            class="inline-block size-1.5 rounded-full shadow-sm shrink-0"
          />
          <span class="font-mono">{{ online ? (status?.port ?? "?") : t("status.offline") }}</span>
        </div>
      </div>

      <div
        class="hidden sm:block px-3 md:px-5 pt-5 md:pt-6 pb-4 md:pb-5 border-b border-vp-border relative z-10"
      >
        <div class="flex items-center justify-center md:justify-start gap-2.5">
          <RouterLink
            to="/"
            class="shrink-0 rounded-[0.8rem] outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
            :aria-label="t('nav.home')"
            :title="t('nav.home')"
          >
            <img
              class="brand-mark--image shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]"
              :src="currentBrandLogo.src"
              :alt="currentBrandLogo.label"
            />
          </RouterLink>
          <div class="min-w-0 hidden md:block">
            <BrandWordmark class="font-semibold text-[15px] tracking-tight text-vp-text" />
          </div>
        </div>
        <div class="flex items-center justify-center md:justify-start gap-2 mt-3 px-0.5 flex-wrap">
          <span
            :class="
              online ? 'bg-emerald-500 shadow-emerald-500/30' : 'bg-red-500 shadow-red-500/30'
            "
            class="inline-block size-1.5 rounded-full shadow-lg live-dot shrink-0"
          />
          <span
            class="hidden md:inline text-[11px] text-vp-muted font-mono font-medium tracking-wide truncate"
          >
            {{ online ? t("status.port", { port: status?.port ?? "?" }) : t("status.offline") }}
          </span>
          <span
            v-if="online && status?.version"
            class="hidden xl:inline text-[10px] text-vp-muted font-mono ml-auto truncate max-w-[5rem]"
            >{{ status.version }}</span
          >
        </div>
        <p
          class="mt-2 text-center md:text-left text-[10px] text-vp-muted font-mono md:hidden"
          :title="online ? t('status.port', { port: status?.port ?? '?' }) : t('status.offline')"
        >
          {{ online ? (status?.port ?? "?") : "—" }}
        </p>
      </div>

      <nav
        class="relative z-20 mx-2 mb-2 grid grid-cols-3 gap-0.75 rounded-[1.1rem] border border-white/55 bg-[linear-gradient(180deg,color-mix(in_srgb,var(--vp-surface)_72%,white)_0%,color-mix(in_srgb,var(--vp-surface)_58%,white)_100%)] p-1 shadow-[0_8px_24px_color-mix(in_srgb,var(--vp-text)_8%,transparent),inset_0_1px_0_rgba(255,255,255,0.55)] backdrop-blur-xl sm:mx-0 sm:mb-0 sm:flex-1 sm:block sm:space-y-1 sm:rounded-none sm:border-0 sm:bg-transparent sm:p-0 sm:shadow-none"
        :aria-label="t('nav.view')"
      >
        <button
          v-for="item in views"
          :key="item.id"
          type="button"
          :aria-label="viewTitle[item.id]"
          :title="viewTitle[item.id]"
          class="nav-link group relative h-10 min-w-0 rounded-[0.95rem] px-0.5 sm:h-auto sm:w-full sm:rounded-none sm:flex-col sm:gap-1 sm:px-0 md:flex-row md:justify-start md:gap-3 md:px-3 outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
          :class="
            currentView === item.id
              ? 'nav-link--active text-[color-mix(in_srgb,var(--vp-primary)_34%,var(--vp-text))]'
              : 'nav-link--idle'
          "
          @click="setView(item.id)"
        >
          <span
            class="nav-icon size-6.5 md:size-7 shrink-0"
            :class="currentView === item.id ? 'nav-icon--active' : 'nav-icon--idle'"
          >
            <span
              v-if="item.iconClass"
              :class="[item.iconClass, 'size-[1.25rem]']"
              aria-hidden="true"
            />
            <VpIcon v-else :name="item.icon" size-class="size-[1.05rem] md:size-[1.125rem]" />
          </span>
          <span
            class="hidden min-w-0 flex-1 items-center gap-1.5 truncate text-left md:inline-flex"
          >
            <span class="truncate">{{ viewLabel[item.id] }}</span>
            <span
              v-if="item.badge"
              class="rounded-full border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[9px] font-bold leading-none tracking-wide text-amber-700"
            >
              {{ item.badge }}
            </span>
          </span>
        </button>
      </nav>
    </aside>

    <main class="main-canvas">
      <div class="relative z-10 p-2 pb-[4.5rem] sm:p-3 sm:pb-3 lg:p-4 xl:p-5 max-w-[96rem] mx-auto">
        <div
          class="page-surface p-2.5 sm:p-3 lg:p-4 min-h-[calc(100dvh-1rem)] sm:min-h-[calc(100dvh-1.5rem)]"
        >
          <nav
            class="top-tabs-desktop top-3 mb-3 lg:top-4 xl:top-5"
            :aria-label="t('nav.toolNavigation')"
          >
            <RouterLink
              v-for="item in topTabs"
              :key="item.to"
              :to="tabTo(item)"
              class="top-tab"
              :class="isActive(item.to) ? 'top-tab--active' : 'top-tab--idle'"
              :title="tabLabel(item)"
              :aria-label="tabLabel(item)"
            >
              <VpIcon :name="item.icon" size-class="size-4" />
              <span class="truncate">{{ tabLabel(item) }}</span>
            </RouterLink>
          </nav>
          <RouterView />
        </div>
      </div>
    </main>

    <nav class="mobile-bottom-bar sm:hidden" :aria-label="t('nav.toolNavigation')">
      <RouterLink
        v-for="item in topTabs"
        :key="`mobile-${item.to}`"
        :to="tabTo(item)"
        class="mobile-bottom-tab"
        :class="isActive(item.to) ? 'top-tab--active' : 'top-tab--idle'"
        :title="tabLabel(item)"
        :aria-label="tabLabel(item)"
      >
        <VpIcon :name="item.icon" size-class="size-4" />
        <span class="text-[11px] leading-none truncate max-w-[4.2rem]">{{ tabLabel(item) }}</span>
      </RouterLink>
    </nav>
  </div>
</template>

<style scoped>
.brand-mark--image {
  display: inline-block;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 0.8rem;
  overflow: hidden;
  object-fit: contain;
}

.brand-mark--image-mobile {
  width: 1.75rem;
  height: 1.75rem;
  border-radius: 0.6rem;
}

.brand-wordmark-mobile {
  font-size: 1.02rem;
  font-weight: 900;
  line-height: 1;
  letter-spacing: 0;
}
</style>

<i18n lang="json">
{
  "en": {
    "compat": {
      "updateRequired": "Vibe Plus CLI update required"
    },
    "nav": {
      "home": "Back to landing page",
      "view": "View",
      "toolNavigation": "Tool navigation"
    },
    "status": {
      "offline": "offline",
      "port": "port {port}"
    },
    "tabs": {
      "overview": "Overview",
      "providers": "Providers",
      "settings": "Settings"
    },
    "views": {
      "all": "All",
      "codex": "Codex",
      "claude": "Claude",
      "claudeExperimental": "{label} · Experimental"
    }
  },
  "zh-CN": {
    "compat": {
      "updateRequired": "需要更新 Vibe Plus CLI"
    },
    "nav": {
      "home": "返回落地页",
      "view": "视图",
      "toolNavigation": "工具导航"
    },
    "status": {
      "offline": "离线",
      "port": "端口 {port}"
    },
    "tabs": {
      "overview": "概览",
      "providers": "供应商",
      "settings": "设置"
    },
    "views": {
      "all": "全部",
      "codex": "Codex",
      "claude": "Claude",
      "claudeExperimental": "{label} · 实验性"
    }
  }
}
</i18n>

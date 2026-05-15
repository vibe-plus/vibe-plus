<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { RouterView, RouterLink, useRoute, useRouter } from "vue-router";
import { useProxyStatus } from "./composables/useProxy.ts";
import { useSmartIntake } from "./composables/use-smart-intake.ts";
import { useIntakeFlow } from "./composables/use-intake-flow.ts";
import { useBrandLogo } from "./composables/use-brand-logo.ts";
import VpIcon from "./components/vp-icon.vue";
import type { vp_icon_name } from "./components/vp-icon.vue";
import SmartIntake from "./components/SmartIntake.vue";
import IntakeWizard from "./components/intake-wizard.vue";
import { useLiveWorkspaceActivity } from "./composables/use-live-workspace-activity.ts";
import { workspaceViewFromQuery, type WorkspaceView } from "./utils/workspace-view.ts";
import { useWebCompatibility } from "./composables/use-web-compatibility.ts";

const { online, status } = useProxyStatus();
const route = useRoute();
const router = useRouter();
const { currentBrandLogo } = useBrandLogo();
const intakeFlow = useIntakeFlow();
const {
  items: intakeItems,
  authItems: intakeAuthItems,
  configItems: intakeConfigItems,
  ccsProfileItems: intakeCcsProfileItems,
  dragActive: intakeDragActive,
  dragIntent: intakeDragIntent,
  busy: intakeBusy,
  message: intakeMessage,
  error: intakeError,
  clipboardWatch: intakeClipboardWatch,
  clipboardWatchAvailable: intakeClipboardWatchAvailable,
  readClipboard,
  toggleClipboardWatch,
  importCcsProfile,
  saveCodexConfig,
  goCodex,
  dismiss: dismissIntake,
} = useSmartIntake({
  onAuthLikeRecognized: (items) => intakeFlow.handlePaste(items),
});

function openIntakeWizardFromCard() {
  intakeFlow.openWithCandidates(intakeItems.value);
  dismissIntake();
}

type SidebarView = {
  id: WorkspaceView;
  label: string;
  icon: vp_icon_name;
  iconClass?: string;
  badge?: string;
};

const views: SidebarView[] = [
  { id: "overview", label: "All", icon: "layout-dashboard" },
  {
    id: "codex",
    label: "Codex",
    icon: "terminal",
    iconClass: "i-lobe-codex-color",
  },
  {
    id: "claude",
    label: "Claude",
    icon: "sparkles",
    iconClass: "i-lobe-claude-color",
    badge: "EXP",
  },
];

const activityBadgeClass: Record<WorkspaceView, string> = {
  overview:
    "border-[color-mix(in_srgb,var(--vp-primary)_22%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-text))]",
  codex:
    "border-[color-mix(in_srgb,#111827_18%,var(--vp-border))] bg-[color-mix(in_srgb,#111827_6%,var(--vp-surface))] text-[color-mix(in_srgb,#111827_72%,var(--vp-text))]",
  claude:
    "border-[color-mix(in_srgb,#d97757_28%,var(--vp-border))] bg-[color-mix(in_srgb,#d97757_10%,var(--vp-surface))] text-[color-mix(in_srgb,#c15f3f_70%,var(--vp-text))]",
};

const topTabs: { to: string; label: string; icon: vp_icon_name; scoped?: boolean }[] = [
  { to: "/ui/overview", label: "Overview", icon: "layout-dashboard", scoped: true },
  { to: "/ui/providers", label: "Providers", icon: "server", scoped: true },
  { to: "/ui/statistics", label: "Statistics", icon: "pie-chart", scoped: true },
  { to: "/ui/monitor", label: "Monitor", icon: "activity", scoped: true },
  { to: "/ui/settings", label: "Settings", icon: "settings", scoped: true },
];

const currentView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const { activeCounts } = useLiveWorkspaceActivity();
const trafficPhase = ref(0);
let trafficTimer: number | null = null;

const viewPrefix = computed(() => {
  if (currentView.value === "codex") return "Codex";
  if (currentView.value === "claude") return "Claude";
  return "";
});

const webCompatibility = useWebCompatibility(online, status);

const hasAnyScopedActivity = computed(
  () => activeCounts.value.codex > 0 || activeCounts.value.claude > 0,
);

const mobileViewBadge = computed(() => {
  if (currentView.value === "codex") return activeCounts.value.codex;
  if (currentView.value === "claude") return activeCounts.value.claude;
  return hasAnyScopedActivity.value ? activeCounts.value.codex + activeCounts.value.claude : 0;
});
const allBusy = computed(() => activeCounts.value.overview > 0);
const codexBusy = computed(() => activeCounts.value.codex > 0);
const claudeBusy = computed(() => activeCounts.value.claude > 0);

function animateBusyLabel(label: string) {
  if (!label) return label;
  const chars = Array.from(label.toLowerCase());
  const activeIndex = trafficPhase.value % chars.length;
  chars[activeIndex] = chars[activeIndex].toUpperCase();
  return chars.join("");
}

const allLabel = computed(() => (allBusy.value ? animateBusyLabel("Alling") : "All"));
const codexLabel = computed(() => (codexBusy.value ? animateBusyLabel("Codexing") : "Codex"));
const claudeLabel = computed(() => (claudeBusy.value ? animateBusyLabel("Clauding") : "Claude"));
const viewLabel = computed<Record<WorkspaceView, string>>(() => ({
  overview: allLabel.value,
  codex: codexLabel.value,
  claude: claudeLabel.value,
}));
const viewTitle = computed<Record<WorkspaceView, string>>(() => ({
  overview: viewLabel.value.overview,
  codex: viewLabel.value.codex,
  claude: `${viewLabel.value.claude} · Experimental`,
}));

onMounted(() => {
  trafficTimer = window.setInterval(() => {
    trafficPhase.value += 1;
  }, 180);
});

onBeforeUnmount(() => {
  if (trafficTimer !== null) {
    window.clearInterval(trafficTimer);
    trafficTimer = null;
  }
});

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

function tabLabel(label: string) {
  if (!viewPrefix.value) return label;
  if (label === "Overview") return viewPrefix.value;
  return label;
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
          <p class="font-semibold">Vibe CLI update required</p>
          <p class="mt-0.5 text-xs leading-5 text-amber-900/85">{{ webCompatibility.message }}</p>
          <code
            class="mt-2 inline-flex rounded-lg border border-amber-300/60 bg-white/70 px-2 py-1 text-[11px] text-amber-950"
            >npm install -g @vibe-plus/cli@latest</code
          >
        </div>
      </div>
    </div>
    <aside
      class="nav-aside noise-overlay w-full sm:w-[4.25rem] lg:w-56 transition-[width] duration-200"
    >
      <div
        class="relative z-10 flex min-w-0 shrink-0 items-center justify-between gap-2 px-3 py-2 sm:hidden"
      >
        <img
          class="brand-mark--image brand-mark--image-mobile shrink-0 shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]"
          :src="currentBrandLogo.src"
          :alt="currentBrandLogo.label"
        />
        <div class="min-w-0 flex-1">
          <span class="brand-wordmark-mobile block truncate text-vp-text">Vibe Plus</span>
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
          <span class="font-mono">{{ online ? (status?.port ?? "?") : "offline" }}</span>
        </div>
      </div>

      <div
        class="hidden sm:block px-3 lg:px-5 pt-5 lg:pt-6 pb-4 lg:pb-5 border-b border-vp-border relative z-10"
      >
        <div class="flex items-center justify-center lg:justify-start gap-2.5">
          <img
            class="brand-mark--image shrink-0 shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]"
            :src="currentBrandLogo.src"
            :alt="currentBrandLogo.label"
          />
          <div class="min-w-0 hidden lg:block">
            <span class="font-semibold text-[15px] tracking-tight text-vp-text">Vibe Plus</span>
          </div>
        </div>
        <div class="flex items-center justify-center lg:justify-start gap-2 mt-3 px-0.5 flex-wrap">
          <span
            :class="
              online ? 'bg-emerald-500 shadow-emerald-500/30' : 'bg-red-500 shadow-red-500/30'
            "
            class="inline-block size-1.5 rounded-full shadow-lg live-dot shrink-0"
          />
          <span
            class="hidden lg:inline text-[11px] text-vp-muted font-mono font-medium tracking-wide truncate"
          >
            {{ online ? `port ${status?.port ?? "?"}` : "offline" }}
          </span>
          <span
            v-if="online && status?.version"
            class="hidden xl:inline text-[10px] text-vp-muted font-mono ml-auto truncate max-w-[5rem]"
            >{{ status.version }}</span
          >
        </div>
        <p
          class="mt-2 text-center lg:text-left text-[10px] text-vp-muted font-mono lg:hidden"
          :title="online ? `port ${status?.port ?? '?'}` : 'offline'"
        >
          {{ online ? (status?.port ?? "?") : "—" }}
        </p>
      </div>

      <nav
        class="relative z-20 mx-2 mb-2 grid grid-cols-3 gap-0.75 rounded-[1.1rem] border border-white/55 bg-[linear-gradient(180deg,color-mix(in_srgb,var(--vp-surface)_72%,white)_0%,color-mix(in_srgb,var(--vp-surface)_58%,white)_100%)] p-1 shadow-[0_8px_24px_color-mix(in_srgb,var(--vp-text)_8%,transparent),inset_0_1px_0_rgba(255,255,255,0.55)] backdrop-blur-xl sm:mx-0 sm:mb-0 sm:flex-1 sm:block sm:space-y-1 sm:rounded-none sm:border-0 sm:bg-transparent sm:p-0 sm:shadow-none"
        aria-label="View"
      >
        <button
          v-for="item in views"
          :key="item.id"
          type="button"
          :aria-label="viewTitle[item.id]"
          :title="viewTitle[item.id]"
          class="nav-link group relative h-10 min-w-0 rounded-[0.95rem] px-0.5 sm:h-auto sm:w-full sm:rounded-none sm:flex-col sm:gap-1 sm:px-0 lg:flex-row lg:justify-start lg:gap-3 lg:px-3 outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
          :class="
            currentView === item.id
              ? 'nav-link--active text-[color-mix(in_srgb,var(--vp-primary)_34%,var(--vp-text))]'
              : 'nav-link--idle'
          "
          @click="setView(item.id)"
        >
          <span
            class="nav-icon size-6.5 lg:size-7 shrink-0"
            :class="currentView === item.id ? 'nav-icon--active' : 'nav-icon--idle'"
          >
            <span
              v-if="item.iconClass"
              :class="[item.iconClass, 'size-[1.25rem]']"
              aria-hidden="true"
            />
            <VpIcon v-else :name="item.icon" size-class="size-[1.05rem] lg:size-[1.125rem]" />
          </span>
          <span
            class="min-w-0 truncate text-[11px] leading-none font-semibold transition-[max-width,opacity,margin] duration-150 sm:hidden"
            :class="
              currentView === item.id ? 'max-w-[5rem] opacity-100' : 'max-w-[3.25rem] opacity-80'
            "
            :aria-hidden="currentView !== item.id"
          >
            {{ item.label }}
          </span>
          <span
            class="hidden min-w-0 flex-1 items-center gap-1.5 truncate text-left lg:inline-flex"
          >
            <span class="truncate">{{ viewLabel[item.id] }}</span>
            <span
              v-if="item.badge"
              class="rounded-full border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[9px] font-bold leading-none tracking-wide text-amber-700"
            >
              {{ item.badge }}
            </span>
          </span>
          <span
            v-if="item.badge"
            class="absolute bottom-0.5 right-1 rounded-full border border-amber-200 bg-amber-50 px-1 text-[8px] font-bold leading-none tracking-wide text-amber-700 sm:bottom-auto sm:top-7 lg:hidden"
          >
            {{ item.badge }}
          </span>
          <span
            v-if="activeCounts[item.id] > 0"
            class="absolute right-1.5 top-1.5 inline-flex h-4 min-w-[1.125rem] max-w-[2rem] items-center justify-center rounded-full border px-1 text-[10px] font-semibold leading-none shadow-[0_1px_2px_color-mix(in_srgb,var(--vp-text)_8%,transparent)] sm:right-1 sm:top-1 lg:static lg:ml-auto"
            :class="activityBadgeClass[item.id]"
            :title="`${activeCounts[item.id]} active`"
          >
            {{ activeCounts[item.id] > 99 ? "99+" : activeCounts[item.id] }}
          </span>
        </button>
        <span
          v-if="mobileViewBadge > 0"
          class="pointer-events-none absolute right-2 top-2 inline-flex h-4 min-w-[1.125rem] items-center justify-center rounded-full border border-white/70 bg-[color-mix(in_srgb,var(--vp-primary)_18%,white)] px-1 text-[10px] font-semibold leading-none text-[color-mix(in_srgb,var(--vp-primary)_62%,var(--vp-text))] shadow-[0_2px_8px_color-mix(in_srgb,var(--vp-primary)_22%,transparent)] sm:hidden"
          :title="`${mobileViewBadge} active`"
        >
          {{ mobileViewBadge > 99 ? "99+" : mobileViewBadge }}
        </span>
      </nav>
    </aside>

    <main class="main-canvas">
      <div class="relative z-10 p-2 pb-[4.5rem] sm:p-3 sm:pb-3 lg:p-4 xl:p-5 max-w-[96rem] mx-auto">
        <div
          class="page-surface p-2.5 sm:p-3 lg:p-4 min-h-[calc(100dvh-1rem)] sm:min-h-[calc(100dvh-1.5rem)]"
        >
          <nav class="top-tabs-desktop top-3 mb-3 lg:top-4 xl:top-5" aria-label="Tool navigation">
            <RouterLink
              v-for="item in topTabs"
              :key="item.to"
              :to="tabTo(item)"
              class="top-tab"
              :class="isActive(item.to) ? 'top-tab--active' : 'top-tab--idle'"
              :title="tabLabel(item.label)"
              :aria-label="tabLabel(item.label)"
            >
              <VpIcon :name="item.icon" size-class="size-4" />
              <span class="truncate">{{ tabLabel(item.label) }}</span>
            </RouterLink>
          </nav>
          <RouterView />
        </div>
      </div>
    </main>

    <nav class="mobile-bottom-bar sm:hidden" aria-label="Tool navigation">
      <RouterLink
        v-for="item in topTabs"
        :key="`mobile-${item.to}`"
        :to="tabTo(item)"
        class="mobile-bottom-tab"
        :class="isActive(item.to) ? 'top-tab--active' : 'top-tab--idle'"
        :title="tabLabel(item.label)"
        :aria-label="tabLabel(item.label)"
      >
        <VpIcon :name="item.icon" size-class="size-4" />
        <span class="text-[11px] leading-none truncate max-w-[4.2rem]">{{
          tabLabel(item.label)
        }}</span>
      </RouterLink>
    </nav>
    <SmartIntake
      :items="intakeItems"
      :auth-count="intakeAuthItems.length"
      :config-count="intakeConfigItems.length"
      :ccs-profile-count="intakeCcsProfileItems.length"
      :drag-active="intakeDragActive"
      :drag-intent="intakeDragIntent"
      :busy="intakeBusy"
      :message="intakeMessage"
      :error="intakeError"
      :clipboard-watch="intakeClipboardWatch"
      :clipboard-watch-available="intakeClipboardWatchAvailable"
      @read-clipboard="readClipboard"
      @toggle-clipboard-watch="toggleClipboardWatch"
      @open-intake-wizard="openIntakeWizardFromCard"
      @import-ccs-profile="importCcsProfile"
      @save-config="saveCodexConfig"
      @go-codex="goCodex"
      @dismiss="dismissIntake"
    />
    <IntakeWizard />
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

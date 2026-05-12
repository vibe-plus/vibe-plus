<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { RouterView, RouterLink, useRoute, useRouter } from "vue-router";
import { api, type ClientStatus } from "./api/client.ts";
import { useProxyStatus } from "./composables/useProxy.ts";
import { useSmartIntake } from "./composables/use-smart-intake.ts";
import VpIcon from "./components/vp-icon.vue";
import type { vp_icon_name } from "./components/vp-icon.vue";
import SmartIntake from "./components/SmartIntake.vue";
import { workspaceViewFromQuery, type WorkspaceView } from "./utils/workspace-view.ts";

const { online, status } = useProxyStatus();
const route = useRoute();
const router = useRouter();
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
  importCodexAuth,
  importCcsProfile,
  saveCodexConfig,
  goCodex,
  dismiss: dismissIntake,
} = useSmartIntake();

type SidebarView = {
  id: WorkspaceView;
  label: string;
  icon: vp_icon_name;
  iconClass?: string;
  takeoverClient?: "codex" | "claude";
};

const views: SidebarView[] = [
  { id: "overview", label: "All", icon: "layout-dashboard" },
  {
    id: "codex",
    label: "Codex",
    icon: "terminal",
    iconClass: "i-lobe-codex-color",
    takeoverClient: "codex",
  },
  {
    id: "claude",
    label: "Claude",
    icon: "sparkles",
    iconClass: "i-lobe-claude-color",
    takeoverClient: "claude",
  },
];

const topTabs: { to: string; label: string; icon: vp_icon_name; scoped?: boolean }[] = [
  { to: "/dashboard", label: "Dashboard", icon: "layout-dashboard", scoped: true },
  { to: "/providers", label: "Providers", icon: "server", scoped: true },
  { to: "/routes", label: "Routes", icon: "route", scoped: true },
  { to: "/logs", label: "Logs", icon: "file-text", scoped: true },
  { to: "/usage", label: "Usage", icon: "pie-chart", scoped: true },
  { to: "/settings", label: "Settings", icon: "settings", scoped: true },
];

const currentView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const takeoverStatus = shallowRef<Record<"codex" | "claude", ClientStatus | null>>({
  codex: null,
  claude: null,
});
const takeoverBusy = ref<Record<"codex" | "claude", boolean>>({ codex: false, claude: false });
const takeoverError = shallowRef<string | null>(null);
const codexingTick = ref(0);
const allRequestPulseUntil = ref(0);
const codexRequestPulseUntil = ref(0);
const lastAllRequestCounter = ref<number | null>(null);
const lastCodexRequestCounter = ref<number | null>(null);
let codexingTimer: ReturnType<typeof setInterval> | null = null;

const viewPrefix = computed(() => {
  if (currentView.value === "codex") return "Codex";
  if (currentView.value === "claude") return "Claude";
  return "";
});

const codexTakenOver = computed(() => takeoverStatus.value.codex?.taken_over ?? false);
const claudeTakenOver = computed(() => takeoverStatus.value.claude?.taken_over ?? false);
const takenOverByClient = computed<Record<"codex" | "claude", boolean>>(() => ({
  codex: codexTakenOver.value,
  claude: claudeTakenOver.value,
}));
const allBusy = computed(() => {
  return codexingTick.value >= 0 && Date.now() < allRequestPulseUntil.value;
});
const codexBusy = computed(() => {
  if (!online.value || !status.value) return false;
  return (status.value.codex_ws_active ?? 0) > 0 || Date.now() < codexRequestPulseUntil.value;
});
function animatedWord(word: string) {
  const letters = word.split("");
  const index = codexingTick.value % letters.length;
  letters[index] = letters[index].toUpperCase();
  return letters.join("");
}
const allLabel = computed(() => {
  if (!allBusy.value) return "All";
  return animatedWord("Alling");
});
const codexLabel = computed(() => {
  if (!codexTakenOver.value || !codexBusy.value) return "Codex";
  return animatedWord("Codexing");
});
const viewLabel = computed<Record<WorkspaceView, string>>(() => ({
  overview: allLabel.value,
  codex: codexLabel.value,
  claude: "Claude",
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
  void router.push({ path: "/dashboard", query });
}

function tabLabel(label: string) {
  if (!viewPrefix.value) return label;
  if (label === "Dashboard") return viewPrefix.value;
  return label;
}

function takeoverTitle(client: "codex" | "claude") {
  return (
    takeoverStatus.value[client]?.configured_base_url ?? takeoverError.value ?? `${client}:takeover`
  );
}

function onTakeoverClick(event: MouseEvent, client: "codex" | "claude") {
  event.stopPropagation();
  void setTakeover(client, !takenOverByClient.value[client]);
}

async function refreshTakeoverStatus() {
  try {
    const [codex, claude] = await Promise.all([
      api.clients.status("codex"),
      api.clients.status("claude"),
    ]);
    takeoverStatus.value = { codex, claude };
    takeoverError.value = null;
  } catch (err) {
    takeoverError.value = (err as Error).message || "takeover:status";
  }
}

async function setTakeover(client: "codex" | "claude", next: boolean) {
  takeoverBusy.value = { ...takeoverBusy.value, [client]: true };
  takeoverError.value = null;
  try {
    const result = next ? await api.clients.takeover(client) : await api.clients.restore(client);
    takeoverStatus.value = { ...takeoverStatus.value, [client]: result.status };
  } catch (err) {
    takeoverError.value = (err as Error).message || "takeover:update";
    await refreshTakeoverStatus();
  } finally {
    takeoverBusy.value = { ...takeoverBusy.value, [client]: false };
  }
}

watch(
  codexBusy,
  (active) => {
    if (active && codexingTimer == null) {
      codexingTimer = setInterval(() => {
        codexingTick.value += 1;
      }, 220);
      return;
    }
    if (!active && codexingTimer != null) {
      clearInterval(codexingTimer);
      codexingTimer = null;
      codexingTick.value = 0;
    }
  },
  { immediate: true },
);

watch(
  () =>
    (status.value?.codex_ws_requests_total ?? 0) + (status.value?.codex_http_responses_total ?? 0),
  (next) => {
    if (lastCodexRequestCounter.value == null) {
      lastCodexRequestCounter.value = next;
      return;
    }
    if (next > lastCodexRequestCounter.value) {
      codexRequestPulseUntil.value = Date.now() + 3500;
    }
    lastCodexRequestCounter.value = next;
  },
);

watch(
  () => status.value?.requests_last_hour ?? 0,
  (next) => {
    if (lastAllRequestCounter.value == null) {
      lastAllRequestCounter.value = next;
      return;
    }
    if (next > lastAllRequestCounter.value) {
      allRequestPulseUntil.value = Date.now() + 3500;
    }
    lastAllRequestCounter.value = next;
  },
);

onMounted(() => {
  void refreshTakeoverStatus();
});

onUnmounted(() => {
  if (codexingTimer) clearInterval(codexingTimer);
});
</script>

<template>
  <div class="layout-shell">
    <aside
      class="nav-aside noise-overlay w-full sm:w-[4.25rem] lg:w-56 transition-[width] duration-200"
    >
      <div
        class="hidden sm:block px-3 lg:px-5 pt-5 lg:pt-6 pb-4 lg:pb-5 border-b border-vp-border relative z-10"
      >
        <div class="flex items-center justify-center lg:justify-start gap-2.5">
          <span
            class="brand-mark--image i-vp-logo shrink-0 shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]"
            aria-hidden="true"
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
        class="flex flex-1 gap-1 overflow-x-auto px-2 py-2 sm:block sm:space-y-1 sm:overflow-visible sm:px-2 sm:py-3 lg:px-3 relative z-10"
        aria-label="视角"
      >
        <button
          v-for="item in views"
          :key="item.id"
          type="button"
          :aria-label="viewLabel[item.id]"
          :title="viewLabel[item.id]"
          class="nav-link group w-28 shrink-0 flex-row px-3 sm:w-full sm:flex-col sm:gap-1 sm:px-0 lg:flex-row lg:justify-start lg:gap-3 lg:px-3 outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
          :class="currentView === item.id ? 'nav-link--active' : 'nav-link--idle'"
          @click="setView(item.id)"
        >
          <span
            class="nav-icon size-8 lg:size-7 shrink-0"
            :class="currentView === item.id ? 'nav-icon--active' : 'nav-icon--idle'"
          >
            <span
              v-if="item.iconClass"
              :class="[item.iconClass, 'size-[1.25rem]']"
              aria-hidden="true"
            />
            <VpIcon v-else :name="item.icon" size-class="size-[1.05rem] lg:size-[1.125rem]" />
          </span>
          <span class="inline min-w-0 flex-1 truncate text-left sm:hidden lg:inline">{{
            viewLabel[item.id]
          }}</span>
          <span
            v-if="item.takeoverClient"
            class="hidden lg:inline-flex size-4 shrink-0 items-center justify-center rounded-full transition-colors"
            :class="
              takenOverByClient[item.takeoverClient]
                ? 'bg-sky-50 text-sky-700 ring-1 ring-sky-200'
                : 'text-vp-muted opacity-30 group-hover:opacity-100 group-hover:bg-vp-surface group-hover:ring-1 group-hover:ring-vp-border'
            "
            :title="takeoverTitle(item.takeoverClient)"
            role="switch"
            :aria-checked="takenOverByClient[item.takeoverClient]"
            @click="onTakeoverClick($event, item.takeoverClient)"
          >
            <span
              class="size-1.5 rounded-full"
              :class="
                item.takeoverClient === 'codex' && codexBusy
                  ? 'bg-emerald-500 animate-pulse'
                  : takenOverByClient[item.takeoverClient]
                    ? 'bg-sky-500'
                    : 'bg-vp-border'
              "
            />
          </span>
        </button>
      </nav>
    </aside>

    <main class="main-canvas">
      <div class="relative z-10 p-2 sm:p-3 lg:p-4 xl:p-5 max-w-[96rem] mx-auto">
        <div
          class="page-surface p-2.5 sm:p-3 lg:p-4 min-h-[calc(100dvh-1rem)] sm:min-h-[calc(100dvh-1.5rem)]"
        >
          <nav class="top-tabs mb-4" aria-label="工具导航">
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
              <span class="sr-only">{{ tabLabel(item.label) }}</span>
            </RouterLink>
          </nav>
          <RouterView />
        </div>
      </div>
    </main>
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
      @import-auth="importCodexAuth"
      @import-ccs-profile="importCcsProfile"
      @save-config="saveCodexConfig"
      @go-codex="goCodex"
      @dismiss="dismissIntake"
    />
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
</style>

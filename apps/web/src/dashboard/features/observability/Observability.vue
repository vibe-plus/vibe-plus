<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import {
  api,
  type Credential,
  type ObservabilityConversation,
  type Provider,
  type RealtimeAttempt,
  type RealtimeRequest,
  type RequestLog,
  type UpstreamAttemptLog,
} from "../../api/client.ts";
import { useRealtimeStream } from "../../composables/useRealtimeStream.ts";
import VpIcon from "../../components/vp-icon.vue";
import ObservabilitySidebar, {
  type SidebarSelection,
} from "./components/observability-sidebar.vue";
import ConversationDetail, { type DetailTab } from "./components/conversation-detail.vue";
import GlobalDetail from "./components/global-detail.vue";

const route = useRoute();
const router = useRouter();
const { t } = useI18n({ useScope: "global" });

const HISTORY_SIZE = 60;
const POLL_MS = 2_000;

const {
  snapshot: realtime,
  transport: realtimeTransport,
  history,
} = useRealtimeStream({ historySize: HISTORY_SIZE });

const conversations = ref<ObservabilityConversation[]>([]);
const conversationsLoading = ref(false);
const conversationsError = ref<string | null>(null);

const requests = ref<RequestLog[]>([]);
const attempts = ref<UpstreamAttemptLog[]>([]);
const providers = ref<Provider[]>([]);
const credentialsByProvider = ref<Record<string, Credential[]>>({});

let pollTimer: number | null = null;
let conversationsTimer: number | null = null;

// ── URL state ──────────────────────────────────────────────────────────────

type RouteFocus =
  | { kind: "request"; id: string }
  | { kind: "attempt"; id: string }
  | { kind: "wave"; id: string };

type SourceView = "all" | "codex" | "claude";

function queryString(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return "";
}

function sourceViewFromQuery(): SourceView {
  const view = queryString(route.query.view);
  if (view === "codex" || view === "claude") return view;
  return "all";
}

function selectionFromQuery(): SidebarSelection {
  const conv = queryString(route.query.conversation);
  if (conv) {
    const idx = conv.indexOf(":");
    if (idx > 0) {
      const source = conv.slice(0, idx);
      const id = conv.slice(idx + 1);
      if ((source === "codex" || source === "claude") && id) {
        return { kind: "conversation", source, conversationId: id };
      }
    }
  }
  const global = queryString(route.query.global);
  if (global === "logs" || global === "waveform" || global === "all") {
    return { kind: "global", view: global };
  }
  return { kind: "global", view: "all" };
}

function detailTabFromQuery(): DetailTab {
  const v = queryString(route.query.detail);
  if (
    v === "overview" ||
    v === "upstream" ||
    v === "downstream" ||
    v === "network" ||
    v === "attempts"
  ) {
    return v;
  }
  return "overview";
}

function focusFromQuery(): RouteFocus | null {
  const request = queryString(route.query.request);
  if (request) return { kind: "request", id: request };
  const attempt = queryString(route.query.attempt);
  if (attempt) return { kind: "attempt", id: attempt };
  const wave = queryString(route.query.wave);
  if (wave) return { kind: "wave", id: wave };
  return null;
}

const selection = ref<SidebarSelection>(selectionFromQuery());
const detailTab = ref<DetailTab>(detailTabFromQuery());
const routeFocus = computed(focusFromQuery);
const sourceView = computed(sourceViewFromQuery);
const visibleConversations = computed(() => {
  if (sourceView.value === "all") return conversations.value;
  return conversations.value.filter((conversation) => conversation.source === sourceView.value);
});

function selectionToQuery(sel: SidebarSelection): Record<string, string | undefined> {
  if (sel.kind === "conversation") {
    return { conversation: `${sel.source}:${sel.conversationId}`, global: undefined };
  }
  return { conversation: undefined, global: sel.view === "all" ? undefined : sel.view };
}

function syncQuery() {
  const next: Record<string, unknown> = {
    ...route.query,
    ...selectionToQuery(selection.value),
    request: undefined,
    attempt: undefined,
    wave: undefined,
  };
  if (selection.value.kind === "conversation") {
    next.detail = detailTab.value === "overview" ? undefined : detailTab.value;
  } else {
    next.detail = undefined;
  }
  for (const key of Object.keys(next)) {
    if (next[key] === undefined) delete next[key];
  }
  if (JSON.stringify(next) === JSON.stringify(route.query)) {
    return;
  }
  void router.replace({ path: route.path, query: next as Record<string, string> });
}

function applySelection(sel: SidebarSelection) {
  selection.value = sel;
  if (sel.kind === "conversation") {
    detailTab.value = detailTabFromQuery();
  }
  syncQuery();
}

watch(
  () => route.query,
  () => {
    const focus = focusFromQuery();
    if (focus) return;
    const next = selectionFromQuery();
    if (JSON.stringify(next) !== JSON.stringify(selection.value)) {
      selection.value = next;
    }
    const nextTab = detailTabFromQuery();
    if (nextTab !== detailTab.value) detailTab.value = nextTab;
  },
);

watch(detailTab, () => syncQuery());

// ── Data loading ──────────────────────────────────────────────────────────

async function loadConversations() {
  conversationsLoading.value = true;
  try {
    conversations.value = await api.observability.conversations();
    conversationsError.value = null;
  } catch (err) {
    conversationsError.value = err instanceof Error ? err.message : String(err);
  } finally {
    conversationsLoading.value = false;
  }
}

async function loadRecords() {
  try {
    const [attemptsRes, requestsRes] = await Promise.all([
      api.observability.networkAttempts({ limit: 500 }),
      api.observability.requests({ limit: 500 }),
    ]);
    attempts.value = attemptsRes;
    requests.value = requestsRes.items;
  } catch {
    // Ignore polling errors; sidebar error message handles the UX.
  }
}

async function loadEntities() {
  try {
    const [providersRes, credentialsRes] = await Promise.all([
      api.providers.list(),
      api.credentials.all(),
    ]);
    providers.value = providersRes;
    credentialsByProvider.value = credentialsRes;
  } catch {
    // Ignore; UI degrades gracefully.
  }
}

function startPolling() {
  stopPolling();
  pollTimer = window.setInterval(() => {
    if (document.visibilityState === "hidden") return;
    void loadRecords();
  }, POLL_MS);
  conversationsTimer = window.setInterval(() => {
    if (document.visibilityState === "hidden") return;
    void loadConversations();
  }, 10_000);
}
function stopPolling() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
  if (conversationsTimer !== null) {
    window.clearInterval(conversationsTimer);
    conversationsTimer = null;
  }
}

onMounted(() => {
  void loadConversations();
  void loadRecords();
  void loadEntities();
  startPolling();
});
onUnmounted(stopPolling);

// ── Conversation matching ──────────────────────────────────────────────────

function looksLikeCodexId(id: string): boolean {
  return id.startsWith("019");
}

function conversationSelectionForTrace(trace: {
  thread_id?: string | null;
  session_id?: string | null;
}): SidebarSelection | null {
  if (trace.thread_id) {
    return { kind: "conversation", source: "codex", conversationId: trace.thread_id };
  }
  if (trace.session_id) {
    return {
      kind: "conversation",
      source: looksLikeCodexId(trace.session_id) ? "codex" : "claude",
      conversationId: trace.session_id,
    };
  }
  return null;
}

function realtimeAttempts(): RealtimeAttempt[] {
  return (realtime.value?.active_requests ?? []).flatMap((request) => request.attempts ?? []);
}

function requestById(id: string): RequestLog | RealtimeRequest | null {
  return (
    requests.value.find((request) => request.id === id) ??
    realtime.value?.active_requests.find((request) => request.id === id) ??
    null
  );
}

function attemptById(id: string): UpstreamAttemptLog | RealtimeAttempt | null {
  return (
    attempts.value.find((attempt) => attempt.attempt_id === id) ??
    realtimeAttempts().find((attempt) => attempt.attempt_id === id) ??
    null
  );
}

function attemptByRequestId(id: string): UpstreamAttemptLog | RealtimeAttempt | null {
  return (
    attempts.value.find((attempt) => attempt.request_id === id) ??
    realtimeAttempts().find((attempt) => attempt.request_id === id) ??
    null
  );
}

function selectionForRouteFocus(focus: RouteFocus | null): {
  selection: SidebarSelection;
  detailTab: DetailTab;
} | null {
  if (!focus) return null;

  if (focus.kind === "request") {
    const request = requestById(focus.id);
    const attempt = attemptByRequestId(focus.id);
    const selection =
      (request ? conversationSelectionForTrace(request) : null) ??
      (attempt ? conversationSelectionForTrace(attempt) : null);
    return selection ? { selection, detailTab: "downstream" } : null;
  }

  if (focus.kind === "attempt") {
    const attempt = attemptById(focus.id);
    const selection = attempt ? conversationSelectionForTrace(attempt) : null;
    return selection ? { selection, detailTab: "attempts" } : null;
  }

  const requestId = focus.id.split("#", 1)[0];
  const request = requestById(requestId);
  const attempt = attemptByRequestId(requestId);
  const selection =
    (request ? conversationSelectionForTrace(request) : null) ??
    (attempt ? conversationSelectionForTrace(attempt) : null);
  return selection ? { selection, detailTab: "attempts" } : null;
}

const resolvedRouteFocus = computed(() => selectionForRouteFocus(routeFocus.value));

watch(resolvedRouteFocus, (next) => {
  if (!next) return;
  let changed = false;
  if (JSON.stringify(next.selection) !== JSON.stringify(selection.value)) {
    selection.value = next.selection;
    changed = true;
  }
  if (detailTab.value !== next.detailTab) {
    detailTab.value = next.detailTab;
    changed = true;
  }
  if (changed) syncQuery();
});

function requestMatchesConversation(
  request: RequestLog | RealtimeRequest,
  source: "codex" | "claude",
  conversationId: string,
): boolean {
  if (source === "codex") {
    if (request.thread_id === conversationId) return true;
    if (request.session_id === conversationId && looksLikeCodexId(conversationId)) return true;
    return false;
  }
  // claude
  return request.session_id === conversationId && !looksLikeCodexId(conversationId);
}

function attemptMatchesConversation(
  attempt: UpstreamAttemptLog | RealtimeAttempt,
  source: "codex" | "claude",
  conversationId: string,
): boolean {
  if (source === "codex") {
    if (attempt.thread_id === conversationId) return true;
    if (attempt.session_id === conversationId && looksLikeCodexId(conversationId)) return true;
    return false;
  }
  return attempt.session_id === conversationId && !looksLikeCodexId(conversationId);
}

const activeConversationKeys = computed(() => {
  const out = new Set<string>();
  for (const req of realtime.value?.active_requests ?? []) {
    if (req.thread_id) out.add(`codex:${req.thread_id}`);
    if (req.session_id) {
      const src = looksLikeCodexId(req.session_id) ? "codex" : "claude";
      out.add(`${src}:${req.session_id}`);
    }
  }
  return out;
});

const selectedConversation = computed<ObservabilityConversation | null>(() => {
  if (selection.value.kind !== "conversation") return null;
  const { source, conversationId } = selection.value;
  return (
    conversations.value.find(
      (c) => c.source === source && c.conversation_id === conversationId,
    ) ?? {
      // Synthetic seed when conversation isn't in the local-history index yet.
      source,
      conversation_id: conversationId,
      title: conversationId.slice(0, 14),
      project_path: null,
      project_name: null,
      updated_at: 0,
      status: "no-data" as const,
      request_count: 0,
      attempt_count: 0,
      latest_request_id: null,
      preview: "",
      estimated_cost_usd: "",
      input_tokens: 0,
      output_tokens: 0,
      archived: false,
      parent_conversation_id: null,
      thread_kind: "user" as const,
      agent_nickname: null,
      local_tokens_used: 0,
      local_estimated_cost_usd: "",
      models_used: [],
      provider_ids: [],
      credential_ids: [],
      duration_seconds: 0,
    }
  );
});

const scopedRealtimeRequests = computed(() => {
  if (selection.value.kind !== "conversation") return [];
  const { source, conversationId } = selection.value;
  return (realtime.value?.active_requests ?? []).filter((r) =>
    requestMatchesConversation(r, source, conversationId),
  );
});

const scopedRealtimeAttempts = computed<RealtimeAttempt[]>(() =>
  scopedRealtimeRequests.value.flatMap((r) => r.attempts ?? []),
);

const scopedRequests = computed(() => {
  if (selection.value.kind !== "conversation") return [];
  const { source, conversationId } = selection.value;
  return requests.value.filter((r) => requestMatchesConversation(r, source, conversationId));
});

const scopedAttempts = computed(() => {
  if (selection.value.kind !== "conversation") return [];
  const { source, conversationId } = selection.value;
  return attempts.value.filter((a) => attemptMatchesConversation(a, source, conversationId));
});

// ── Responsive master-detail nav ────────────────────────────────────────────

// On small screens we present a master-detail flow:
//  - default: show the list (sidebar)
//  - after the user picks a conversation or global view: show the detail
//  - "Back" returns to the list
// On md+, both are visible side-by-side.
// Default: if the URL boots with a conversation already selected, jump into
// the detail pane directly so a refreshed deep-link works on mobile.
const mobileShowDetail = ref(selection.value.kind === "conversation");

watch(selection, (next, prev) => {
  // Drill in only on an actual user click — not on the boot-time URL parse.
  if (prev && JSON.stringify(prev) !== JSON.stringify(next)) {
    mobileShowDetail.value = true;
  }
});

const showDetailPane = computed(() => mobileShowDetail.value);

function goBack() {
  mobileShowDetail.value = false;
}
</script>

<template>
  <div
    class="relative flex h-[calc(100dvh-7rem)] min-h-[28rem] overflow-hidden rounded-lg border border-vp-border bg-vp-surface"
  >
    <!-- Sidebar: < md acts as the "list page" of master-detail navigation -->
    <aside
      class="border-vp-border bg-vp-bg-hover/20 md:border-r"
      :class="[
        'md:w-[17rem] lg:w-[19rem] xl:w-[22rem]',
        // Mobile: full-width when no chat is selected; hidden when one is.
        showDetailPane ? 'hidden md:block md:shrink-0' : 'flex-1 md:flex-none md:shrink-0',
      ]"
    >
      <ObservabilitySidebar
        :conversations="visibleConversations"
        :loading="conversationsLoading"
        :error="conversationsError"
        :selection="selection"
        :active-conversation-keys="activeConversationKeys"
        :providers="providers"
        :credentials-by-provider="credentialsByProvider"
        @select="applySelection"
        @refresh="loadConversations"
      />
    </aside>

    <!-- Detail pane -->
    <main
      class="flex-1 overflow-hidden bg-vp-surface"
      :class="showDetailPane ? 'flex' : 'hidden md:flex'"
    >
      <div class="flex h-full w-full flex-col">
        <!-- Mobile back button -->
        <button
          v-if="showDetailPane"
          type="button"
          class="flex shrink-0 items-center gap-1 border-b border-vp-border px-3 py-2 text-xs text-vp-muted transition hover:text-vp-text md:hidden"
          @click="goBack"
        >
          <VpIcon name="chevron-left" size-class="size-3.5" />
          <span>{{ t("obs.sidebar.backToList") }}</span>
        </button>
        <div class="flex-1 overflow-hidden">
          <ConversationDetail
            v-if="selection.kind === 'conversation' && selectedConversation"
            :conversation="selectedConversation"
            :realtime-requests="scopedRealtimeRequests"
            :realtime-attempts="scopedRealtimeAttempts"
            :requests="scopedRequests"
            :attempts="scopedAttempts"
            :providers="providers"
            :credentials-by-provider="credentialsByProvider"
            :active-tab="detailTab"
            @update:active-tab="detailTab = $event"
          />
          <GlobalDetail
            v-else
            :view="selection.kind === 'global' ? selection.view : 'all'"
            :realtime="realtime"
            :realtime-transport="realtimeTransport"
            :history="history"
            :providers="providers"
            :poll-ms="POLL_MS"
          />
        </div>
      </div>
    </main>
  </div>
</template>

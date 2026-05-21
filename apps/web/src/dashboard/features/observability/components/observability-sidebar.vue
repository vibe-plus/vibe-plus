<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import type {
  Credential,
  ObservabilityConversation,
  ObservabilityConversationStatus,
  Provider,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import type { vp_icon_name } from "../../../components/vp-icon.vue";
import { usePrivacyMode } from "../use-privacy-mode.ts";
import { useMetricMode } from "../use-metric-mode.ts";

export type SidebarSelection =
  | { kind: "conversation"; source: "codex" | "claude"; conversationId: string }
  | { kind: "global"; view: GlobalView };

export type GlobalView = "logs" | "waveform" | "all";

export type SortKey = "updated" | "requests" | "cost" | "tokens" | "duration";

export type ViewMode = "tree" | "flat";

const props = defineProps<{
  conversations: ObservabilityConversation[];
  loading: boolean;
  error: string | null;
  selection: SidebarSelection;
  activeConversationKeys: ReadonlySet<string>;
  providers: Provider[];
  credentialsByProvider: Record<string, Credential[]>;
}>();

const emit = defineEmits<{
  (e: "select", value: SidebarSelection): void;
  (e: "refresh"): void;
}>();

const { t } = useI18n({ useScope: "global" });
const { privacy, toggle: togglePrivacy, mask, maskPath } = usePrivacyMode();
const {
  metric,
  toggle: toggleMetric,
  formatUsd: formatUsdMetric,
  formatTokens: formatTokensMetric,
  formatMetric,
} = useMetricMode();

// ── Per-conversation aggregates (combine gateway + local-history numbers) ──

function totalUsdFor(c: ObservabilityConversation): number {
  const gateway = Number.parseFloat(c.estimated_cost_usd || "0") || 0;
  const local = Number.parseFloat(c.local_estimated_cost_usd || "0") || 0;
  // Prefer the larger of the two: gateway-observed cost is authoritative when
  // present, otherwise local-history estimate fills in.
  return Math.max(gateway, local);
}

function totalTokensFor(c: ObservabilityConversation): number {
  const gateway = c.input_tokens + c.output_tokens;
  const local = c.local_tokens_used;
  return Math.max(gateway, local);
}

function metricCellFor(c: ObservabilityConversation): string {
  return formatMetric(totalUsdFor(c), totalTokensFor(c));
}

// ── Section collapse state ──────────────────────────────────────────────────
const fnCollapsed = ref(false);
const projectsCollapsed = ref(false);
const providersSectionCollapsed = ref(false);
const chatsCollapsed = ref(false);

// Per-provider/account expand state (default: collapsed).
const expandedProviders = ref<Set<string>>(new Set());
const expandedAccounts = ref<Set<string>>(new Set());

function toggleProviderExpand(id: string, ev: Event) {
  ev.stopPropagation();
  const next = new Set(expandedProviders.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedProviders.value = next;
}

function toggleAccountExpand(id: string, ev: Event) {
  ev.stopPropagation();
  const next = new Set(expandedAccounts.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedAccounts.value = next;
}

// Per-account currently active filter (in addition to provider/credential).
const selectedAccountKey = ref<string>("");

// Per-project expand state (default: expanded).
const expandedProjects = ref<Set<string>>(new Set());
const collapsedProjects = ref<Set<string>>(new Set()); // explicit collapse beats default expand
// Per-thread expand state for showing subagents (default: collapsed).
const expandedThreads = ref<Set<string>>(new Set());

// Filters
const hideArchived = ref(true);
const hideNoRecords = ref(false);
const hideSubagents = ref(false);
const selectedProviderId = ref<string>("");
const selectedCredentialId = ref<string>("");

// Sort
const sortKey = ref<SortKey>("updated");

// View mode (tree vs flat).
const viewMode = ref<ViewMode>("tree");

// Popovers
const showSortMenu = ref(false);
const showFilterMenu = ref(false);
const filterMenuStyle = ref<Record<string, string>>({});
const sortMenuStyle = ref<Record<string, string>>({});

// Search
const search = ref("");

// ── Provider / credential lookup ───────────────────────────────────────────

const providersById = computed(() => new Map(props.providers.map((p) => [p.id, p])));
const allCredentials = computed<Credential[]>(() =>
  Object.values(props.credentialsByProvider).flat(),
);
const credentialsById = computed(() => new Map(allCredentials.value.map((c) => [c.id, c])));

const availableProviderOptions = computed(() => {
  const seen = new Set<string>();
  for (const c of props.conversations) {
    for (const id of c.provider_ids) seen.add(id);
  }
  return [...seen].map((id) => {
    const p = providersById.value.get(id);
    return { id, label: p?.name || id };
  });
});

const availableCredentialOptions = computed(() => {
  const seen = new Set<string>();
  for (const c of props.conversations) {
    if (selectedProviderId.value) {
      // When filtering by provider, only show credentials that belong to it
      // (heuristic: credential.provider_id matches selected provider).
      for (const cid of c.credential_ids) {
        const cred = credentialsById.value.get(cid);
        if (cred && cred.provider_id === selectedProviderId.value) seen.add(cid);
      }
    } else {
      for (const cid of c.credential_ids) seen.add(cid);
    }
  }
  return [...seen].map((id) => {
    const cred = credentialsById.value.get(id);
    return { id, label: cred?.label || id };
  });
});

function clearFilters() {
  hideArchived.value = true;
  hideNoRecords.value = false;
  hideSubagents.value = false;
  selectedProviderId.value = "";
  selectedCredentialId.value = "";
  selectedAccountKey.value = "";
}

// ── Provider → Account → Credential tree ───────────────────────────────────

function accountKeyForCredential(c: Credential): string {
  return (
    (c.oauth_account_email && `email:${c.oauth_account_email}`) ||
    (c.upstream_username && `user:${c.upstream_username}`) ||
    (c.oauth_account_subject && `sub:${c.oauth_account_subject}`) ||
    `cred:${c.id}` // fall back to one-account-per-credential
  );
}

function accountLabelForCredential(c: Credential): string {
  return (
    c.oauth_account_email ||
    c.upstream_username ||
    c.oauth_account_subject ||
    t("obs.sidebar.unknownAccount")
  );
}

type CredNode = { credential: Credential; chatCount: number; cost: number };
type AccountNode = {
  key: string;
  label: string;
  credentials: CredNode[];
  chatCount: number;
  cost: number;
};
type ProviderNode = {
  id: string;
  label: string;
  accounts: AccountNode[];
  chatCount: number;
  cost: number;
};

// Use conversations matching all *other* filters (archive / no-records / search)
// so that selecting a provider doesn't make the others disappear.
const conversationsForProviderTree = computed(() => {
  const q = search.value.trim().toLowerCase();
  return props.conversations.filter((c) => {
    if (hideArchived.value && c.archived) return false;
    if (hideNoRecords.value && c.request_count === 0) return false;
    if (q) {
      const hay =
        `${c.title} ${c.preview} ${c.project_name ?? ""} ${c.project_path ?? ""} ${c.conversation_id} ${c.agent_nickname ?? ""}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
});

const providerTree = computed<ProviderNode[]>(() => {
  // First, index chats by credential id and by provider id so we can count.
  const chatsByProvider = new Map<string, ObservabilityConversation[]>();
  const chatsByCredential = new Map<string, ObservabilityConversation[]>();
  for (const c of conversationsForProviderTree.value) {
    for (const pid of c.provider_ids) {
      const list = chatsByProvider.get(pid) ?? [];
      list.push(c);
      chatsByProvider.set(pid, list);
    }
    for (const cid of c.credential_ids) {
      const list = chatsByCredential.get(cid) ?? [];
      list.push(c);
      chatsByCredential.set(cid, list);
    }
  }

  const out: ProviderNode[] = [];
  for (const provider of props.providers) {
    const creds = props.credentialsByProvider[provider.id] ?? [];
    // Group this provider's credentials by account key.
    const byAccount = new Map<string, AccountNode>();
    for (const cred of creds) {
      const acctKey = accountKeyForCredential(cred);
      const credChats = chatsByCredential.get(cred.id) ?? [];
      const credCost = credChats.reduce(
        (s, c) => s + (parseFloat(c.estimated_cost_usd || "0") || 0),
        0,
      );
      const node: CredNode = {
        credential: cred,
        chatCount: credChats.length,
        cost: credCost,
      };
      const existing = byAccount.get(acctKey);
      if (existing) {
        existing.credentials.push(node);
        existing.chatCount += node.chatCount;
        existing.cost += node.cost;
      } else {
        byAccount.set(acctKey, {
          key: acctKey,
          label: accountLabelForCredential(cred),
          credentials: [node],
          chatCount: node.chatCount,
          cost: node.cost,
        });
      }
    }
    const accounts = [...byAccount.values()];
    const providerChats = chatsByProvider.get(provider.id) ?? [];
    const providerCost = providerChats.reduce(
      (s, c) => s + (parseFloat(c.estimated_cost_usd || "0") || 0),
      0,
    );
    // Only surface providers that had at least one chat or have credentials.
    if (providerChats.length === 0 && accounts.length === 0) continue;
    out.push({
      id: provider.id,
      label: provider.name || provider.id,
      accounts,
      chatCount: providerChats.length,
      cost: providerCost,
    });
  }
  // Sort by chat count desc.
  out.sort((a, b) => b.chatCount - a.chatCount || a.label.localeCompare(b.label));
  for (const p of out) {
    p.accounts.sort((a, b) => b.chatCount - a.chatCount || a.label.localeCompare(b.label));
  }
  return out;
});

function isProviderSelected(id: string): boolean {
  return (
    selectedProviderId.value === id && !selectedAccountKey.value && !selectedCredentialId.value
  );
}

function isAccountSelected(providerId: string, acctKey: string): boolean {
  return (
    selectedProviderId.value === providerId &&
    selectedAccountKey.value === acctKey &&
    !selectedCredentialId.value
  );
}

function isCredentialSelected(id: string): boolean {
  return selectedCredentialId.value === id;
}

function selectProvider(p: ProviderNode) {
  selectedProviderId.value = p.id;
  selectedAccountKey.value = "";
  selectedCredentialId.value = "";
}

function selectAccount(provider: ProviderNode, account: AccountNode) {
  selectedProviderId.value = provider.id;
  selectedAccountKey.value = account.key;
  selectedCredentialId.value = "";
}

function selectCredential(provider: ProviderNode, _account: AccountNode, cred: Credential) {
  selectedProviderId.value = provider.id;
  selectedAccountKey.value = "";
  selectedCredentialId.value = cred.id;
}

// Active provider/account/credential filter label for the banner.
const activeProviderFilterLabel = computed<string>(() => {
  if (selectedCredentialId.value) {
    const cred = credentialsById.value.get(selectedCredentialId.value);
    if (cred)
      return `${providersById.value.get(cred.provider_id)?.name ?? cred.provider_id} / ${cred.label || cred.id}`;
    return selectedCredentialId.value;
  }
  if (selectedProviderId.value && selectedAccountKey.value) {
    const provider = providersById.value.get(selectedProviderId.value);
    // find the account label
    const creds = props.credentialsByProvider[selectedProviderId.value] ?? [];
    const cred = creds.find((c) => accountKeyForCredential(c) === selectedAccountKey.value);
    const acctLabel = cred ? accountLabelForCredential(cred) : selectedAccountKey.value;
    return `${provider?.name ?? selectedProviderId.value} / ${acctLabel}`;
  }
  if (selectedProviderId.value) {
    return providersById.value.get(selectedProviderId.value)?.name ?? selectedProviderId.value;
  }
  return "";
});

function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  if (seconds < 60) return t("obs.sidebar.duration.seconds", { n: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("obs.sidebar.duration.minutes", { n: minutes });
  const hours = Math.floor(minutes / 60);
  const remM = minutes - hours * 60;
  if (hours < 24) return t("obs.sidebar.duration.hours", { h: hours, m: remM });
  const days = Math.floor(hours / 24);
  const remH = hours - days * 24;
  return t("obs.sidebar.duration.days", { d: days, h: remH });
}

type GlobalEntry = { view: GlobalView; icon: vp_icon_name; labelKey: string };
const GLOBAL_ENTRIES: GlobalEntry[] = [
  { view: "all", icon: "layout-dashboard", labelKey: "obs.global.all" },
  { view: "logs", icon: "activity", labelKey: "obs.global.logs" },
  { view: "waveform", icon: "zap", labelKey: "obs.global.waveform" },
];

// ── Helpers ─────────────────────────────────────────────────────────────────

function conversationKey(c: ObservabilityConversation): string {
  return `${c.source}:${c.conversation_id}`;
}

function statusFor(c: ObservabilityConversation): ObservabilityConversationStatus {
  return props.activeConversationKeys.has(conversationKey(c)) ? "running" : c.status;
}

function statusDotClass(status: ObservabilityConversationStatus): string {
  switch (status) {
    case "running":
      return "bg-sky-500 animate-pulse";
    case "ok":
      return "bg-emerald-500";
    case "failed":
      return "bg-red-500";
    case "no-data":
    default:
      return "bg-slate-300";
  }
}

function relativeTime(ts: number): string {
  if (!ts) return "";
  const now = Date.now() / 1000;
  const diff = now - ts;
  if (diff < 60) return t("obs.time.justNow");
  if (diff < 3600) return t("obs.time.minutesAgo", { n: Math.floor(diff / 60) });
  if (diff < 86_400) return t("obs.time.hoursAgo", { n: Math.floor(diff / 3600) });
  if (diff < 86_400 * 30) return t("obs.time.daysAgo", { n: Math.floor(diff / 86_400) });
  return new Date(ts * 1000).toLocaleDateString();
}

function sourceIconClass(source: "codex" | "claude"): string {
  return source === "codex" ? "i-[lobe--codex-color]" : "i-[lobe--claude-color]";
}

function displayTitle(c: ObservabilityConversation): string {
  const base = c.thread_kind === "subagent" && c.agent_nickname ? c.agent_nickname : c.title;
  return privacy.value ? mask(base, c.conversation_id.slice(0, 8)) : base;
}

function displayProjectName(name: string, path: string | null): string {
  if (!privacy.value) return name || path || "";
  return maskPath(name || path || "");
}

function sortConversations(list: ObservabilityConversation[]): ObservabilityConversation[] {
  return [...list].sort((a, b) => {
    switch (sortKey.value) {
      case "requests":
        return b.request_count - a.request_count || b.updated_at - a.updated_at;
      case "cost":
        return totalUsdFor(b) - totalUsdFor(a) || b.updated_at - a.updated_at;
      case "tokens":
        return totalTokensFor(b) - totalTokensFor(a) || b.updated_at - a.updated_at;
      case "duration":
        return b.duration_seconds - a.duration_seconds || b.updated_at - a.updated_at;
      case "updated":
      default:
        return b.updated_at - a.updated_at;
    }
  });
}

// ── Base filtering (archive / no-records / search) ──────────────────────────

const baseFiltered = computed(() => {
  const q = search.value.trim().toLowerCase();
  return props.conversations.filter((c) => {
    if (hideArchived.value && c.archived) return false;
    if (hideNoRecords.value && c.request_count === 0) return false;
    if (selectedProviderId.value && !c.provider_ids.includes(selectedProviderId.value))
      return false;
    if (selectedCredentialId.value && !c.credential_ids.includes(selectedCredentialId.value))
      return false;
    if (selectedAccountKey.value) {
      // Chat passes the account filter if any of its credential_ids resolve to
      // a credential whose account key matches.
      const matches = c.credential_ids.some((cid) => {
        const cred = credentialsById.value.get(cid);
        return cred ? accountKeyForCredential(cred) === selectedAccountKey.value : false;
      });
      if (!matches) return false;
    }
    if (q) {
      const hay =
        `${c.title} ${c.preview} ${c.project_name ?? ""} ${c.project_path ?? ""} ${c.conversation_id} ${c.agent_nickname ?? ""}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
});

// ── Flat list (for ViewMode = 'flat') ───────────────────────────────────────

const flatList = computed<ObservabilityConversation[]>(() => {
  const list = hideSubagents.value
    ? baseFiltered.value.filter((c) => c.thread_kind !== "subagent")
    : baseFiltered.value;
  return sortConversations(list);
});

// ── Build the project tree ──────────────────────────────────────────────────

type ChatNode = {
  conv: ObservabilityConversation;
  subagents: ObservabilityConversation[];
};
type ProjectGroup = {
  key: string;
  name: string;
  path: string;
  mainThreads: ChatNode[];
  totalCost: number;
  totalTokens: number;
  totalChats: number;
  latestUpdated: number;
};

const tree = computed<{ projects: ProjectGroup[]; orphans: ChatNode[] }>(() => {
  // Group all conversations by parent_id for subagent lookup
  const byParent = new Map<string, ObservabilityConversation[]>();
  for (const c of baseFiltered.value) {
    if (c.parent_conversation_id) {
      const list = byParent.get(c.parent_conversation_id) ?? [];
      list.push(c);
      byParent.set(c.parent_conversation_id, list);
    }
  }

  // Main threads only (not subagents) → into projects vs orphans.
  const mainThreads = baseFiltered.value.filter((c) => c.thread_kind !== "subagent");

  const projects = new Map<string, ProjectGroup>();
  const orphans: ChatNode[] = [];

  for (const conv of mainThreads) {
    const subagents = byParent.get(conv.conversation_id) ?? [];
    const node: ChatNode = { conv, subagents };
    if (!conv.project_path) {
      orphans.push(node);
      continue;
    }
    const key = conv.project_path;
    const existing = projects.get(key);
    const ownCost = totalUsdFor(conv);
    const ownTokens = totalTokensFor(conv);
    const subCost = subagents.reduce((s, c) => s + totalUsdFor(c), 0);
    const subTokens = subagents.reduce((s, c) => s + totalTokensFor(c), 0);
    if (existing) {
      existing.mainThreads.push(node);
      existing.totalCost += ownCost + subCost;
      existing.totalTokens += ownTokens + subTokens;
      existing.totalChats += 1 + subagents.length;
      existing.latestUpdated = Math.max(existing.latestUpdated, conv.updated_at);
    } else {
      projects.set(key, {
        key,
        name: conv.project_name ?? key,
        path: key,
        mainThreads: [node],
        totalCost: ownCost + subCost,
        totalTokens: ownTokens + subTokens,
        totalChats: 1 + subagents.length,
        latestUpdated: conv.updated_at,
      });
    }
  }

  const projectsList = [...projects.values()];
  // Sort projects by their latest activity by default; respects sortKey for cost/tokens/requests too.
  projectsList.sort((a, b) => {
    if (sortKey.value === "cost")
      return b.totalCost - a.totalCost || b.latestUpdated - a.latestUpdated;
    if (sortKey.value === "tokens")
      return b.totalTokens - a.totalTokens || b.latestUpdated - a.latestUpdated;
    if (sortKey.value === "requests")
      return b.totalChats - a.totalChats || b.latestUpdated - a.latestUpdated;
    return b.latestUpdated - a.latestUpdated;
  });
  for (const group of projectsList) {
    const sortedMain = sortConversations(group.mainThreads.map((n) => n.conv));
    group.mainThreads = sortedMain.map((conv) => ({
      conv,
      subagents: byParent.get(conv.conversation_id) ?? [],
    }));
    for (const node of group.mainThreads) {
      node.subagents = sortConversations(node.subagents);
    }
  }

  const sortedOrphans = sortConversations(orphans.map((n) => n.conv)).map((conv) => ({
    conv,
    subagents: byParent.get(conv.conversation_id) ?? [],
  }));

  return { projects: projectsList, orphans: sortedOrphans };
});

function isProjectExpanded(key: string): boolean {
  if (collapsedProjects.value.has(key)) return false;
  // Default expanded.
  return !collapsedProjects.value.has(key);
}

function toggleProject(key: string, ev: Event) {
  ev.stopPropagation();
  if (collapsedProjects.value.has(key)) {
    const next = new Set(collapsedProjects.value);
    next.delete(key);
    collapsedProjects.value = next;
    const ex = new Set(expandedProjects.value);
    ex.add(key);
    expandedProjects.value = ex;
  } else {
    const next = new Set(collapsedProjects.value);
    next.add(key);
    collapsedProjects.value = next;
  }
}

function toggleThread(id: string, ev: Event) {
  ev.stopPropagation();
  const next = new Set(expandedThreads.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedThreads.value = next;
}

// ── Selection ───────────────────────────────────────────────────────────────

function isSelected(c: ObservabilityConversation): boolean {
  if (props.selection.kind !== "conversation") return false;
  return (
    props.selection.source === c.source && props.selection.conversationId === c.conversation_id
  );
}

function isGlobalSelected(view: GlobalView): boolean {
  return props.selection.kind === "global" && props.selection.view === view;
}

function selectConversation(c: ObservabilityConversation) {
  emit("select", { kind: "conversation", source: c.source, conversationId: c.conversation_id });
}

function selectGlobal(view: GlobalView) {
  emit("select", { kind: "global", view });
}

// ── Popover positioning (Teleport to body to avoid clipping) ───────────────

const filterTriggerRef = ref<HTMLElement | null>(null);
const sortTriggerRef = ref<HTMLElement | null>(null);
const filterMenuRef = ref<HTMLElement | null>(null);
const sortMenuRef = ref<HTMLElement | null>(null);

function anchorMenu(trigger: HTMLElement | null, width: number): Record<string, string> {
  if (!trigger) return { display: "none" };
  const rect = trigger.getBoundingClientRect();
  const vw = window.innerWidth;
  const left = Math.max(8, Math.min(rect.right - width, vw - width - 8));
  return {
    position: "fixed",
    top: `${rect.bottom + 4}px`,
    left: `${left}px`,
    width: `${width}px`,
    zIndex: "60",
  };
}

function openFilterMenu() {
  showFilterMenu.value = !showFilterMenu.value;
  showSortMenu.value = false;
  if (showFilterMenu.value) {
    filterMenuStyle.value = anchorMenu(filterTriggerRef.value, 256);
  }
}

function openSortMenu() {
  showSortMenu.value = !showSortMenu.value;
  showFilterMenu.value = false;
  if (showSortMenu.value) {
    sortMenuStyle.value = anchorMenu(sortTriggerRef.value, 192);
  }
}

function onDocClick(e: MouseEvent) {
  const target = e.target as Node;
  if (
    showFilterMenu.value &&
    !filterTriggerRef.value?.contains(target) &&
    !filterMenuRef.value?.contains(target)
  ) {
    showFilterMenu.value = false;
  }
  if (
    showSortMenu.value &&
    !sortTriggerRef.value?.contains(target) &&
    !sortMenuRef.value?.contains(target)
  ) {
    showSortMenu.value = false;
  }
}
function onScroll() {
  if (showFilterMenu.value && filterTriggerRef.value) {
    filterMenuStyle.value = anchorMenu(filterTriggerRef.value, 256);
  }
  if (showSortMenu.value && sortTriggerRef.value) {
    sortMenuStyle.value = anchorMenu(sortTriggerRef.value, 192);
  }
}
onMounted(() => {
  document.addEventListener("click", onDocClick);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", onScroll);
});
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocClick);
  window.removeEventListener("scroll", onScroll, true);
  window.removeEventListener("resize", onScroll);
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-vp-bg-hover/20">
    <!-- Top toolbar: shared controls for every section below
         (view / filter / sort apply to chats; metric / privacy / refresh
         are display-only). -->
    <div class="flex shrink-0 items-center gap-1 border-b border-vp-border px-2 py-1.5">
      <span class="mr-auto px-1 text-[10px] font-semibold uppercase tracking-wider text-vp-muted">
        {{ t("obs.title") }}
      </span>
      <!-- Tree / Flat view toggle -->
      <div class="flex items-center rounded-md border border-vp-border bg-vp-surface">
        <button
          type="button"
          class="rounded-l-md px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wide transition"
          :class="
            viewMode === 'tree' ? 'bg-vp-bg-hover text-vp-text' : 'text-vp-muted hover:text-vp-text'
          "
          :title="t('obs.sidebar.view.tooltipTree')"
          @click="viewMode = 'tree'"
        >
          {{ t("obs.sidebar.view.tree") }}
        </button>
        <button
          type="button"
          class="rounded-r-md px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wide transition"
          :class="
            viewMode === 'flat' ? 'bg-vp-bg-hover text-vp-text' : 'text-vp-muted hover:text-vp-text'
          "
          :title="t('obs.sidebar.view.tooltipFlat')"
          @click="viewMode = 'flat'"
        >
          {{ t("obs.sidebar.view.flat") }}
        </button>
      </div>
      <!-- Filter -->
      <button
        ref="filterTriggerRef"
        type="button"
        class="rounded p-1 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
        :class="
          hideArchived ||
          hideNoRecords ||
          hideSubagents ||
          selectedProviderId ||
          selectedCredentialId ||
          selectedAccountKey
            ? 'text-vp-primary'
            : ''
        "
        :title="t('obs.sidebar.filter.title')"
        @click.stop="openFilterMenu()"
      >
        <VpIcon name="filter" size-class="size-3.5" />
      </button>
      <!-- Sort -->
      <button
        ref="sortTriggerRef"
        type="button"
        class="rounded p-1 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
        :class="sortKey !== 'updated' ? 'text-vp-primary' : ''"
        :title="t('obs.sidebar.sort.title')"
        @click.stop="openSortMenu()"
      >
        <VpIcon name="arrow-up-down" size-class="size-3.5" />
      </button>
      <!-- Metric toggle (USD / Tokens) -->
      <button
        type="button"
        class="rounded px-1.5 py-1 font-mono text-[10px] font-semibold uppercase text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
        :title="metric === 'usd' ? t('obs.sidebar.metric.toTokens') : t('obs.sidebar.metric.toUsd')"
        @click="toggleMetric"
      >
        {{ metric === "usd" ? "$" : "T" }}
      </button>
      <!-- Privacy -->
      <button
        type="button"
        class="rounded p-1 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
        :title="privacy ? t('obs.sidebar.privacyOn') : t('obs.sidebar.privacyOff')"
        @click="togglePrivacy"
      >
        <VpIcon :name="privacy ? 'eye-off' : 'eye'" size-class="size-3.5" />
      </button>
      <!-- Refresh -->
      <button
        type="button"
        class="rounded p-1 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
        :title="t('obs.sidebar.refresh')"
        @click="emit('refresh')"
      >
        <VpIcon name="refresh-cw" size-class="size-3.5" />
      </button>
    </div>

    <div class="flex flex-1 min-h-0 flex-col overflow-auto">
      <!-- ──────── 功能栏 (FunctionBar) — collapsible ──────── -->
      <section class="shrink-0 border-b border-vp-border">
        <button
          type="button"
          class="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-[10px] font-semibold uppercase tracking-wider text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
          @click="fnCollapsed = !fnCollapsed"
        >
          <VpIcon
            name="chevron-right"
            size-class="size-3"
            class="transition-transform"
            :class="fnCollapsed ? '' : 'rotate-90'"
          />
          <span>{{ t("obs.sidebar.functionBar") }}</span>
        </button>
        <div v-if="!fnCollapsed" class="p-2 pt-0">
          <div class="flex flex-col gap-0.5">
            <button
              v-for="entry in GLOBAL_ENTRIES"
              :key="entry.view"
              type="button"
              class="flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition hover:bg-vp-bg-hover"
              :class="
                isGlobalSelected(entry.view)
                  ? 'bg-vp-surface font-semibold text-vp-text ring-1 ring-vp-border'
                  : 'text-vp-muted'
              "
              @click="selectGlobal(entry.view)"
            >
              <VpIcon :name="entry.icon" size-class="size-3.5" />
              <span class="truncate">{{ t(entry.labelKey) }}</span>
            </button>
          </div>
          <div class="relative mt-2">
            <VpIcon
              name="search"
              size-class="size-3"
              class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-vp-muted"
            />
            <input
              v-model="search"
              type="search"
              :placeholder="t('obs.sidebar.searchPlaceholder')"
              class="w-full rounded-md border border-vp-border bg-vp-surface py-1 pl-7 pr-2 text-xs text-vp-text placeholder:text-vp-muted focus:border-vp-primary focus:outline-none"
            />
          </div>
        </div>
      </section>

      <!-- ──────── 项目 (Projects tree) ──────── -->
      <section class="shrink-0 border-b border-vp-border">
        <button
          type="button"
          class="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-[10px] font-semibold uppercase tracking-wider text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
          @click="projectsCollapsed = !projectsCollapsed"
        >
          <VpIcon
            name="chevron-right"
            size-class="size-3"
            class="transition-transform"
            :class="projectsCollapsed ? '' : 'rotate-90'"
          />
          <span>{{ t("obs.sidebar.projects") }}</span>
          <span class="ml-auto font-mono text-[9px] text-vp-muted/70">
            {{ viewMode === "flat" ? flatList.length : tree.projects.length }}
          </span>
        </button>
        <div v-if="!projectsCollapsed" class="p-1 pt-0">
          <div
            v-if="props.loading && !props.conversations.length"
            class="px-2 py-2 text-xs text-vp-muted"
          >
            {{ t("obs.sidebar.loading") }}
          </div>
          <!-- Flat mode: all chats merged into one sorted list -->
          <div v-else-if="viewMode === 'flat'" class="flex flex-col gap-0.5">
            <div
              v-if="!flatList.length"
              class="m-1 rounded-md border border-dashed border-vp-border px-2 py-3 text-center text-[11px] text-vp-muted"
            >
              {{ t("obs.sidebar.empty") }}
            </div>
            <button
              v-for="conv in flatList"
              v-else
              :key="conversationKey(conv)"
              type="button"
              class="group flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left text-xs transition hover:bg-vp-bg-hover"
              :class="
                isSelected(conv)
                  ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30 shadow-sm'
                  : 'text-vp-muted'
              "
              @click="selectConversation(conv)"
            >
              <div class="flex items-center gap-1.5">
                <span
                  class="size-1.5 shrink-0 rounded-full"
                  :class="statusDotClass(statusFor(conv))"
                />
                <span
                  :class="[sourceIconClass(conv.source), 'size-3.5 shrink-0']"
                  aria-hidden="true"
                />
                <span class="min-w-0 flex-1 truncate font-medium text-vp-text" :title="conv.title">
                  {{ displayTitle(conv) }}
                </span>
                <span
                  v-if="metricCellFor(conv)"
                  class="shrink-0 font-mono text-[10px]"
                  :class="metric === 'usd' ? 'text-emerald-700/80' : 'text-vp-text/70'"
                >
                  {{ metricCellFor(conv) }}
                </span>
              </div>
              <div class="flex items-center gap-1.5 pl-5 text-[10px] text-vp-muted">
                <span>{{ relativeTime(conv.updated_at) }}</span>
                <span
                  v-if="conv.project_name"
                  class="truncate"
                  :title="conv.project_path ?? undefined"
                >
                  {{ displayProjectName(conv.project_name, conv.project_path) }}
                </span>
                <span
                  v-if="conv.duration_seconds > 0"
                  class="font-mono"
                  :title="t('obs.sidebar.duration.label')"
                >
                  {{ formatDuration(conv.duration_seconds) }}
                </span>
              </div>
            </button>
          </div>
          <div
            v-else-if="!tree.projects.length"
            class="m-1 rounded-md border border-dashed border-vp-border px-2 py-3 text-center text-[11px] text-vp-muted"
          >
            {{ t("obs.sidebar.emptyProjects") }}
          </div>
          <div v-else class="flex flex-col gap-0.5">
            <div v-for="group in tree.projects" :key="group.key" class="flex flex-col">
              <button
                type="button"
                class="group/proj flex items-center gap-1 rounded-md px-1.5 py-1 text-left transition hover:bg-vp-bg-hover"
                :title="group.path"
                @click="toggleProject(group.key, $event)"
              >
                <VpIcon
                  name="chevron-right"
                  size-class="size-3"
                  class="shrink-0 text-vp-muted transition-transform"
                  :class="isProjectExpanded(group.key) ? 'rotate-90' : ''"
                />
                <VpIcon name="folder" size-class="size-3" class="shrink-0 text-vp-muted" />
                <span
                  class="min-w-0 flex-1 truncate text-xs font-medium text-vp-text"
                  :title="
                    t('obs.sidebar.projectTooltip', {
                      chats: group.totalChats,
                      tokens: formatTokensMetric(group.totalTokens) || 0,
                    })
                  "
                >
                  {{ displayProjectName(group.name, group.path) }}
                </span>
                <span
                  v-if="metric === 'usd' && group.totalCost > 0"
                  class="shrink-0 font-mono text-[10px] text-emerald-700/80"
                >
                  {{ formatUsdMetric(group.totalCost) }}
                </span>
                <span
                  v-else-if="metric === 'tokens' && group.totalTokens > 0"
                  class="shrink-0 font-mono text-[10px] text-vp-text/80"
                >
                  {{ formatTokensMetric(group.totalTokens) }}
                </span>
                <span v-else class="shrink-0 font-mono text-[10px] text-vp-muted">
                  {{ group.totalChats }}
                </span>
              </button>
              <div
                v-if="isProjectExpanded(group.key)"
                class="ml-3 flex flex-col gap-0.5 border-l border-vp-border/40 pl-1"
              >
                <template v-for="node in group.mainThreads" :key="conversationKey(node.conv)">
                  <div class="flex flex-col">
                    <button
                      type="button"
                      class="flex w-full items-center gap-1 rounded-md px-1.5 py-1 text-left text-xs transition hover:bg-vp-bg-hover"
                      :class="
                        isSelected(node.conv)
                          ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30'
                          : 'text-vp-muted'
                      "
                      @click="selectConversation(node.conv)"
                    >
                      <button
                        v-if="node.subagents.length > 0"
                        type="button"
                        class="shrink-0 rounded p-0.5 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
                        @click.stop="toggleThread(node.conv.conversation_id, $event)"
                      >
                        <VpIcon
                          name="chevron-right"
                          size-class="size-2.5"
                          class="transition-transform"
                          :class="expandedThreads.has(node.conv.conversation_id) ? 'rotate-90' : ''"
                        />
                      </button>
                      <span v-else class="size-3 shrink-0" />
                      <span
                        class="size-1.5 shrink-0 rounded-full"
                        :class="statusDotClass(statusFor(node.conv))"
                      />
                      <span
                        :class="[sourceIconClass(node.conv.source), 'size-3 shrink-0']"
                        aria-hidden="true"
                      />
                      <span
                        class="min-w-0 flex-1 truncate font-medium text-vp-text"
                        :title="node.conv.title"
                      >
                        {{ displayTitle(node.conv) }}
                      </span>
                      <span
                        v-if="metricCellFor(node.conv)"
                        class="shrink-0 font-mono text-[9px]"
                        :class="metric === 'usd' ? 'text-emerald-700/80' : 'text-vp-text/70'"
                      >
                        {{ metricCellFor(node.conv) }}
                      </span>
                      <span
                        v-else-if="node.conv.request_count > 0"
                        class="shrink-0 font-mono text-[9px] text-vp-muted/70"
                      >
                        {{ node.conv.request_count }}
                      </span>
                    </button>
                    <div
                      v-if="
                        expandedThreads.has(node.conv.conversation_id) &&
                        node.subagents.length &&
                        !(hideSubagents && !expandedThreads.has(node.conv.conversation_id))
                      "
                      class="ml-4 flex flex-col gap-0.5 border-l border-vp-border/30 pl-1"
                    >
                      <button
                        v-for="sub in node.subagents"
                        :key="conversationKey(sub)"
                        type="button"
                        class="flex w-full items-center gap-1 rounded-md px-1.5 py-1 text-left text-[11px] transition hover:bg-vp-bg-hover"
                        :class="
                          isSelected(sub)
                            ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30'
                            : 'text-vp-muted'
                        "
                        @click="selectConversation(sub)"
                      >
                        <span
                          class="size-1.5 shrink-0 rounded-full"
                          :class="statusDotClass(statusFor(sub))"
                        />
                        <VpIcon name="bot" size-class="size-3" class="shrink-0 text-vp-muted" />
                        <span class="min-w-0 flex-1 truncate" :title="sub.title">
                          {{ displayTitle(sub) }}
                        </span>
                        <span
                          v-if="metricCellFor(sub)"
                          class="shrink-0 font-mono text-[9px]"
                          :class="metric === 'usd' ? 'text-emerald-700/80' : 'text-vp-text/70'"
                        >
                          {{ metricCellFor(sub) }}
                        </span>
                      </button>
                    </div>
                  </div>
                </template>
                <div v-if="!group.mainThreads.length" class="px-2 py-1 text-[11px] text-vp-muted">
                  {{ t("obs.sidebar.emptyProjectChats") }}
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- ──────── 供应商 (Providers tree) ──────── -->
      <section v-if="providerTree.length" class="shrink-0 border-b border-vp-border">
        <button
          type="button"
          class="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-[10px] font-semibold uppercase tracking-wider text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
          @click="providersSectionCollapsed = !providersSectionCollapsed"
        >
          <VpIcon
            name="chevron-right"
            size-class="size-3"
            class="transition-transform"
            :class="providersSectionCollapsed ? '' : 'rotate-90'"
          />
          <span>{{ t("obs.sidebar.providersSection") }}</span>
          <span class="ml-auto font-mono text-[9px] text-vp-muted/70">
            {{ providerTree.length }}
          </span>
        </button>
        <div v-if="!providersSectionCollapsed" class="p-1 pt-0">
          <div
            v-if="activeProviderFilterLabel"
            class="m-1 flex items-center gap-1 rounded-md bg-vp-primary/10 px-2 py-1 text-[10px] text-vp-primary"
          >
            <VpIcon name="filter" size-class="size-3" />
            <button
              type="button"
              class="flex-1 truncate text-left"
              :title="t('obs.sidebar.clearProviderFilter', { label: activeProviderFilterLabel })"
              @click="
                selectedProviderId = '';
                selectedAccountKey = '';
                selectedCredentialId = '';
              "
            >
              {{ activeProviderFilterLabel }}
            </button>
            <button
              type="button"
              class="shrink-0 rounded p-0.5 hover:bg-vp-primary/20"
              @click="
                selectedProviderId = '';
                selectedAccountKey = '';
                selectedCredentialId = '';
              "
            >
              <VpIcon name="x" size-class="size-3" />
            </button>
          </div>
          <div class="flex flex-col gap-0.5">
            <div v-for="provider in providerTree" :key="provider.id" class="flex flex-col">
              <button
                type="button"
                class="flex items-center gap-1 rounded-md px-1.5 py-1 text-left text-xs transition hover:bg-vp-bg-hover"
                :class="
                  isProviderSelected(provider.id)
                    ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30'
                    : 'text-vp-muted'
                "
                @click="selectProvider(provider)"
              >
                <button
                  v-if="provider.accounts.length"
                  type="button"
                  class="shrink-0 rounded p-0.5 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
                  @click.stop="toggleProviderExpand(provider.id, $event)"
                >
                  <VpIcon
                    name="chevron-right"
                    size-class="size-2.5"
                    class="transition-transform"
                    :class="expandedProviders.has(provider.id) ? 'rotate-90' : ''"
                  />
                </button>
                <span v-else class="size-3 shrink-0" />
                <VpIcon name="server" size-class="size-3" class="shrink-0 text-vp-muted" />
                <span
                  class="min-w-0 flex-1 truncate font-medium text-vp-text"
                  :title="provider.label"
                >
                  {{ provider.label }}
                </span>
                <span
                  v-if="metric === 'usd' && provider.cost > 0"
                  class="shrink-0 font-mono text-[10px] text-emerald-700/80"
                >
                  {{ formatUsdMetric(provider.cost) }}
                </span>
                <span v-else class="shrink-0 font-mono text-[10px] text-vp-muted">
                  {{ provider.chatCount }}
                </span>
              </button>
              <div
                v-if="expandedProviders.has(provider.id) && provider.accounts.length"
                class="ml-3 flex flex-col gap-0.5 border-l border-vp-border/40 pl-1"
              >
                <template v-for="account in provider.accounts" :key="account.key">
                  <div class="flex flex-col">
                    <button
                      type="button"
                      class="flex items-center gap-1 rounded-md px-1.5 py-1 text-left text-[11px] transition hover:bg-vp-bg-hover"
                      :class="
                        isAccountSelected(provider.id, account.key)
                          ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30'
                          : 'text-vp-muted'
                      "
                      @click="selectAccount(provider, account)"
                    >
                      <button
                        v-if="account.credentials.length > 1"
                        type="button"
                        class="shrink-0 rounded p-0.5 text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
                        @click.stop="toggleAccountExpand(account.key, $event)"
                      >
                        <VpIcon
                          name="chevron-right"
                          size-class="size-2.5"
                          class="transition-transform"
                          :class="expandedAccounts.has(account.key) ? 'rotate-90' : ''"
                        />
                      </button>
                      <span v-else class="size-3 shrink-0" />
                      <VpIcon name="key" size-class="size-2.5" class="shrink-0 text-vp-muted" />
                      <span class="min-w-0 flex-1 truncate" :title="account.label">
                        {{ privacy ? mask(account.label) : account.label }}
                      </span>
                      <span
                        v-if="metric === 'usd' && account.cost > 0"
                        class="shrink-0 font-mono text-[9px] text-emerald-700/80"
                      >
                        {{ formatUsdMetric(account.cost) }}
                      </span>
                      <span v-else class="shrink-0 font-mono text-[9px] text-vp-muted/70">
                        {{ account.chatCount }}
                      </span>
                    </button>
                    <div
                      v-if="expandedAccounts.has(account.key) || account.credentials.length === 1"
                      class="ml-4 flex flex-col gap-0.5 border-l border-vp-border/30 pl-1"
                    >
                      <button
                        v-for="cred in account.credentials"
                        :key="cred.credential.id"
                        type="button"
                        class="flex items-center gap-1 rounded-md px-1.5 py-1 text-left text-[11px] transition hover:bg-vp-bg-hover"
                        :class="
                          isCredentialSelected(cred.credential.id)
                            ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30'
                            : 'text-vp-muted'
                        "
                        @click="selectCredential(provider, account, cred.credential)"
                      >
                        <span class="size-3 shrink-0" />
                        <span
                          class="size-1 shrink-0 rounded-full"
                          :class="cred.credential.enabled ? 'bg-emerald-500' : 'bg-slate-300'"
                        />
                        <span
                          class="min-w-0 flex-1 truncate"
                          :title="
                            (cred.credential.label || cred.credential.id) +
                            (cred.credential.plan_type ? ` (${cred.credential.plan_type})` : '')
                          "
                        >
                          {{
                            privacy
                              ? mask(cred.credential.label || cred.credential.id)
                              : cred.credential.label || cred.credential.id
                          }}
                        </span>
                        <span
                          v-if="metric === 'usd' && cred.cost > 0"
                          class="shrink-0 font-mono text-[9px] text-emerald-700/80"
                        >
                          {{ formatUsdMetric(cred.cost) }}
                        </span>
                        <span
                          v-else-if="cred.chatCount > 0"
                          class="shrink-0 font-mono text-[9px] text-vp-muted/70"
                        >
                          {{ cred.chatCount }}
                        </span>
                      </button>
                    </div>
                  </div>
                </template>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- ──────── 聊天 (Unfiled chats) ──────── -->
      <section class="flex min-h-[10rem] flex-col">
        <button
          type="button"
          class="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-[10px] font-semibold uppercase tracking-wider text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
          @click="chatsCollapsed = !chatsCollapsed"
        >
          <VpIcon
            name="chevron-right"
            size-class="size-3"
            class="transition-transform"
            :class="chatsCollapsed ? '' : 'rotate-90'"
          />
          <span>{{ t("obs.sidebar.chats") }}</span>
          <span class="ml-auto font-mono text-[9px] text-vp-muted/70">
            {{ tree.orphans.length }}
          </span>
        </button>
        <div v-if="!chatsCollapsed" class="flex-1 p-1 pt-0">
          <div
            v-if="props.error"
            class="mx-1 rounded-md bg-red-50 px-2 py-1.5 text-xs text-red-700"
          >
            {{ props.error }}
          </div>
          <div
            v-else-if="!tree.orphans.length"
            class="m-1 rounded-md border border-dashed border-vp-border px-2 py-4 text-center text-[11px] text-vp-muted"
          >
            {{ t("obs.sidebar.emptyChats") }}
          </div>
          <div v-else class="flex flex-col gap-0.5">
            <button
              v-for="node in tree.orphans"
              :key="conversationKey(node.conv)"
              type="button"
              class="group flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left text-xs transition hover:bg-vp-bg-hover"
              :class="
                isSelected(node.conv)
                  ? 'bg-vp-surface text-vp-text ring-1 ring-vp-primary/30 shadow-sm'
                  : 'text-vp-muted'
              "
              @click="selectConversation(node.conv)"
            >
              <div class="flex items-center gap-1.5">
                <span
                  class="size-1.5 shrink-0 rounded-full"
                  :class="statusDotClass(statusFor(node.conv))"
                />
                <span
                  :class="[sourceIconClass(node.conv.source), 'size-3.5 shrink-0']"
                  aria-hidden="true"
                />
                <span
                  class="min-w-0 flex-1 truncate font-medium text-vp-text"
                  :title="node.conv.title"
                >
                  {{ displayTitle(node.conv) }}
                </span>
                <span
                  v-if="metricCellFor(node.conv)"
                  class="shrink-0 font-mono text-[10px]"
                  :class="metric === 'usd' ? 'text-emerald-700/80' : 'text-vp-text/70'"
                >
                  {{ metricCellFor(node.conv) }}
                </span>
              </div>
              <div class="flex items-center gap-1.5 pl-5 text-[10px] text-vp-muted">
                <span>{{ relativeTime(node.conv.updated_at) }}</span>
                <span v-if="node.conv.request_count > 0" class="font-mono">
                  {{ t("obs.sidebar.requestCount", { n: node.conv.request_count }) }}
                </span>
                <span
                  v-if="node.conv.duration_seconds > 0"
                  class="font-mono"
                  :title="t('obs.sidebar.duration.label')"
                >
                  {{ formatDuration(node.conv.duration_seconds) }}
                </span>
                <span
                  v-if="node.conv.models_used.length"
                  class="ml-auto truncate font-mono"
                  :title="node.conv.models_used.join(', ')"
                >
                  {{ node.conv.models_used[0] }}
                </span>
              </div>
            </button>
          </div>
        </div>
      </section>
    </div>

    <!-- Teleported dropdowns so they aren't clipped by overflow:auto -->
    <Teleport to="body">
      <div
        v-if="showFilterMenu"
        ref="filterMenuRef"
        :style="filterMenuStyle"
        class="rounded-md border border-vp-border bg-vp-surface p-2 shadow-xl"
        @click.stop
      >
        <div class="mb-1 px-1 text-[9px] uppercase tracking-wider text-vp-muted">
          {{ t("obs.sidebar.filter.title") }}
        </div>
        <label class="flex items-center gap-2 rounded px-1 py-1 text-xs hover:bg-vp-bg-hover">
          <input v-model="hideArchived" type="checkbox" class="size-3" />
          <span>{{ t("obs.sidebar.filter.hideArchived") }}</span>
        </label>
        <label class="flex items-center gap-2 rounded px-1 py-1 text-xs hover:bg-vp-bg-hover">
          <input v-model="hideNoRecords" type="checkbox" class="size-3" />
          <span>{{ t("obs.sidebar.filter.hideNoRecords") }}</span>
        </label>
        <label class="flex items-center gap-2 rounded px-1 py-1 text-xs hover:bg-vp-bg-hover">
          <input v-model="hideSubagents" type="checkbox" class="size-3" />
          <span>{{ t("obs.sidebar.filter.hideSubagents") }}</span>
        </label>
        <div v-if="availableProviderOptions.length" class="mt-2 border-t border-vp-border/60 pt-1">
          <div class="px-1 pb-1 text-[9px] uppercase tracking-wider text-vp-muted">
            {{ t("obs.sidebar.filter.providers") }}
          </div>
          <select
            v-model="selectedProviderId"
            class="w-full rounded border border-vp-border bg-vp-surface px-1 py-0.5 text-xs"
          >
            <option value="">{{ t("obs.sidebar.filter.anyProvider") }}</option>
            <option v-for="opt in availableProviderOptions" :key="opt.id" :value="opt.id">
              {{ opt.label }}
            </option>
          </select>
        </div>
        <div
          v-if="availableCredentialOptions.length"
          class="mt-2 border-t border-vp-border/60 pt-1"
        >
          <div class="px-1 pb-1 text-[9px] uppercase tracking-wider text-vp-muted">
            {{ t("obs.sidebar.filter.credentials") }}
          </div>
          <select
            v-model="selectedCredentialId"
            class="w-full rounded border border-vp-border bg-vp-surface px-1 py-0.5 text-xs"
          >
            <option value="">{{ t("obs.sidebar.filter.anyCredential") }}</option>
            <option v-for="opt in availableCredentialOptions" :key="opt.id" :value="opt.id">
              {{ opt.label }}
            </option>
          </select>
        </div>
        <button
          type="button"
          class="mt-2 w-full rounded border border-vp-border px-2 py-1 text-[11px] text-vp-muted transition hover:bg-vp-bg-hover hover:text-vp-text"
          @click="clearFilters"
        >
          {{ t("obs.sidebar.filter.clear") }}
        </button>
      </div>
      <div
        v-if="showSortMenu"
        ref="sortMenuRef"
        :style="sortMenuStyle"
        class="rounded-md border border-vp-border bg-vp-surface p-1 shadow-xl"
        @click.stop
      >
        <button
          v-for="opt in [
            { id: 'updated' as const, key: 'obs.sidebar.sort.updated' },
            { id: 'requests' as const, key: 'obs.sidebar.sort.requests' },
            { id: 'cost' as const, key: 'obs.sidebar.sort.cost' },
            { id: 'tokens' as const, key: 'obs.sidebar.sort.tokens' },
            { id: 'duration' as const, key: 'obs.sidebar.sort.duration' },
          ]"
          :key="opt.id"
          type="button"
          class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-vp-bg-hover"
          :class="sortKey === opt.id ? 'font-semibold text-vp-text' : 'text-vp-muted'"
          @click="
            sortKey = opt.id;
            showSortMenu = false;
          "
        >
          <VpIcon name="check" size-class="size-3" :class="sortKey === opt.id ? '' : 'invisible'" />
          <span>{{ t(opt.key) }}</span>
        </button>
      </div>
    </Teleport>
  </div>
</template>

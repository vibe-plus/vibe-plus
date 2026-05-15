/**
 * Global credential-intake flow orchestration.
 *
 * Entry points:
 * - `handlePaste(items)` / `handleDrop(items)` —— aggressive mode: auto-probe, select all by default, and open the wizard immediately.
 * - `tryShyClipboard()` —— shy mode: silently try reading the clipboard when entering Providers;
 *   on detection, show a bottom-right bubble asking whether to open the wizard.
 * - `openWithCandidates(items)` —— manually open the wizard.
 *
 * All state is a module-level singleton; components call `useIntakeFlow()` to get the same state.
 */

import { computed, reactive, ref } from "vue";
import { api, type Provider } from "../api/client.ts";
import type {
  IntakeCandidate,
  IntakeCandidateView,
  ImportError,
  ProbeResult,
  CandidateSource,
  RemoteImportResponse,
  RemotePreviewResponse,
} from "../api/intake-types.ts";
import { itemsFromText, type SmartIntakeItem } from "./use-smart-intake.ts";

export type IntakeFlowMode = "idle" | "shy" | "aggressive";

interface LootItem {
  label: string;
  targetProviderId: string | null;
}

interface IntakeFlowState {
  open: boolean;
  mode: IntakeFlowMode;
  probing: boolean;
  importing: boolean;
  loadingProviders: boolean;
  candidates: IntakeCandidateView[];
  providers: Provider[];
  results: ProbeResult[];
  /** key = `${candidateId}::${providerId}` */
  selections: Record<string, boolean>;
  toast: { tone: "ok" | "warn" | "bad"; text: string; detail?: string } | null;
  remotePreviewByCandidate: Record<string, RemotePreviewResponse | null>;
  remoteImportResultByCandidate: Record<string, RemoteImportResponse | null>;
  lootOpen: boolean;
  lootLabel: string;
  lootTargetProviderId: string | null;
  shyPromptOpen: boolean;
  shyCandidates: IntakeCandidateView[];
  error: string | null;
}

const state = reactive<IntakeFlowState>({
  open: false,
  mode: "idle",
  probing: false,
  importing: false,
  loadingProviders: false,
  candidates: [],
  providers: [],
  results: [],
  selections: {},
  toast: null,
  remotePreviewByCandidate: {},
  remoteImportResultByCandidate: {},
  lootOpen: false,
  lootLabel: "",
  lootTargetProviderId: null,
  shyPromptOpen: false,
  shyCandidates: [],
  error: null,
});

let toastTimer: number | null = null;
let lootTimer: number | null = null;
const lootQueue: LootItem[] = [];

function clearToastTimer() {
  if (toastTimer != null) {
    window.clearTimeout(toastTimer);
    toastTimer = null;
  }
}

function playNextLoot() {
  if (lootTimer != null) {
    return;
  }
  const next = lootQueue.shift();
  if (!next) return;
  state.lootLabel = next.label;
  state.lootTargetProviderId = next.targetProviderId;
  state.lootOpen = true;
  lootTimer = window.setTimeout(() => {
    state.lootOpen = false;
    state.lootTargetProviderId = null;
    lootTimer = window.setTimeout(() => {
      lootTimer = null;
      playNextLoot();
    }, 140);
  }, 820);
}

function triggerLoot(label: string, targetProviderId?: string | null) {
  lootQueue.push({ label, targetProviderId: targetProviderId ?? null });
  playNextLoot();
}

function showToast(tone: "ok" | "warn" | "bad", text: string, detail?: string) {
  clearToastTimer();
  state.toast = { tone, text, detail };
  toastTimer = window.setTimeout(
    () => {
      state.toast = null;
      toastTimer = null;
    },
    tone === "bad" ? 8000 : 4500,
  );
}

function sourceFromIntakeItem(item: SmartIntakeItem): CandidateSource {
  return item.source === "deeplink" ? "deeplink" : item.source;
}

function shorthand(value: string, head = 5, tail = 4): string {
  if (value.length <= head + tail + 1) return value;
  return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

function candidatePreview(c: IntakeCandidate): string {
  if (c.auth.type === "api-key") return shorthand(c.auth.value);
  if (c.auth.type === "auth-ref") return shorthand(c.auth.value);
  return shorthand(c.auth.access);
}

function candidateSummary(c: IntakeCandidate): string {
  const hintEmail = c.hints?.email;
  if (c.auth.type === "oauth") {
    if (hintEmail) return `${hintEmail} · OAuth`;
    return "OAuth";
  }
  if (c.auth.type === "api-key") return `API Key ${shorthand(c.auth.value)}`;
  return `auth_ref ${shorthand(c.auth.value)}`;
}

function intakeItemsToCandidates(items: SmartIntakeItem[]): IntakeCandidateView[] {
  const out: IntakeCandidateView[] = [];
  for (const item of items) {
    const source = sourceFromIntakeItem(item);
    if (item.kind === "api-key" && item.auth?.apiKey) {
      const cand: IntakeCandidate = {
        id: item.id,
        label: item.summary,
        auth: { type: "api-key", value: item.auth.apiKey },
        hints: {
          email: item.auth.email,
          subject: item.auth.subject,
          plan_slug: item.auth.plan,
        },
      };
      out.push({
        ...cand,
        source,
        summary: candidateSummary(cand),
        preview: candidatePreview(cand),
      });
      continue;
    }
    if (item.kind === "remote-provider" && item.text) {
      const cand: IntakeCandidate = {
        id: item.id,
        label: item.name,
        auth: { type: "api-key", value: item.text },
        hints: null,
      };
      out.push({
        ...cand,
        source,
        summary: item.name,
        preview: candidatePreview(cand),
        remoteText: item.text,
      });
      continue;
    }
    if (item.kind === "codex-auth" && item.auth?.access) {
      const cand: IntakeCandidate = {
        id: item.id,
        label: item.summary,
        auth: {
          type: "oauth",
          access: item.auth.access,
          refresh: item.auth.refresh,
          expires_at: item.auth.exp,
        },
        hints: {
          email: item.auth.email,
          subject: item.auth.subject,
          plan_slug: item.auth.plan,
        },
      };
      out.push({
        ...cand,
        source,
        summary: candidateSummary(cand),
        preview: candidatePreview(cand),
      });
    }
  }
  return out;
}

async function loadProviders(): Promise<Provider[]> {
  state.loadingProviders = true;
  try {
    const list = dedupeProvidersById(await api.providers.list());
    state.providers = list;
    return list;
  } catch (e) {
    state.error = e instanceof Error ? e.message : String(e);
    return [];
  } finally {
    state.loadingProviders = false;
  }
}

function selectionKey(candidateId: string, providerId: string): string {
  return `${candidateId}::${providerId}`;
}

function resetSelectionsAfterProbe() {
  const next: Record<string, boolean> = {};
  for (const r of state.results) {
    next[selectionKey(r.candidate_id, r.provider_id)] = r.ok;
  }
  state.selections = next;
}

function hasRemoteCandidate(): boolean {
  return state.candidates.some((cand) => !!cand.remoteText);
}

async function runProbe(): Promise<void> {
  if (hasRemoteCandidate()) {
    state.results = [];
    state.selections = {};
    return;
  }
  if (state.candidates.length === 0 || state.providers.length === 0) {
    state.results = [];
    return;
  }
  state.probing = true;
  state.error = null;
  try {
    const payload: IntakeCandidate[] = state.candidates.map(({ id, label, auth, hints }) => ({
      id,
      label,
      auth: { ...auth },
      hints: hints ? { ...hints } : null,
    }));
    console.debug("[intake] probe payload", payload);
    const r = await api.intake.probe({
      candidates: payload,
      provider_ids: state.providers.map((p) => p.id),
    });
    state.results = r.results;
    console.debug("[intake] probe results", r.results);
    resetSelectionsAfterProbe();
  } catch (e) {
    console.error("[intake] probe failed", e);
    state.error = e instanceof Error ? e.message : String(e);
    state.results = [];
  } finally {
    state.probing = false;
  }
}

async function importSelected(): Promise<{
  okCount: number;
  failCount: number;
  errors: ImportError[];
}> {
  const remoteItems = state.candidates.filter((cand) => cand.remoteText);
  if (remoteItems.length > 0) {
    let okCount = 0;
    let failCount = 0;
    const errors: ImportError[] = [];
    state.importing = true;
    state.error = null;
    try {
      for (const item of remoteItems) {
        try {
          const result = await api.intake.importRemote({ text: item.remoteText! });
          state.remoteImportResultByCandidate[item.id] = result;
          triggerLoot(result.preview.display_name, result.provider.id);
          okCount += 1;
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          failCount += 1;
          state.error = msg;
          errors.push({ candidate_id: item.id, provider_id: "remote", error: msg });
        }
      }
      return { okCount, failCount, errors };
    } finally {
      state.importing = false;
    }
  }
  const assignments = state.candidates.flatMap((cand) =>
    state.providers
      .filter((p) => state.selections[selectionKey(cand.id, p.id)])
      .map((p) => ({
        candidate: { id: cand.id, label: cand.label, auth: cand.auth, hints: cand.hints },
        provider_id: p.id,
        label: cand.hints?.email ?? cand.label ?? cand.summary,
        plan_type: cand.hints?.plan_slug ?? null,
      })),
  );
  if (assignments.length === 0) {
    return { okCount: 0, failCount: 0, errors: [] };
  }
  state.importing = true;
  state.error = null;
  try {
    const r = await api.intake.import({ assignments });
    return {
      okCount: r.credentials.length,
      failCount: r.errors.length,
      errors: r.errors,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    state.error = msg;
    return { okCount: 0, failCount: assignments.length, errors: [] };
  } finally {
    state.importing = false;
  }
}

async function previewRemoteCandidate(): Promise<void> {
  const remoteItems = state.candidates.filter((cand) => cand.remoteText);
  if (!remoteItems.length) return;
  state.probing = true;
  state.error = null;
  state.remotePreviewByCandidate = {};
  try {
    await Promise.all(
      remoteItems.map(async (item) => {
        try {
          state.remotePreviewByCandidate[item.id] = await api.intake.previewRemote({
            text: item.remoteText!,
          });
        } catch (e) {
          state.remotePreviewByCandidate[item.id] = null;
          if (!state.error) state.error = e instanceof Error ? e.message : String(e);
        }
      }),
    );
  } finally {
    state.probing = false;
  }
}

/** Browser event emitted after persistence so the Providers page can refresh the credential list. */
const INTAKE_IMPORTED_EVENT = "vibe:intake-imported";

async function confirmImport(): Promise<void> {
  const { okCount, failCount, errors } = await importSelected();
  if (okCount > 0) {
    const providerIds = hasRemoteCandidate()
      ? Object.values(state.remoteImportResultByCandidate)
          .map((x) => x?.provider.id)
          .filter(Boolean)
      : state.providers
          .filter((p) =>
            state.candidates.some((cand) => state.selections[selectionKey(cand.id, p.id)]),
          )
          .map((p) => p.id);
    window.dispatchEvent(new CustomEvent(INTAKE_IMPORTED_EVENT, { detail: { providerIds } }));
    if (!hasRemoteCandidate()) {
      triggerLoot(state.candidates[0]?.summary ?? "Saved");
    }
  }
  if (failCount === 0 && okCount > 0) {
    showToast("ok", hasRemoteCandidate() ? "Provider added" : `Added to ${okCount} providers`);
    closeWizard();
  } else if (okCount > 0) {
    const detail = errors.map((e) => `${e.provider_id}: ${e.error}`).join("\n");
    showToast("warn", `${okCount} succeeded · ${failCount} failed`, detail);
    closeWizard();
  } else {
    const detail = errors.map((e) => `${e.provider_id}: ${e.error}`).join("\n");
    showToast("bad", `All ${failCount} combinations failed to save`, detail);
  }
}

export const INTAKE_FLOW_IMPORTED_EVENT = INTAKE_IMPORTED_EVENT;

function openWizard(candidates: IntakeCandidateView[], mode: IntakeFlowMode) {
  const unique = dedupeIntakeCandidates(candidates);
  if (unique.length === 0) return;
  // Use a plain array to avoid tracking confusion from external reactive references.
  state.candidates = unique.map((c) => ({ ...c }));
  state.results = [];
  state.selections = {};
  state.error = null;
  state.remotePreviewByCandidate = {};
  state.remoteImportResultByCandidate = {};
  state.lootOpen = false;
  state.lootLabel = "";
  state.lootTargetProviderId = null;
  lootQueue.length = 0;
  if (lootTimer != null) {
    window.clearTimeout(lootTimer);
    lootTimer = null;
  }
  state.mode = mode;
  state.open = true;
  void (async () => {
    try {
      if (state.candidates.some((cand) => !!cand.remoteText)) {
        state.providers = [];
        await previewRemoteCandidate();
        return;
      }
      if (state.providers.length === 0) {
        await loadProviders();
      }
      await runProbe();
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      console.error("[intake] openWizard failed", e);
    }
  })();
}

function closeWizard() {
  state.open = false;
  state.mode = "idle";
}

function dismissShy() {
  state.shyPromptOpen = false;
  state.shyCandidates = [];
}

function acceptShy() {
  // Take a plain array copy to avoid stale reactive proxy references after state.shyCandidates is cleared.
  const candidates = state.shyCandidates.map((c) => ({ ...c }));
  state.shyPromptOpen = false;
  state.shyCandidates = [];
  if (candidates.length > 0) openWizard(candidates, "shy");
}

function toggleSelection(candidateId: string, providerId: string) {
  const key = selectionKey(candidateId, providerId);
  state.selections[key] = !state.selections[key];
}

function resultFor(candidateId: string, providerId: string): ProbeResult | undefined {
  return state.results.find((r) => r.candidate_id === candidateId && r.provider_id === providerId);
}

function isSelected(candidateId: string, providerId: string): boolean {
  return !!state.selections[selectionKey(candidateId, providerId)];
}

/** 仅探测成功的 candidate×provider 允许勾选（与 resetSelectionsAfterProbe 一致）。 */
function isSelectable(candidateId: string, providerId: string): boolean {
  const r = resultFor(candidateId, providerId);
  if (state.probing && !r) return false;
  if (!r) return false;
  if (r.skipped) return false;
  return r.ok;
}

function isChecked(candidateId: string, providerId: string): boolean {
  return isSelected(candidateId, providerId);
}

const successCount = computed(() => state.results.filter((r) => r.ok).length);
const remoteCandidate = computed(() => state.candidates.find((cand) => !!cand.remoteText) ?? null);
const remoteCandidates = computed(() => state.candidates.filter((cand) => !!cand.remoteText));
const selectedCount = computed(() =>
  remoteCandidates.value.length > 0
    ? remoteCandidates.value.length
    : Object.values(state.selections).filter(Boolean).length,
);
const candidateSuccessMap = computed(() => {
  const m = new Map<string, number>();
  for (const r of state.results) if (r.ok) m.set(r.candidate_id, (m.get(r.candidate_id) ?? 0) + 1);
  return m;
});

// ---------------------------------------------------------------------------
// Public flow entries
// ---------------------------------------------------------------------------

const lastSeenSignature = ref<string>("");

function signatureOf(candidates: IntakeCandidateView[]): string {
  return candidates
    .map((c) => {
      if (c.auth.type === "oauth") return `o:${c.auth.access}`;
      if (c.auth.type === "api-key") return `k:${c.auth.value}`;
      return `r:${c.auth.value}`;
    })
    .sort()
    .join("|");
}

/** 同一凭据/远程块只保留一条，避免剪贴板多段解析或重复粘贴导致重复探测与重复行。 */
function intakeCandidateDedupeKey(c: IntakeCandidateView): string {
  const rt = c.remoteText?.trim();
  if (rt) return `remote:${rt}`;
  const a = c.auth;
  if (a.type === "oauth") return `oauth:${a.access}`;
  if (a.type === "api-key") return `apikey:${a.value}`;
  return `authref:${a.value}`;
}

function dedupeIntakeCandidates(candidates: IntakeCandidateView[]): IntakeCandidateView[] {
  const seen = new Set<string>();
  const out: IntakeCandidateView[] = [];
  for (const c of candidates) {
    const k = intakeCandidateDedupeKey(c);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(c);
  }
  return out;
}

function dedupeProvidersById(list: Provider[]): Provider[] {
  const byId = new Map<string, Provider>();
  for (const p of list) {
    if (!byId.has(p.id)) byId.set(p.id, p);
  }
  return [...byId.values()];
}

function handlePaste(items: SmartIntakeItem[]) {
  const candidates = dedupeIntakeCandidates(intakeItemsToCandidates(items));
  if (candidates.length === 0) return false;
  lastSeenSignature.value = signatureOf(candidates);
  openWizard(candidates, "aggressive");
  return true;
}

function handleDrop(items: SmartIntakeItem[]) {
  return handlePaste(items);
}

async function tryShyClipboard(): Promise<void> {
  if (state.open || state.shyPromptOpen) return;
  if (typeof navigator === "undefined" || !navigator.clipboard?.readText) return;
  let text = "";
  try {
    text = await navigator.clipboard.readText();
  } catch {
    return;
  }
  if (!text.trim()) return;
  const items = itemsFromText(text, "clipboard");
  const candidates = dedupeIntakeCandidates(intakeItemsToCandidates(items));
  if (candidates.length === 0) return;
  const sig = signatureOf(candidates);
  if (sig === lastSeenSignature.value) return;
  lastSeenSignature.value = sig;
  state.shyCandidates = candidates;
  state.shyPromptOpen = true;
}

function openWithCandidates(items: SmartIntakeItem[]) {
  const candidates = dedupeIntakeCandidates(intakeItemsToCandidates(items));
  if (candidates.length === 0) return false;
  lastSeenSignature.value = signatureOf(candidates);
  openWizard(candidates, "aggressive");
  return true;
}

let shyFocusTimer: number | null = null;
let shyFocusBound = false;

function bindShyClipboardOnFocus() {
  if (shyFocusBound || typeof window === "undefined") return;
  shyFocusBound = true;
  const revisit = () => {
    if (shyFocusTimer != null) window.clearTimeout(shyFocusTimer);
    shyFocusTimer = window.setTimeout(() => {
      void tryShyClipboard();
    }, 900);
  };
  window.addEventListener("focus", revisit);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") revisit();
  });
}

export function useIntakeFlow() {
  return {
    state,
    open: computed(() => state.open),
    mode: computed(() => state.mode),
    candidates: computed(() => state.candidates),
    providers: computed(() => state.providers),
    results: computed(() => state.results),
    probing: computed(() => state.probing),
    importing: computed(() => state.importing),
    loadingProviders: computed(() => state.loadingProviders),
    toast: computed(() => state.toast),
    error: computed(() => state.error),
    shyPromptOpen: computed(() => state.shyPromptOpen),
    shyCandidates: computed(() => state.shyCandidates),
    successCount,
    selectedCount,
    remoteCandidate,
    remoteCandidates,
    remotePreviewByCandidate: computed(() => state.remotePreviewByCandidate),
    remoteImportResultByCandidate: computed(() => state.remoteImportResultByCandidate),
    lootOpen: computed(() => state.lootOpen),
    lootLabel: computed(() => state.lootLabel),
    lootTargetProviderId: computed(() => state.lootTargetProviderId),
    candidateSuccessMap,
    isSelected,
    isSelectable,
    isChecked,
    toggleSelection,
    resultFor,
    confirmImport,
    closeWizard,
    runProbe,
    handlePaste,
    handleDrop,
    tryShyClipboard,
    openWithCandidates,
    dismissShy,
    acceptShy,
    bindShyClipboardOnFocus,
  };
}

export const intakeFlowInternals = {
  intakeItemsToCandidates,
  signatureOf,
  dedupeIntakeCandidates,
  intakeCandidateDedupeKey,
};

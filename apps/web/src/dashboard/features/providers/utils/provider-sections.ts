import type {
  Provider,
  ProviderAuthPoolSummary,
  ProviderHealthSummary,
} from "../../../api/client.ts";
import {
  CLIENT_TOOLS,
  getToolProtocolSupport,
  type ClientToolInfo,
} from "../../../utils/client-tools.ts";
import { displayProviderName } from "../../../utils/providers-display.ts";
import type {
  ProviderCardProtocolBadge,
  ProviderCardView,
  ProviderGroupKey,
  ProviderSectionSummary,
  ProviderSectionView,
} from "../types.ts";

export type BuildProviderSectionsInput = {
  providers: Provider[];
  selectedTool: ClientToolInfo | null;
  healthMap: Record<string, ProviderHealthSummary>;
  poolByProviderId: Record<string, ProviderAuthPoolSummary>;
  fallbackGroupName?: string;
  text?: Partial<ProviderSectionText>;
};

export type ProviderSectionText = {
  bridge: string;
  credentialShort: string;
  fastest: (ms: number) => string;
  first: (ms: number) => string;
  models: string;
  native: string;
  noCredential: string;
  notTested: string;
  success: (pct: number) => string;
  tokensPerSecond: (value: string) => string;
};

const DEFAULT_TEXT: ProviderSectionText = {
  bridge: "bridge",
  credentialShort: "cred",
  fastest: (ms) => `${ms}ms best`,
  first: (ms) => `${ms}ms first`,
  models: "models",
  native: "native",
  noCredential: "no cred",
  notTested: "not tested",
  success: (pct) => `${pct}% ok`,
  tokensPerSecond: (value) => `${value} tok/s`,
};

function sectionText(input: BuildProviderSectionsInput): ProviderSectionText {
  return { ...DEFAULT_TEXT, ...input.text };
}

export function providerGroupName(provider: Provider, fallback = "Ungrouped"): string {
  const trimmed = provider.group_name?.trim();
  if (trimmed) return trimmed;
  return fallback;
}

export function providerGroupKey(provider: Provider, fallback?: string): string {
  return providerGroupName(provider, fallback).toLowerCase();
}

export function buildProviderSections(input: BuildProviderSectionsInput): ProviderSectionView[] {
  const cards = input.providers
    .map((provider) => rankProviderCard(provider, input))
    .filter((card) => {
      if (!input.selectedTool) return true;
      return input.selectedTool.consumesKinds.includes(card.provider.kind);
    })
    .sort((a, b) => a.sortKey.localeCompare(b.sortKey));

  const grouped = new Map<string, ProviderSectionView>();
  for (const card of cards) {
    const key = providerGroupKey(card.provider, input.fallbackGroupName);
    const title = providerGroupName(card.provider, input.fallbackGroupName);
    const section =
      grouped.get(key) ??
      ({
        key,
        title,
        description: "",
        summary: summarizeProviderSection([], input),
        providers: [],
      } satisfies ProviderSectionView);
    section.providers.push(card);
    grouped.set(key, section);
  }

  return [...grouped.values()]
    .map((section) => {
      const providers = section.providers.sort((a, b) => a.sortKey.localeCompare(b.sortKey));
      const summary = summarizeProviderSection(providers, input);
      return {
        ...section,
        providers,
        summary,
        description: providerSectionDescription(summary, sectionText(input)),
      };
    })
    .sort((a, b) => a.title.localeCompare(b.title, "zh-Hans-CN"));
}

function latencyCandidatesForProvider(
  provider: Provider,
  input: BuildProviderSectionsInput,
): number[] {
  const health = input.healthMap[provider.id];
  const values = [
    provider.last_speedtest?.latency_ms ?? null,
    health?.rolling?.avg_latency_ms ?? null,
    health?.cumulative.avg_latency_ms ?? null,
  ];
  return values.filter(
    (value): value is number => typeof value === "number" && Number.isFinite(value),
  );
}

function summarizeProviderSection(
  cards: ProviderCardView[],
  input: BuildProviderSectionsInput,
): ProviderSectionSummary {
  let enabledEndpoints = 0;
  let nativeEndpoints = 0;
  let bridgedEndpoints = 0;
  let availableCredentials = 0;
  let enabledCredentials = 0;
  let blockedCredentials = 0;
  let remoteModels = 0;
  let testedEndpoints = 0;
  let directEndpoints = 0;
  let wsEndpoints = 0;
  let passthroughEndpoints = 0;
  const latencies: number[] = [];

  for (const card of cards) {
    const provider = card.provider;
    const pool = input.poolByProviderId[provider.id];
    if (provider.enabled) enabledEndpoints += 1;
    if (card.group === "native") nativeEndpoints += 1;
    if (card.group === "bridged") bridgedEndpoints += 1;
    if (provider.passthrough_mode) passthroughEndpoints += 1;
    if (provider.supports_websocket === true) wsEndpoints += 1;
    if (provider.last_speedtest) testedEndpoints += 1;
    if (!provider.base_url.includes("127.0.0.1") && !provider.base_url.includes("localhost")) {
      directEndpoints += 1;
    }
    remoteModels += provider.remote_models?.length ?? 0;
    availableCredentials += pool?.available_credentials ?? 0;
    enabledCredentials += pool?.enabled_credentials ?? 0;
    blockedCredentials +=
      (pool?.rate_limited_credentials ?? 0) + (pool?.open_circuit_credentials ?? 0);
    latencies.push(...latencyCandidatesForProvider(provider, input));
  }

  return {
    totalEndpoints: cards.length,
    enabledEndpoints,
    nativeEndpoints,
    bridgedEndpoints,
    availableCredentials,
    enabledCredentials,
    blockedCredentials,
    fastestLatencyMs: latencies.length ? Math.min(...latencies) : null,
    remoteModels,
    testedEndpoints,
    directEndpoints,
    wsEndpoints,
    passthroughEndpoints,
  };
}

function providerSectionDescription(
  summary: ProviderSectionSummary,
  text: ProviderSectionText,
): string {
  const pieces = [
    summary.nativeEndpoints ? `${summary.nativeEndpoints} ${text.native}` : "",
    summary.bridgedEndpoints ? `${summary.bridgedEndpoints} ${text.bridge}` : "",
    summary.availableCredentials
      ? `${summary.availableCredentials}/${summary.enabledCredentials} ${text.credentialShort}`
      : text.noCredential,
    summary.fastestLatencyMs == null
      ? text.notTested
      : text.fastest(Math.round(summary.fastestLatencyMs)),
    summary.remoteModels ? `${summary.remoteModels} ${text.models}` : "",
  ].filter(Boolean);
  return pieces.join(" · ");
}

function providerCardBadges(provider: Provider): ProviderCardProtocolBadge[] {
  return CLIENT_TOOLS.filter((tool) => tool.consumesKinds.includes(provider.kind)).map((tool) => ({
    toolId: tool.id,
    toolLabel: tool.shortLabel,
    toolIcon: tool.icon,
    support: getToolProtocolSupport(provider, tool),
  }));
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value));
}

function providerCompositeScore(
  provider: Provider,
  input: BuildProviderSectionsInput,
): { score: number; reason: string } {
  const health = input.healthMap[provider.id];
  const rolling = health?.rolling ?? null;
  const pool = input.poolByProviderId[provider.id];
  const rollingTps = rolling?.decode_output_tokens_per_sec || rolling?.output_tokens_per_sec || 0;
  const tps = rollingTps;
  const successRate =
    rolling && rolling.requests > 0
      ? rolling.success_rate
      : (health?.cumulative.success_rate ?? (provider.enabled ? 1 : 0));
  const latencyMs =
    provider.last_speedtest?.latency_ms ??
    rolling?.avg_latency_ms ??
    health?.cumulative.avg_latency_ms ??
    null;
  const availableCreds = pool?.available_credentials ?? 0;
  const enabledCreds = pool?.enabled_credentials ?? 0;
  const circuitOpen = pool?.provider_circuit_open || health?.cumulative.circuit_state === "open";
  const rateLimited = pool?.rate_limited_credentials ?? 0;
  const openCreds = pool?.open_circuit_credentials ?? 0;
  const priorityScore = Math.max(0, 240 - provider.priority);
  const latencyScore = latencyMs == null ? 120 : 260 * (1 - clamp01(latencyMs / 5000));
  const speedScore = Math.min(360, tps * 10);
  const score =
    (provider.enabled ? 650 : -1600) +
    (circuitOpen ? -1200 : 250) +
    availableCreds * 180 +
    Math.min(180, enabledCreds * 40) -
    rateLimited * 120 -
    openCreds * 160 +
    successRate * 900 +
    latencyScore +
    speedScore +
    priorityScore;
  const reasonParts = [
    sectionText(input).success(Math.round(successRate * 100)),
    latencyMs == null ? "" : sectionText(input).first(Math.round(latencyMs)),
    tps ? sectionText(input).tokensPerSecond(tps.toFixed(1)) : "",
    availableCreds
      ? `${availableCreds} ${sectionText(input).credentialShort}`
      : sectionText(input).noCredential,
  ].filter(Boolean);
  return { score, reason: reasonParts.join(" · ") };
}

function rankProviderCard(provider: Provider, input: BuildProviderSectionsInput): ProviderCardView {
  const badges = providerCardBadges(provider);
  const title = displayProviderName(provider.name);
  const primarySupport = input.selectedTool
    ? getToolProtocolSupport(provider, input.selectedTool)
    : null;
  const firstUsefulSupport =
    primarySupport ??
    badges.map((badge) => badge.support).sort((a, b) => a.order - b.order)[0] ??
    null;

  let group: ProviderGroupKey = "other";
  if (primarySupport) {
    group =
      primarySupport.mode === "native"
        ? "native"
        : primarySupport.mode === "bridged"
          ? "bridged"
          : "other";
  } else {
    const hasNative = badges.some((badge) => badge.support.mode === "native");
    const hasBridged = badges.some((badge) => badge.support.mode === "bridged");
    group = hasNative ? "native" : hasBridged ? "bridged" : "other";
  }
  const quality = providerCompositeScore(provider, input);
  const normalizedTitle = title.toLocaleLowerCase("zh-Hans-CN");

  return {
    provider,
    title,
    badges,
    primarySupport: firstUsefulSupport,
    group,
    qualityScore: quality.score,
    sortReason: quality.reason,
    sortKey: `${provider.enabled ? "0" : "1"}:${normalizedTitle}:${provider.id}`,
  };
}

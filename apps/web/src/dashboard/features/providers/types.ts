import type { Provider } from "../../api/client.ts";
import type { ClientToolId, ProtocolSupportInfo } from "../../utils/client-tools.ts";

export type LiveRequestMetric = {
  request_id: string;
  provider_id: string;
  upstream_first_byte_ms: number | null;
  active_request_tokens_per_sec: number | null;
  active_upstream_decode_tps: number | null;
  active_downstream_emit_tps: number | null;
  updated_at: number;
};

export type ProviderGroupKey = "native" | "bridged" | "other";

export interface ProviderTabOption {
  id: "common" | ClientToolId;
  label: string;
  shortLabel: string;
  icon: string;
  description: string;
}

export interface ProviderCardProtocolBadge {
  toolId: ClientToolId;
  toolLabel: string;
  toolIcon: string;
  support: ProtocolSupportInfo;
}

export interface ProviderCardView {
  provider: Provider;
  title: string;
  badges: ProviderCardProtocolBadge[];
  primarySupport: ProtocolSupportInfo | null;
  group: ProviderGroupKey;
  qualityScore: number;
  sortReason: string;
  sortKey: string;
}

export interface ProviderSectionView {
  key: string;
  title: string;
  description: string;
  summary: ProviderSectionSummary;
  providers: ProviderCardView[];
}

export interface ProviderSectionSummary {
  totalEndpoints: number;
  enabledEndpoints: number;
  nativeEndpoints: number;
  bridgedEndpoints: number;
  availableCredentials: number;
  enabledCredentials: number;
  blockedCredentials: number;
  activeRequests: number;
  fastestLatencyMs: number | null;
  remoteModels: number;
  testedEndpoints: number;
  directEndpoints: number;
  wsEndpoints: number;
  passthroughEndpoints: number;
}

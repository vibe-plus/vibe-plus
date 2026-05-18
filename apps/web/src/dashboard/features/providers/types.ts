import type { Provider } from "../../api/client.ts";
import type { ClientToolId, ProtocolSupportInfo } from "../../utils/client-tools.ts";

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
  fastestLatencyMs: number | null;
  remoteModels: number;
  testedEndpoints: number;
  directEndpoints: number;
  wsEndpoints: number;
  passthroughEndpoints: number;
}

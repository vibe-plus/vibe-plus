/**
 * Shared smart-intake types corresponding to vibe-core/src/intake.rs.
 *
 * End-to-end protocol for credential detection -> parallel probing -> one-click persistence.
 */

import type { Credential } from "./client.ts";

export interface CandidateHints {
  email?: string | null;
  subject?: string | null;
  plan_slug?: string | null;
}

export type CandidateAuth =
  | { type: "api-key"; value: string }
  | { type: "auth-ref"; value: string }
  | {
      type: "oauth";
      access: string;
      refresh?: string | null;
      expires_at?: number | null;
    };

export type CandidateSource = "clipboard" | "paste" | "drop" | "deeplink";

export interface IntakeCandidate {
  id: string;
  label?: string | null;
  auth: CandidateAuth;
  hints?: CandidateHints | null;
}

export interface IntakeCandidateView extends IntakeCandidate {
  source: CandidateSource;
  /** Short UI description such as "sk-abc…1234" or "user@x · ChatGPT OAuth". */
  summary: string;
  /** Last 6 key characters for API-key candidates, used to distinguish same-source keys. */
  preview: string;
  remoteText?: string | null;
}

export interface ProbeInput {
  candidates: IntakeCandidate[];
  provider_ids?: string[];
  timeout_ms?: number;
}

export interface ProbeResult {
  candidate_id: string;
  provider_id: string;
  provider_name: string;
  provider_kind: string;
  ok: boolean;
  skipped: boolean;
  status: number | null;
  latency_ms: number;
  error: string | null;
  skip_reason: string | null;
}

export interface ProbeResponse {
  results: ProbeResult[];
}

export interface ImportAssignment {
  candidate: IntakeCandidate;
  provider_id: string;
  label?: string | null;
  plan_type?: string | null;
  priority?: number | null;
  notes?: string | null;
  enabled?: boolean | null;
}

export interface ImportInput {
  assignments: ImportAssignment[];
}

export interface ImportError {
  candidate_id: string;
  provider_id: string;
  error: string;
}

export interface ImportResponse {
  credentials: Credential[];
  errors: ImportError[];
}

export interface RemoteImportInput {
  text: string;
}

export interface ProviderBalanceSnapshot {
  currency: string;
  balance: string | null;
  remaining: string | null;
  used: string | null;
  total: string | null;
  period: string | null;
  note: string | null;
}

export interface RemoteProviderCapabilities {
  can_fetch_branding: boolean;
  can_fetch_models: boolean;
  can_fetch_balance: boolean;
  can_fetch_usage: boolean;
}

export interface RemoteDetectedProtocol {
  kind: string;
  label: string;
  base_url: string;
}

export interface RemotePreviewResponse {
  detected_kind: string;
  detected_base_url: string;
  detected_protocols: RemoteDetectedProtocol[];
  display_name: string;
  avatar_url: string | null;
  note: string;
  passthrough_mode: boolean;
  remote_models: string[];
  model_aliases: import("./client.ts").ModelAlias[];
  balance: ProviderBalanceSnapshot | null;
  usage: ProviderBalanceSnapshot | null;
  capabilities: RemoteProviderCapabilities;
  fetched_at: number;
}

export interface RemoteImportResponse {
  provider: import("./client.ts").Provider;
  credential: Credential | null;
  preview: RemotePreviewResponse;
}

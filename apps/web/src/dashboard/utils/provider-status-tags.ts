export type StatusTagTone = "ok" | "warn" | "bad" | "muted";

export interface StatusTag {
  key: string;
  label: string;
  tone: StatusTagTone;
}

export const STATUS_TAG_CLASS: Record<StatusTagTone, string> = {
  ok: "bg-emerald-50 text-emerald-700 ring-1 ring-emerald-200",
  warn: "bg-amber-50 text-amber-800 ring-1 ring-amber-200",
  bad: "bg-red-50 text-red-700 ring-1 ring-red-200",
  muted: "bg-slate-100 text-slate-600 ring-1 ring-slate-200",
};

export interface ProviderRowTagLabels {
  operational: string;
  paused: string;
  limited: string;
  circuit: string;
  recovering: string;
  degraded: string;
  readyCount: (count: number) => string;
  disabledCreds: (count: number) => string;
  noReady: string;
}

export interface ProviderRowTagInput {
  providerEnabled: boolean;
  circuit: string;
  availableCredentials: number;
  enabledCredentials: number;
  totalCredentials: number;
  rateLimitedCredentials: number;
  openCircuitCredentials: number;
  successRate: number;
  labels: ProviderRowTagLabels;
}

/** Build provider status tags for overview rows and provider cards. */
export function buildProviderRowTags(input: ProviderRowTagInput): StatusTag[] {
  const tags: StatusTag[] = [];

  if (!input.providerEnabled) {
    tags.push({ key: "paused", label: input.labels.paused, tone: "muted" });
    return tags;
  }

  if (input.circuit === "open") {
    tags.push({ key: "circuit", label: input.labels.circuit, tone: "bad" });
  } else if (input.circuit === "half-open") {
    tags.push({ key: "recovering", label: input.labels.recovering, tone: "warn" });
  }

  if (input.rateLimitedCredentials > 0) {
    tags.push({ key: "limited", label: input.labels.limited, tone: "warn" });
  }

  if (input.openCircuitCredentials > 0 && input.circuit === "closed") {
    tags.push({
      key: "circuit-creds",
      label: input.labels.circuit,
      tone: "bad",
    });
  }

  const disabledCreds = Math.max(0, input.totalCredentials - input.enabledCredentials);
  if (disabledCreds > 0) {
    tags.push({
      key: "disabled-creds",
      label: input.labels.disabledCreds(disabledCreds),
      tone: "muted",
    });
  }

  if (input.successRate < 0.9) {
    tags.push({ key: "degraded", label: input.labels.degraded, tone: "bad" });
  }

  if (input.availableCredentials > 0) {
    tags.push({
      key: "ready",
      label: input.labels.readyCount(input.availableCredentials),
      tone: "ok",
    });
  } else if (input.enabledCredentials > 0) {
    tags.push({ key: "no-ready", label: input.labels.noReady, tone: "warn" });
  }

  const issueKeys = new Set([
    "circuit",
    "recovering",
    "limited",
    "circuit-creds",
    "degraded",
    "no-ready",
    "disabled-creds",
  ]);
  const hasIssue = tags.some((tag) => issueKeys.has(tag.key));

  if (!hasIssue) {
    tags.unshift({ key: "operational", label: input.labels.operational, tone: "ok" });
  }

  return tags;
}

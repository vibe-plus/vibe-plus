import type { Credential, CredentialInput, Provider, ProviderInput } from "../../../api/client.ts";
import { protocolKeysForProvider, normalizeBaseUrl } from "../../../utils/provider-protocols.ts";

export const PROVIDER_CLIPBOARD_SCHEMA_VERSION = 1 as const;

export interface ProviderClipboardCredential {
  label: string;
  auth_ref: string | null;
  /** Stable dedupe id from gateway (`fp:…`), survives OAuth access-token refresh. */
  auth_fingerprint?: string | null;
  plan_type: string | null;
  notes: string | null;
  enabled: boolean;
  priority: number;
  oauth_access_token: string | null;
  oauth_expires_at: number | null;
  oauth_cached_email?: string | null;
  oauth_cached_subject?: string | null;
  oauth_cached_plan_slug?: string | null;
  upstream_vendor?: CredentialInput["upstream_vendor"];
  upstream_username?: string | null;
  upstream_group?: string | null;
  price_multiplier?: number;
  /** Secrets the API cannot export; user must re-paste on the target device. */
  missing_secrets?: string[];
}

export interface ProviderClipboardEntry {
  provider: ProviderInput;
  credentials: ProviderClipboardCredential[];
  portable_warnings?: string[];
}

export interface ProviderClipboardBundle {
  schemaVersion: typeof PROVIDER_CLIPBOARD_SCHEMA_VERSION;
  exportedAt: string;
  app: "vibe-plus";
  providers: ProviderClipboardEntry[];
}

export type ProviderClipboardImportCredentialAction = "create" | "skip";

export interface ProviderClipboardImportCredentialPlan {
  credential: ProviderClipboardCredential;
  action: ProviderClipboardImportCredentialAction;
  reason?: string;
}

export type ProviderClipboardImportProviderAction = "create" | "merge";

export interface ProviderClipboardImportPlanItem {
  entry: ProviderClipboardEntry;
  action: ProviderClipboardImportProviderAction;
  existingProviderId?: string;
  credentials: ProviderClipboardImportCredentialPlan[];
}

export interface ProviderClipboardImportPlan {
  items: ProviderClipboardImportPlanItem[];
  totals: {
    providersToCreate: number;
    providersToMerge: number;
    credentialsToCreate: number;
    credentialsToSkip: number;
    /** Nothing new would be written — bundle already matches this device. */
    inSync: boolean;
  };
}

export function providerToClipboardInput(provider: Provider): ProviderInput {
  return {
    name: provider.name,
    group_name: provider.group_name,
    avatar_url: provider.avatar_url,
    kind: provider.kind,
    base_url: provider.base_url,
    protocols: provider.protocols,
    host: provider.host ?? null,
    auth_ref: provider.auth_ref,
    enabled: provider.enabled,
    priority: provider.priority,
    supports_websocket: provider.supports_websocket,
    passthrough_mode: provider.passthrough_mode,
    model_aliases: provider.model_aliases,
  };
}

export function credentialToClipboardExport(credential: Credential): ProviderClipboardCredential {
  const missing: string[] = [];
  if (credential.oauth_has_refresh) missing.push("oauth_refresh_token");
  if (credential.upstream_has_session) missing.push("upstream_session");

  return {
    label: credential.label,
    auth_ref: credential.auth_ref,
    auth_fingerprint: credential.auth_fingerprint ?? null,
    plan_type: credential.plan_type,
    notes: credential.notes,
    enabled: credential.enabled,
    priority: credential.priority,
    oauth_access_token: credential.oauth_access_token,
    oauth_expires_at: credential.oauth_expires_at,
    oauth_cached_email: credential.oauth_account_email ?? null,
    oauth_cached_subject: credential.oauth_account_subject ?? null,
    oauth_cached_plan_slug: credential.oauth_chatgpt_plan_slug ?? null,
    upstream_vendor: credential.upstream_vendor ?? null,
    upstream_username: credential.upstream_username ?? null,
    upstream_group: credential.upstream_group ?? null,
    price_multiplier: credential.price_multiplier ?? 1,
    ...(missing.length > 0 ? { missing_secrets: missing } : {}),
  };
}

function portableWarningsForProvider(provider: ProviderInput): string[] {
  const warnings: string[] = [];
  const authRef = provider.auth_ref?.trim();
  if (authRef?.startsWith("keyring:") || authRef?.startsWith("env:")) {
    warnings.push(`provider_auth_ref:${authRef.split(":")[0]}`);
  }
  return warnings;
}

function portableWarningsForCredential(credential: ProviderClipboardCredential): string[] {
  const warnings: string[] = [];
  const authRef = credential.auth_ref?.trim();
  if (authRef?.startsWith("keyring:") || authRef?.startsWith("env:")) {
    warnings.push(`credential_auth_ref:${authRef.split(":")[0]}`);
  }
  if (credential.missing_secrets?.length) {
    warnings.push(...credential.missing_secrets.map((s) => `missing:${s}`));
  }
  return warnings;
}

export function buildProviderClipboardBundle(
  providers: Provider[],
  credsByProvider: Record<string, Credential[]>,
): ProviderClipboardBundle {
  const entries: ProviderClipboardEntry[] = providers.map((provider) => {
    const providerInput = providerToClipboardInput(provider);
    const credentials = (credsByProvider[provider.id] ?? []).map(credentialToClipboardExport);
    const portable_warnings = [
      ...portableWarningsForProvider(providerInput),
      ...credentials.flatMap(portableWarningsForCredential),
    ];
    return {
      provider: providerInput,
      credentials,
      ...(portable_warnings.length > 0
        ? { portable_warnings: [...new Set(portable_warnings)] }
        : {}),
    };
  });

  return {
    schemaVersion: PROVIDER_CLIPBOARD_SCHEMA_VERSION,
    exportedAt: new Date().toISOString(),
    app: "vibe-plus",
    providers: entries,
  };
}

export function serializeProviderClipboardBundle(bundle: ProviderClipboardBundle): string {
  return JSON.stringify(bundle, null, 2);
}

export function parseProviderClipboardBundle(text: string): ProviderClipboardBundle {
  const trimmed = text.trim();
  if (!trimmed) {
    throw new Error("clipboard_empty");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed) as unknown;
  } catch {
    throw new Error("invalid_json");
  }
  if (!isProviderClipboardBundle(parsed)) {
    throw new Error("invalid_bundle");
  }
  return parsed;
}

function isProviderClipboardBundle(value: unknown): value is ProviderClipboardBundle {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  if (record.schemaVersion !== PROVIDER_CLIPBOARD_SCHEMA_VERSION) return false;
  if (record.app !== "vibe-plus") return false;
  if (!Array.isArray(record.providers)) return false;
  return record.providers.every(isProviderClipboardEntry);
}

function isProviderClipboardEntry(value: unknown): value is ProviderClipboardEntry {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  if (!record.provider || typeof record.provider !== "object") return false;
  if (!Array.isArray(record.credentials)) return false;
  const provider = record.provider as Record<string, unknown>;
  return (
    typeof provider.name === "string" &&
    typeof provider.kind === "string" &&
    typeof provider.base_url === "string" &&
    typeof provider.enabled === "boolean" &&
    typeof provider.priority === "number" &&
    typeof provider.passthrough_mode === "boolean" &&
    Array.isArray(provider.model_aliases)
  );
}

export function clipboardCredentialToInput(
  credential: ProviderClipboardCredential,
): CredentialInput {
  return {
    label: credential.label,
    auth_ref: credential.auth_ref,
    plan_type: credential.plan_type,
    notes: credential.notes,
    enabled: credential.enabled,
    priority: credential.priority,
    oauth_access_token: credential.oauth_access_token,
    oauth_refresh_token: null,
    oauth_expires_at: credential.oauth_expires_at,
    oauth_cached_email: credential.oauth_cached_email ?? null,
    oauth_cached_subject: credential.oauth_cached_subject ?? null,
    oauth_cached_plan_slug: credential.oauth_cached_plan_slug ?? null,
    upstream_vendor: credential.upstream_vendor ?? null,
    upstream_username: credential.upstream_username ?? null,
    upstream_group: credential.upstream_group ?? null,
    price_multiplier: credential.price_multiplier ?? 1,
  };
}

function jwtSubForDedupe(token: string): string | null {
  const mid = token.split(".")[1];
  if (!mid) return null;
  try {
    const normalized = mid.replace(/-/g, "+").replace(/_/g, "/");
    const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
    const payload = JSON.parse(atob(padded)) as Record<string, unknown>;
    const sub = payload.sub;
    return typeof sub === "string" && sub.trim() ? sub.trim() : null;
  } catch {
    return null;
  }
}

function credentialIdentityKeys(
  credential: Pick<
    ProviderClipboardCredential,
    "auth_fingerprint" | "auth_ref" | "oauth_access_token" | "label"
  >,
): string[] {
  const keys: string[] = [];
  const fingerprint = credential.auth_fingerprint?.trim();
  if (fingerprint) keys.push(`fp:${fingerprint}`);
  const authRef = credential.auth_ref?.trim();
  if (authRef) keys.push(`auth:${authRef}`);
  const token = credential.oauth_access_token?.trim();
  if (token) {
    keys.push(`oauth:${token}`);
    const sub = jwtSubForDedupe(token);
    if (sub) keys.push(`oauth-sub:${sub}`);
  }
  keys.push(`label:${credential.label.trim().toLowerCase()}`);
  return keys;
}

function existingCredentialKeys(credentials: Credential[]): Set<string> {
  const keys = new Set<string>();
  for (const row of credentials) {
    for (const key of credentialIdentityKeys({
      auth_fingerprint: row.auth_fingerprint ?? null,
      auth_ref: row.auth_ref,
      oauth_access_token: row.oauth_access_token,
      label: row.label,
    })) {
      keys.add(key);
    }
  }
  return keys;
}

function credentialAlreadyExists(
  credential: ProviderClipboardCredential,
  seen: Set<string>,
): boolean {
  return credentialIdentityKeys(credential).some((key) => seen.has(key));
}

function providerMatchScore(entry: ProviderClipboardEntry, existing: Provider): number {
  const importKeys = protocolKeysForProvider(entry.provider);
  const existingKeys = protocolKeysForProvider(existing);
  const importKeySet = new Set(importKeys);
  const overlap = existingKeys.filter((key) => importKeySet.has(key)).length;
  if (overlap === 0) return -1;

  let score = overlap * 1000;

  const importName = entry.provider.name.trim().toLowerCase();
  if (existing.name.trim().toLowerCase() === importName) {
    score += 10_000;
  }

  if (normalizeBaseUrl(existing.base_url) === normalizeBaseUrl(entry.provider.base_url)) {
    score += 100;
  }

  if (
    overlap === importKeys.length &&
    overlap === existingKeys.length &&
    importKeys.every((key) => existingKeys.includes(key))
  ) {
    score += 500;
  }

  return score;
}

function findExistingProviderId(
  entry: ProviderClipboardEntry,
  existingProviders: Provider[],
): string | undefined {
  let best: Provider | undefined;
  let bestScore = -1;

  for (const provider of existingProviders) {
    const score = providerMatchScore(entry, provider);
    if (score > bestScore) {
      bestScore = score;
      best = provider;
    }
  }

  return best?.id;
}

export function planProviderClipboardImport(
  bundle: ProviderClipboardBundle,
  existingProviders: Provider[],
  existingCredsByProvider: Record<string, Credential[]>,
): ProviderClipboardImportPlan {
  const items: ProviderClipboardImportPlanItem[] = [];
  let providersToCreate = 0;
  let providersToMerge = 0;
  let credentialsToCreate = 0;
  let credentialsToSkip = 0;

  for (const entry of bundle.providers) {
    const existingProviderId = findExistingProviderId(entry, existingProviders);
    const action: ProviderClipboardImportProviderAction = existingProviderId ? "merge" : "create";
    if (action === "create") providersToCreate += 1;
    else providersToMerge += 1;

    const seen = existingProviderId
      ? existingCredentialKeys(existingCredsByProvider[existingProviderId] ?? [])
      : new Set<string>();

    const credentialPlans: ProviderClipboardImportCredentialPlan[] = [];
    for (const credential of entry.credentials) {
      if (credentialAlreadyExists(credential, seen)) {
        credentialPlans.push({ credential, action: "skip", reason: "duplicate" });
        credentialsToSkip += 1;
        continue;
      }
      credentialPlans.push({ credential, action: "create" });
      credentialsToCreate += 1;
      for (const key of credentialIdentityKeys(credential)) {
        seen.add(key);
      }
    }

    items.push({
      entry,
      action,
      existingProviderId,
      credentials: credentialPlans,
    });
  }

  return {
    items,
    totals: {
      providersToCreate,
      providersToMerge,
      credentialsToCreate,
      credentialsToSkip,
      inSync: providersToCreate === 0 && credentialsToCreate === 0,
    },
  };
}

export function bundleSummary(bundle: ProviderClipboardBundle): {
  providerCount: number;
  credentialCount: number;
  secretCount: number;
} {
  let credentialCount = 0;
  let secretCount = 0;
  for (const entry of bundle.providers) {
    credentialCount += entry.credentials.length;
    for (const cred of entry.credentials) {
      if (cred.auth_ref?.startsWith("literal:") || cred.oauth_access_token) secretCount += 1;
    }
    if (entry.provider.auth_ref?.startsWith("literal:")) secretCount += 1;
  }
  return {
    providerCount: bundle.providers.length,
    credentialCount,
    secretCount,
  };
}

/** Stable display key for import preview rows. */
export function providerDisplayKey(provider: Pick<ProviderInput, "kind" | "base_url">): string {
  return `${provider.kind} · ${normalizeBaseUrl(provider.base_url)}`;
}

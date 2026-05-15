/** Matches canonical UUID (any version). */
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const UNKNOWN_PROVIDER_LABEL = "unknown";

export function looksLikeUuid(s: string): boolean {
  return UUID_RE.test(s.trim());
}

/** When logs reference a provider row that no longer exists, DB falls back to raw id. */
export function formatUnknownProviderId(id: string): string {
  const t = id.trim();
  if (!t || looksLikeUuid(t)) return UNKNOWN_PROVIDER_LABEL;
  return t;
}

export function isUnknownProviderName(name: string | null | undefined): boolean {
  const t = (name ?? "").trim();
  return !t || looksLikeUuid(t) || t.toLowerCase().startsWith("unknown provider");
}

export function resolveProviderLabel(
  providerId: string,
  fallbackNameFromStats: string,
  nameById: ReadonlyMap<string, string>,
): string {
  const mapped = nameById.get(providerId)?.trim();
  if (mapped) return mapped;
  const fb = fallbackNameFromStats.trim();
  if (fb && !isUnknownProviderName(fb)) return fb;
  return formatUnknownProviderId(providerId);
}

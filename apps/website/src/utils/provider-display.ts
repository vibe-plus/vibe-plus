/** Matches canonical UUID (any version). */
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function looksLikeUuid(s: string): boolean {
  return UUID_RE.test(s.trim());
}

/** When logs reference a provider row that no longer exists, DB falls back to raw id. */
export function formatUnknownProviderId(id: string): string {
  const t = id.trim();
  if (looksLikeUuid(t)) {
    return `Unknown provider (…${t.slice(-8)})`;
  }
  return t.length > 0 ? t : "Unknown provider";
}

export function resolveProviderLabel(
  providerId: string,
  fallbackNameFromStats: string,
  nameById: ReadonlyMap<string, string>,
): string {
  const mapped = nameById.get(providerId)?.trim();
  if (mapped) return mapped;
  const fb = fallbackNameFromStats.trim();
  if (fb && !looksLikeUuid(fb)) return fb;
  return formatUnknownProviderId(providerId);
}

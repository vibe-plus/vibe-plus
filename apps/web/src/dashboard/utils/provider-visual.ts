/** Framework / relay gateways — shown after favicon, before protocol icons. */
export function frameworkIconFromBaseUrl(baseUrl: string | null | undefined): string | null {
  if (!baseUrl) return null;
  const lower = baseUrl.toLowerCase();
  if (lower.includes("sub2api")) return "i-[lucide--shuffle]";
  if (lower.includes("newapi") || lower.includes("new-api") || lower.includes("freeapi")) {
    return "i-[lucide--layers]";
  }
  return null;
}

export function hostFromUrlOrHost(input: string | null | undefined): string | null {
  if (!input?.trim()) return null;
  const trimmed = input.trim();
  try {
    if (trimmed.includes("://")) {
      return new URL(trimmed).hostname || null;
    }
  } catch {
    return null;
  }
  return (
    trimmed
      .replace(/^www\./i, "")
      .split("/")[0]
      ?.split(":")[0] ?? null
  );
}

export function faviconUrlForHost(host: string | null | undefined): string | null {
  if (!host?.trim()) return null;
  const domain = host.trim().replace(/^www\./i, "");
  return `https://www.google.com/s2/favicons?domain=${encodeURIComponent(domain)}&sz=64`;
}

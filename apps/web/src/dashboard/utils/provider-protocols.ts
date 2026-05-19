import type { LocalCandidate, Provider, ProviderKind, ProviderProtocol } from "../api/client.ts";

export function providerProtocolKinds(
  provider: Pick<Provider, "kind" | "protocols">,
): ProviderKind[] {
  const fromProtocols = (provider.protocols ?? [])
    .map((p) => p.kind)
    .filter((k): k is ProviderKind => !!k);
  if (fromProtocols.length) return [...new Set(fromProtocols)];
  return [provider.kind];
}

export function providerHasKind(
  provider: Pick<Provider, "kind" | "protocols">,
  kind: ProviderKind,
): boolean {
  return providerProtocolKinds(provider).includes(kind);
}

export function candidateProtocolKinds(
  candidate: Pick<LocalCandidate, "kind" | "protocols">,
): ProviderKind[] {
  const fromProtocols = (candidate.protocols ?? [])
    .map((p) => p.kind)
    .filter((k): k is ProviderKind => !!k);
  if (fromProtocols.length) return [...new Set(fromProtocols)];
  return [candidate.kind];
}

export function protocolKeysForProvider(
  provider: Pick<Provider, "kind" | "base_url" | "protocols">,
): string[] {
  const protos: ProviderProtocol[] = provider.protocols?.length
    ? provider.protocols
    : [{ kind: provider.kind, base_url: provider.base_url, model_aliases: [] }];
  return protos.map((p) => `${p.kind}|${normalizeBaseUrl(p.base_url)}`);
}

export function protocolKeysForCandidate(
  candidate: Pick<LocalCandidate, "kind" | "base_url" | "protocols">,
): string[] {
  const protos: ProviderProtocol[] = candidate.protocols?.length
    ? candidate.protocols
    : [{ kind: candidate.kind, base_url: candidate.base_url, model_aliases: [] }];
  return protos.map((p) => `${p.kind}|${normalizeBaseUrl(p.base_url)}`);
}

export function normalizeBaseUrl(url: string): string {
  return url.trim().replace(/\/+$/, "").toLowerCase();
}

import type { ProviderKind } from "../api/client.ts";

/** Human-facing wire protocol label (not the internal slug). */
export function protocolLabel(kind: string | null | undefined): string {
  switch (kind) {
    case "anthropic":
      return "Messages";
    case "openai-chat":
    case "openai-compat":
      return "Chat";
    case "openai-responses":
      return "Responses";
    case "gemini-native":
      return "Generate";
    default:
      if (!kind) return "Unknown";
      return String(kind);
  }
}

/** Compact wire marker used in dense tables: Messages / Responses / Chat / Generate. */
export function protocolWireLetter(kind: string | null | undefined): string {
  switch (kind) {
    case "anthropic":
      return "M";
    case "openai-responses":
      return "R";
    case "openai-chat":
    case "openai-compat":
      return "C";
    case "gemini-native":
      return "G";
    default:
      if (!kind) return "—";
      return protocolLabel(kind).charAt(0).toUpperCase() || String(kind).charAt(0).toUpperCase();
  }
}

export function protocolLabelsForProvider(provider: {
  kind: ProviderKind;
  protocols?: Array<{ kind: ProviderKind }> | null;
}): string[] {
  const protos =
    provider.protocols && provider.protocols.length > 0
      ? provider.protocols
      : [{ kind: provider.kind }];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const p of protos) {
    const label = protocolLabel(p.kind);
    if (seen.has(label)) continue;
    seen.add(label);
    out.push(label);
  }
  return out;
}

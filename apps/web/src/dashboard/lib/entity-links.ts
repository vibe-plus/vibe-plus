import type { RouteLocationRaw } from "vue-router";

/**
 * Generic entity-link mechanism.
 *
 * Anywhere in the dashboard that mentions an entity (provider, credential,
 * request, attempt, wave, …) should funnel through this registry instead of
 * hand-coding URLs. That keeps log lines, realtime cards, observability
 * panels, and future features in sync when routes move.
 *
 * The registry is intentionally additive: kinds we don't know how to route
 * yet (or whose destination page doesn't exist yet) return `null` from
 * `resolveEntityRoute` and render as plain chips/badges. Wiring up a new
 * destination is one switch-arm edit, nothing else.
 */

export type EntityKind = "provider" | "credential" | "request" | "attempt" | "wave";

export interface EntityRef {
  kind: EntityKind;
  id: string;
  label?: string | null;
}

export type EntityLinkResolver = (ref: EntityRef) => RouteLocationRaw | null;

const defaultResolvers: Record<EntityKind, EntityLinkResolver> = {
  provider: ({ id }) => (id ? { path: "/ui/providers", query: { provider: id } } : null),
  credential: ({ id }) => (id ? { path: "/ui/providers", query: { credential: id } } : null),
  request: ({ id }) =>
    // Destination tab ships in part-3 of the refactor; the chip stays
    // forward-compatible — once /ui/observability exists, the link lights up.
    id ? { path: "/ui/observability", query: { request: id } } : null,
  attempt: ({ id }) => (id ? { path: "/ui/observability", query: { attempt: id } } : null),
  wave: ({ id }) => (id ? { path: "/ui/observability", query: { wave: id } } : null),
};

let resolvers: Record<EntityKind, EntityLinkResolver> = { ...defaultResolvers };

export function registerEntityResolver(kind: EntityKind, resolver: EntityLinkResolver): void {
  resolvers = { ...resolvers, [kind]: resolver };
}

export function resetEntityResolvers(): void {
  resolvers = { ...defaultResolvers };
}

export function resolveEntityRoute(ref: EntityRef): RouteLocationRaw | null {
  if (!ref.id) return null;
  const resolver = resolvers[ref.kind];
  return resolver ? resolver(ref) : null;
}

export function resolveEntityLabel(ref: EntityRef, fallback?: string): string {
  return ref.label?.trim() || fallback?.trim() || ref.id;
}

/**
 * Token shape used by log/event renderers. Kept lossy on purpose: callers
 * either emit a `link` token (clickable) or a `text` token (plain).
 *
 * The same shape is what `app-log-renderer.ts` already produces, so this
 * file is the new home for that contract.
 */
export type EntityToken =
  | { type: "text"; text: string }
  | { type: "link"; text: string; to: RouteLocationRaw };

export function entityToken(ref: EntityRef, fallback?: string): EntityToken {
  const text = resolveEntityLabel(ref, fallback);
  const to = resolveEntityRoute(ref);
  return to ? { type: "link", text, to } : { type: "text", text };
}

/**
 * Best-effort regex scan for known entity ID patterns inside free-form text.
 *
 * Today we recognize:
 *   - UUID v4/v7   → request | attempt (caller must pick which kind by context)
 *   - `req_…`      → request
 *   - `att_…`      → attempt
 *   - `wave_…`     → wave
 *
 * The function returns the text broken into tokens. Callers that already
 * have structured payloads should prefer building tokens directly via
 * `entityToken()` — `linkifyText` is the fallback for legacy free-form
 * messages where we can only guess.
 */
const PATTERNS: Array<{ kind: EntityKind; re: RegExp }> = [
  { kind: "request", re: /\breq_[A-Za-z0-9]{6,}\b/g },
  { kind: "attempt", re: /\batt_[A-Za-z0-9]{6,}\b/g },
  { kind: "wave", re: /\bwave_[A-Za-z0-9]{4,}\b/g },
];

export function linkifyText(text: string, opts?: { uuidKind?: EntityKind }): EntityToken[] {
  if (!text) return [];

  type Match = { start: number; end: number; ref: EntityRef };
  const matches: Match[] = [];

  for (const { kind, re } of PATTERNS) {
    re.lastIndex = 0;
    for (const m of text.matchAll(re)) {
      const start = m.index ?? -1;
      if (start < 0) continue;
      matches.push({ start, end: start + m[0].length, ref: { kind, id: m[0] } });
    }
  }

  if (opts?.uuidKind) {
    const uuidRe =
      /\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b/g;
    for (const m of text.matchAll(uuidRe)) {
      const start = m.index ?? -1;
      if (start < 0) continue;
      matches.push({ start, end: start + m[0].length, ref: { kind: opts.uuidKind, id: m[0] } });
    }
  }

  if (matches.length === 0) return [{ type: "text", text }];

  matches.sort((a, b) => a.start - b.start);
  const tokens: EntityToken[] = [];
  let cursor = 0;
  for (const m of matches) {
    if (m.start < cursor) continue; // overlap — keep the earlier match
    if (m.start > cursor) tokens.push({ type: "text", text: text.slice(cursor, m.start) });
    tokens.push(entityToken(m.ref));
    cursor = m.end;
  }
  if (cursor < text.length) tokens.push({ type: "text", text: text.slice(cursor) });
  return tokens;
}

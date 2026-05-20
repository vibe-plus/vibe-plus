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

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isUuidEntityId(id: string): boolean {
  return UUID_RE.test(id.trim());
}

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
  if (!isUuidEntityId(ref.id)) return null;
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
 * Free-form linkification is intentionally disabled for new structured flows.
 * Old log text should be rendered as text only; structured entity IDs get
 * clickable links through `entityToken()`.
 */
export function linkifyText(text: string): EntityToken[] {
  return text ? [{ type: "text", text }] : [];
}

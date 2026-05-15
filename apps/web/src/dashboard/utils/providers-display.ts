import type { Credential, CredentialPoolStatus, CredentialPlanSnapshot } from "../api/client.ts";

function providerTitleInputToString(raw: unknown): string {
  if (raw == null) return "";
  if (typeof raw === "string") return raw.trim();
  if (typeof raw === "number" && Number.isFinite(raw)) return String(raw);
  if (typeof raw === "boolean") return raw ? "true" : "false";
  if (typeof raw === "bigint") return String(raw);
  if (typeof raw === "object") {
    const n = (raw as { name?: unknown }).name;
    if (typeof n === "string") return n.trim();
    if (typeof n === "number" && Number.isFinite(n)) return String(n);
    if (typeof n === "boolean") return n ? "true" : "false";
    if (typeof n === "bigint") return String(n);
    return "";
  }
  return "";
}

/** Normalize provider titles: collapse historical names like Codex (...) and Claude (...) into short names. */
export function displayProviderName(raw: unknown): string {
  const t = providerTitleInputToString(raw);
  if (!t) return "provider";
  const lower = t.toLowerCase();
  if (lower.startsWith("codex (")) return "Codex";
  if (lower.startsWith("claude (")) return "Claude";
  return t;
}

export function fingerprintDisplay(fp: string | null | undefined): string {
  if (!fp?.trim()) return "—";
  const t = fp.trim();
  if (t.length <= 20) return t;
  return `${t.slice(0, 16)}…`;
}

/** List and advanced areas: redact literal values and truncate other auth_ref values. */
export function authRefPreview(c: Credential): string {
  const r = c.auth_ref?.trim();
  if (!r) return "—";
  if (r.startsWith("literal:")) {
    const inner = r.slice("literal:".length).trim();
    if (inner.length <= 6) return "literal:****";
    return `literal:${inner.slice(0, 3)}…${inner.slice(-2)}`;
  }
  if (r.length > 52) return `${r.slice(0, 48)}…`;
  return r;
}

export function credentialAuthShort(c: Credential, row: CredentialPoolStatus | undefined): string {
  if (c.oauth_access_token || c.oauth_has_refresh) return "OAuth";
  if (row?.auth_mode === "oauth" || row?.auth_mode === "chatgpt") return "OAuth";
  if (row?.auth_mode) return row.auth_mode === "apikey" ? "API Key" : row.auth_mode;
  if (c.auth_ref) return "API Key";
  return "unconfigured";
}

/** Primary identity line: email -> JWT subject/user id -> credential label. */
export function credentialPrimaryAccountLabel(c: Credential): string {
  const mail = c.oauth_account_email?.trim();
  if (mail) return mail;
  const sub = c.oauth_account_subject?.trim();
  if (sub) return sub;
  const lab = c.label?.trim();
  return lab || "credential";
}

/** Plan tier inside JWT as lowercase slug; secondary information only, null when absent. */
export function credentialJwtPlanSlugDisplay(c: Credential): string | null {
  const s = c.oauth_chatgpt_plan_slug?.trim().toLowerCase();
  return s || null;
}

function titleCaseSlug(slug: string): string {
  if (!slug) return "";
  return slug.charAt(0).toUpperCase() + slug.slice(1);
}

/** Short tier label paired with usage bars, without prefixes such as ChatGPT. */
export function credentialPlanTierHint(c: Credential): string | null {
  const slug = credentialJwtPlanSlugDisplay(c);
  if (slug) return titleCaseSlug(slug);
  const pt = c.plan_type?.trim();
  if (!pt) return null;
  if (pt.toLowerCase() === "codex-pro") return null;
  return pt;
}

/** Whether to hide DB `plan_type` as duplicate noise from JWT, such as historical codex-pro imports. */
export function shouldHideDbPlanTypeChip(c: Credential): boolean {
  const p = c.plan_type?.trim().toLowerCase();
  if (p !== "codex-pro") return false;
  return !!credentialJwtPlanSlugDisplay(c) || !!(c.oauth_access_token || c.oauth_has_refresh);
}

export type StatusTone = "ok" | "warn" | "bad";

export function mergedPoolStatus(
  c: Credential,
  row: CredentialPoolStatus | undefined,
): { ok: boolean; text: string; tone: StatusTone } {
  if (!c.enabled) return { ok: false, text: "disabled", tone: "warn" };
  if (!row) return { ok: true, text: "enabled", tone: "ok" };
  if (row.circuit_open) return { ok: false, text: "circuit:open", tone: "bad" };
  if (row.circuit_state === "half-open")
    return { ok: false, text: "circuit:half-open", tone: "warn" };
  if (row.is_rate_limited) return { ok: false, text: "rate_limited", tone: "bad" };
  return { ok: true, text: "ok", tone: "ok" };
}

/** Avoid obscure text when the gateway has not returned this credential row in the pool yet. */
export function poolRowMissingLabel(): string {
  return "metrics:pending";
}

export function primaryPlanPercent(snap: CredentialPlanSnapshot | null | undefined): {
  pct: number | null;
  windowLabel: string | null;
} {
  if (!snap) return { pct: null, windowLabel: null };
  const pick = (
    v: number | null | undefined,
    label: string,
  ): { pct: number; windowLabel: string } | null => {
    if (v == null || Number.isNaN(v)) return null;
    return { pct: Math.min(100, Math.max(0, v)), windowLabel: label };
  };
  return (
    pick(snap.codex_primary_used_percent, "W") ??
    pick(snap.codex_5h_used_percent, "5h") ??
    pick(snap.codex_7d_used_percent, "7d") ?? { pct: null, windowLabel: null }
  );
}

export function planPctClass(p: number | null | undefined): string {
  if (p == null || Number.isNaN(p)) return "bg-gray-600";
  if (p < 60) return "bg-emerald-500";
  if (p < 85) return "bg-yellow-500";
  return "bg-red-500";
}

export function rlPercent(remaining: number | null, limit: number | null): number {
  if (remaining == null || limit == null || limit === 0) return 100;
  return Math.round((remaining / limit) * 100);
}

export function rlClass(pct: number): string {
  if (pct > 50) return "bg-emerald-500";
  if (pct > 20) return "bg-yellow-500";
  return "bg-red-500";
}

export function fmtTs(ts: number | null): string {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleTimeString();
}

/** Collapse common backend/English fragments into short labels; unknown values pass through unchanged. */
export function mapUpstreamUserMessage(msg: string | null | undefined): string | null {
  if (!msg?.trim()) return null;
  const t = msg.trim();
  const lower = t.toLowerCase();
  if (lower.includes("not found") && lower.includes("credential")) return "credential:not_found";
  if (/no such credential/i.test(t)) return "credential:not_found";
  if (/pool.*empty|empty.*pool/i.test(t)) return "pool:empty";
  if (/fingerprint|duplicate/i.test(t) && /same|duplicate|conflict/i.test(lower))
    return "fingerprint:duplicate";
  return t;
}

export function lastErrorSummary(
  c: Credential,
  row: CredentialPoolStatus | undefined,
): string | null {
  const a = row?.last_error?.trim();
  const b = c.last_error?.trim();
  if (a && b && a !== b) {
    const ma = mapUpstreamUserMessage(a);
    const mb = mapUpstreamUserMessage(b);
    return `${ma ?? a} · ${mb ?? b}`;
  }
  const single = a || b || null;
  return mapUpstreamUserMessage(single) ?? single;
}

export function isDupFingerprint(c: Credential, creds: Credential[] | undefined): boolean {
  if (!creds?.length || !c.auth_fingerprint) return false;
  return creds.filter((x) => x.auth_fingerprint === c.auth_fingerprint).length > 1;
}

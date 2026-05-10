import type { Credential, CredentialPoolStatus, CredentialPlanSnapshot } from "../api/client.ts";

/** 统一供应商标题：历史「Codex (…）」「Claude (…）」等收敛短名。 */
export function displayProviderName(raw: string | null | undefined): string {
  const t = raw?.trim() ?? "";
  if (!t) return "上游";
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

/** 列表与高级区：literal 脱敏，其它 auth_ref 截断。 */
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
  return "未配置";
}

/** 主行身份：邮箱 → JWT 主体/用户 id → 凭证标签。 */
export function credentialPrimaryAccountLabel(c: Credential): string {
  const mail = c.oauth_account_email?.trim();
  if (mail) return mail;
  const sub = c.oauth_account_subject?.trim();
  if (sub) return sub;
  const lab = c.label?.trim();
  return lab || "凭证";
}

/** JWT 内套餐档位（小写 slug），仅作次要信息；无则 null。 */
export function credentialJwtPlanSlugDisplay(c: Credential): string | null {
  const s = c.oauth_chatgpt_plan_slug?.trim().toLowerCase();
  return s || null;
}

function titleCaseSlug(slug: string): string {
  if (!slug) return "";
  return slug.charAt(0).toUpperCase() + slug.slice(1);
}

/** 与用量条搭配的简短「档位」文案（不含 ChatGPT 等前缀）。 */
export function credentialPlanTierHint(c: Credential): string | null {
  const slug = credentialJwtPlanSlugDisplay(c);
  if (slug) return titleCaseSlug(slug);
  const pt = c.plan_type?.trim();
  if (!pt) return null;
  if (pt.toLowerCase() === "codex-pro") return null;
  return pt;
}

/** 是否将 DB `plan_type` 当作与 JWT 重复的噪音隐藏（历史导入 codex-pro）。 */
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
  if (!c.enabled) return { ok: false, text: "已关闭", tone: "warn" };
  if (!row) return { ok: true, text: "已启用", tone: "ok" };
  if (row.circuit_open) return { ok: false, text: "熔断", tone: "bad" };
  if (row.circuit_state === "half-open") return { ok: false, text: "探测中", tone: "warn" };
  if (row.is_rate_limited) return { ok: false, text: "限流", tone: "bad" };
  return { ok: true, text: "可用", tone: "ok" };
}

/** 网关尚未返回该凭证在池中的行时，避免「池暂无此条」等晦涩文案。 */
export function poolRowMissingLabel(): string {
  return "无实时指标";
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
    pick(snap.codex_primary_used_percent, "主窗口") ??
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

/** 将常见后端/英文片段收成简短中文，未知则原样返回。 */
export function mapUpstreamUserMessage(msg: string | null | undefined): string | null {
  if (!msg?.trim()) return null;
  const t = msg.trim();
  const lower = t.toLowerCase();
  if (lower.includes("not found") && lower.includes("credential")) return "凭证未找到或已删除";
  if (/no such credential/i.test(t)) return "凭证不存在";
  if (/pool.*empty|empty.*pool/i.test(t)) return "密钥池暂无可用条目";
  if (/fingerprint|duplicate/i.test(t) && /same|duplicate|conflict/i.test(lower))
    return "与其它凭证冲突（可能重复导入）";
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

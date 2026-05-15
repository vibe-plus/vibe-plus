/** Decode ChatGPT-style JWT payload (no signature verify) — mirrors vibe-core `chatgpt_oauth_hints_from_access_token`. */

function b64UrlToUtf8(segment: string): string {
  let s = segment.replace(/-/g, "+").replace(/_/g, "/");
  while (s.length % 4 !== 0) s += "=";
  const bin = atob(s);
  try {
    return decodeURIComponent(
      Array.from(bin, (c) => `%${`00${c.charCodeAt(0).toString(16)}`.slice(-2)}`).join(""),
    );
  } catch {
    return bin;
  }
}

function jwtPayloadJson(jwt: string): Record<string, unknown> | null {
  const mid = jwt.split(".")[1];
  if (!mid) return null;
  try {
    const raw = b64UrlToUtf8(mid);
    const v = JSON.parse(raw) as unknown;
    return typeof v === "object" && v !== null && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function planSlugFromAuthJson(auth: unknown): string | null {
  if (typeof auth !== "object" || auth === null) return null;
  const a = auth as Record<string, unknown>;
  const pt = a.chatgpt_plan_type;
  if (typeof pt === "string" && pt.trim()) return pt.trim().toLowerCase();
  if (typeof pt === "object" && pt !== null) {
    const vals = Object.values(pt as Record<string, unknown>);
    const s = vals.find((x) => typeof x === "string") as string | undefined;
    return s?.trim().toLowerCase() ?? null;
  }
  return null;
}

/** Same fields persisted as `CredentialInput.oauth_cached_*` when importing Codex `auth.json`. */
export function chatgptHintsFromJwt(jwt: string | null | undefined): {
  oauth_cached_email: string | null;
  oauth_cached_subject: string | null;
  oauth_cached_plan_slug: string | null;
} {
  const empty = {
    oauth_cached_email: null as string | null,
    oauth_cached_subject: null as string | null,
    oauth_cached_plan_slug: null as string | null,
  };
  if (!jwt?.trim()) return empty;
  const v = jwtPayloadJson(jwt.trim());
  if (!v) return empty;

  const emailTop = typeof v.email === "string" ? v.email.trim() : "";
  const profile = v["https://api.openai.com/profile"];
  const emailProfile =
    typeof profile === "object" &&
    profile !== null &&
    typeof (profile as Record<string, unknown>).email === "string"
      ? String((profile as Record<string, unknown>).email).trim()
      : "";

  const authRaw = v["https://api.openai.com/auth"];
  const auth = typeof authRaw === "object" && authRaw !== null ? authRaw : null;
  const authObj = auth as Record<string, unknown> | null;

  const sub = typeof v.sub === "string" ? v.sub.trim() : "";
  const uid =
    authObj &&
    (typeof authObj.chatgpt_user_id === "string"
      ? authObj.chatgpt_user_id.trim()
      : typeof authObj.user_id === "string"
        ? String(authObj.user_id).trim()
        : "");

  const plan = authObj ? planSlugFromAuthJson(authObj) : null;

  return {
    oauth_cached_email: emailTop || emailProfile || null,
    oauth_cached_subject: sub || uid || null,
    oauth_cached_plan_slug: plan,
  };
}

export function hintsFromAuthJsonTokens(tokens: unknown): ReturnType<typeof chatgptHintsFromJwt> {
  if (typeof tokens !== "object" || tokens === null) {
    return chatgptHintsFromJwt(null);
  }
  const id = (tokens as Record<string, unknown>).id_token;
  return chatgptHintsFromJwt(typeof id === "string" ? id : null);
}

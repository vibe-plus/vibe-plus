const CANDIDATES = [
  "https://vibe-plus.github.io/vibe-plus",
  "https://vibe-plus.cheez.tech/vibe-plus",
] as const;

const CACHE_KEY = "vp-cdn-origin";
const CACHE_TS_KEY = "vp-cdn-ts";
const CACHE_TTL = 60 * 60 * 1000; // 1 hour

async function probeMs(origin: string): Promise<number> {
  const t = Date.now();
  const res = await fetch(`${origin}/version.json`, { cache: "no-cache" });
  if (!res.ok) throw new Error(`probe failed: ${res.status}`);
  return Date.now() - t;
}

export async function pickFastestOrigin(): Promise<string> {
  const cached = localStorage.getItem(CACHE_KEY);
  const cachedTs = Number(localStorage.getItem(CACHE_TS_KEY) ?? 0);
  if (cached && Date.now() - cachedTs < CACHE_TTL) return cached;

  const results = await Promise.allSettled(
    CANDIDATES.map((origin) => probeMs(origin).then((ms) => ({ origin, ms }))),
  );

  type probe_row = { origin: (typeof CANDIDATES)[number]; ms: number };
  const fulfilled: probe_row[] = [];
  for (const r of results) {
    if (r.status === "fulfilled") fulfilled.push(r.value);
  }
  const winner = fulfilled.sort((a, b) => a.ms - b.ms)[0];

  const best = winner?.origin ?? CANDIDATES[0];
  localStorage.setItem(CACHE_KEY, best);
  localStorage.setItem(CACHE_TS_KEY, String(Date.now()));
  return best;
}

/** Redirect to the fastest CDN origin if we're not already there. No-op in local dev. */
export async function redirectToFastestCDN(
  path = `${window.location.pathname}${window.location.search}${window.location.hash}`,
): Promise<void> {
  const { hostname } = window.location;
  const isGhPages = hostname === "vibe-plus.github.io";
  const isCheezTech = hostname === "vibe-plus.cheez.tech";
  if (!isGhPages && !isCheezTech) return; // local dev or custom domain — skip

  const best = await pickFastestOrigin();
  if (!window.location.href.startsWith(best)) {
    window.location.replace(`${best}${path}`);
  }
}

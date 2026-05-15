/**
 * Compatibility epoch expected by this Web UI.
 *
 * Bump `WEB_COMPAT_API` when the dashboard starts relying on gateway endpoints,
 * fields, or semantics that older `vibe` CLI binaries do not provide. This is
 * intentionally decoupled from the Web package version so the dashboard can ship
 * frequently without forcing a CLI release every time.
 */
export const WEB_COMPAT_API = 1;

export function compareSemver(a: string, b: string): number {
  const parse = (value: string) =>
    value
      .trim()
      .replace(/^v/i, "")
      .split(/[.-]/)
      .slice(0, 3)
      .map((part) => {
        const n = Number.parseInt(part, 10);
        return Number.isFinite(n) ? n : 0;
      });
  const aa = parse(a);
  const bb = parse(b);
  for (let i = 0; i < 3; i += 1) {
    const diff = (aa[i] ?? 0) - (bb[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

export function cliSatisfiesMinimum(current: string | null | undefined, minimum: string): boolean {
  return !!current && compareSemver(current, minimum) >= 0;
}
